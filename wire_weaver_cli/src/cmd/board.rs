//! `ww board`: tools around `ww_board_info::BoardInfo` (TOOL-28). `rework set|clear` records a rework done on the
//! bench, on any device serving `BoardInfo::set_reworks`, through a global trait attachment (API-20). `rework
//! allow|disallow` works on a board already in its boot ROM instead, storing or clearing the allowance STD-101's
//! guard reads at boot (TOOL-30).

use anyhow::{Result, anyhow, bail};
use clap::{Args, Subcommand};
use console::style;
use wire_weaver::prelude::MaxLen;
use wire_weaver_client::ww_client_server::ErrorKindOwned;
use wire_weaver_client::{CallError, DynClient, Error};
use ww_board_info::guard::StoredAllowance;
use ww_board_info::table::{IdRanges, ReworkSet};
use ww_board_info::{ReworkChange, ReworkErrorOwned, ReworkId, ReworkState};
use ww_report::clients::BoardInfoClient;
use ww_report::rework_check::{
    RomFlash, allowance_line, board_guard, read_board, scan_installed_image, set_allowance,
};

use crate::cmd::rom::{self, DfuseRom, PicobootRom, RP_SCAN, STM32_FLASH, STM32_SCAN};
use crate::device::Selection;

#[derive(Subcommand)]
pub(crate) enum BoardCommand {
    /// Record reworks done on the bench (STD-98), or allow/disallow unknown ones on a board in its boot ROM
    /// (STD-101, TOOL-30)
    #[command(subcommand)]
    Rework(ReworkCommand),
}

#[derive(Subcommand)]
pub(crate) enum ReworkCommand {
    /// Mark reworks applied on this unit
    ///
    /// All of them or none: a number the firmware doesn't know is refused, nothing is stored. The board runs on
    /// the record from its next boot (`ww info` then shows it).
    Set(ReworkArgs),

    /// Mark reworks undone (not applied) on this unit
    ///
    /// Undoing a number the firmware doesn't know is refused too: it can't judge it.
    Clear(ReworkArgs),

    /// On a board in its boot ROM (BOOTSEL / the STM32 bootloader): let the installed image run with these unknown
    /// reworks (STD-101)
    ///
    /// Bound to the installed image's build id: a different image flashed later ignores it, and the next `ww`
    /// flashing that needs none clears it. The numbers must cover every rework `ww info` lists as unknown there
    /// (covering more is fine); giving fewer is refused, naming what is left out.
    Allow(AllowArgs),

    /// On a board in its boot ROM: clear a stored allowance (STD-101)
    Disallow(DisallowArgs),
}

#[derive(Args)]
pub(crate) struct ReworkArgs {
    /// Rework numbers, comma separated (`7` or `1,3,5`), as listed by `ww info`
    #[arg(required = true, value_delimiter = ',')]
    ids: Vec<u16>,

    /// Check without writing: every number is checked, nothing is stored
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
pub(crate) struct AllowArgs {
    /// Rework numbers, comma separated (`7` or `1,3,5`), covering every one `ww info` lists as unknown
    #[arg(required = true, value_delimiter = ',')]
    ids: Vec<u16>,

    /// Check without writing
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
pub(crate) struct DisallowArgs {
    /// Check without writing
    #[arg(long)]
    dry_run: bool,
}

pub(crate) async fn board(
    cmd: BoardCommand,
    device: Option<&DynClient>,
    selection: &Selection,
) -> Result<()> {
    match cmd {
        BoardCommand::Rework(ReworkCommand::Set(args)) => {
            set_reworks(args, true, device.unwrap()).await
        }
        BoardCommand::Rework(ReworkCommand::Clear(args)) => {
            set_reworks(args, false, device.unwrap()).await
        }
        BoardCommand::Rework(ReworkCommand::Allow(args)) => {
            rom_allowance(selection, Some(&args.ids), args.dry_run).await
        }
        BoardCommand::Rework(ReworkCommand::Disallow(args)) => {
            rom_allowance(selection, None, args.dry_run).await
        }
    }
}

/// `ww board rework allow N...` (`ids: Some`) or `disallow` (`ids: None`), on whichever boot ROM [rom::find_rom]
/// finds matching `selection`.
async fn rom_allowance(selection: &Selection, ids: Option<&[u16]>, dry_run: bool) -> Result<()> {
    let Some(target) = rom::find_rom(selection).await? else {
        bail!(
            "no board in its boot ROM matches {selection}: `ww board rework allow` and `disallow` work only on a \
             board in BOOTSEL or the STM32 bootloader; reboot it there first (`ww dfu rp reboot --bootsel`, \
             `ww dfu rp flash`/`ww dfu stm32 flash --no-reboot` on a running device, or hold BOOTSEL while \
             plugging it in)"
        );
    };
    match &target {
        rom::RomTarget::Rp { serial } => {
            let mut pb = rom::open_rp(serial).await?;
            pb.exclusive_access(true, false).await?;
            pb.exit_xip().await?;
            let r = in_rom(
                &mut PicobootRom(&mut pb),
                wire_weaver_client::ww_picoboot::proto::FLASH_BASE,
                RP_SCAN,
                ids,
                dry_run,
            )
            .await;
            pb.exclusive_access(false, false).await?;
            r
        }
        rom::RomTarget::Stm32 { serial } => {
            let mut dfu = rom::open_stm32(serial).await?;
            if !dfu.layout().readable(STM32_FLASH, 1) {
                bail!(
                    "the STM32 bootloader doesn't read its flash (readout protection?): the allowance can't be \
                     read or written"
                );
            }
            in_rom(
                &mut DfuseRom(&mut dfu),
                STM32_FLASH,
                STM32_SCAN,
                ids,
                dry_run,
            )
            .await
        }
    }
}

/// The allowance check and write/clear, with the chip in its boot ROM.
async fn in_rom(
    rom: &mut impl RomFlash,
    flash_base: u32,
    scan_len: u32,
    ids: Option<&[u16]>,
    dry_run: bool,
) -> Result<()> {
    let image = scan_installed_image(rom, flash_base, scan_len).await?;
    let Some(board) = read_board(rom, &image, None, None).await? else {
        bail!("the installed image names no bench record: nothing to allow");
    };
    match ids {
        Some(ids) => {
            let Some(id) = image.id else {
                bail!("the installed image keeps no build id note: no allowance can be its own");
            };
            let wanted = ids
                .iter()
                .fold(ReworkSet::EMPTY, |s, &n| s.with(ReworkId(n), true));
            let unknown = board_guard(&image, &board).unknown();
            if let Err(missing) = covers_unknown(wanted, unknown) {
                bail!(
                    "rework(s) {}: leaves out {} the installed image doesn't know; `ww board rework allow {}`",
                    IdRanges(wanted),
                    IdRanges(missing),
                    numbers(unknown)
                );
            }
            if dry_run {
                println!(
                    "{} would allow rework(s) {} for image {id} (dry run, nothing written)",
                    style("?").yellow(),
                    IdRanges(wanted)
                );
                return Ok(());
            }
            let allowance = StoredAllowance::new(id, wanted);
            let written = set_allowance(rom, &board, allowance).await?;
            println!(
                "{} rework(s) {} allowed for image {id}{}",
                style("\u{2714}").green(),
                IdRanges(wanted),
                if written { "" } else { " (already stored)" }
            );
        }
        None => {
            if dry_run {
                println!(
                    "{} would clear the stored allowance ({}) (dry run, nothing written)",
                    style("?").yellow(),
                    allowance_line(&board.allowance)
                );
                return Ok(());
            }
            if set_allowance(rom, &board, StoredAllowance::NONE).await? {
                println!(
                    "{} the stored allowance is cleared",
                    style("\u{2714}").green()
                );
            } else {
                println!("{} no allowance was stored", style("=").dim());
            }
        }
    }
    Ok(())
}

/// `wanted` must name every number of `unknown` (covering more is fine, decision 6 of the rework-guard design):
/// `Err` is what it leaves out.
fn covers_unknown(wanted: ReworkSet, unknown: ReworkSet) -> Result<(), ReworkSet> {
    let missing = ReworkSet(unknown.0 & !wanted.0);
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

/// `7,9`, as `ww board rework allow` takes them.
fn numbers(set: ReworkSet) -> String {
    set.iter()
        .map(|id| id.0.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

async fn set_reworks(args: ReworkArgs, applied: bool, device: &DynClient) -> Result<()> {
    let client = BoardInfoClient::new(device);
    let changes: Vec<ReworkState> = args
        .ids
        .iter()
        .map(|&id| ReworkState {
            id: ReworkId(id),
            applied,
        })
        .collect();
    let result = client
        .set_reworks(MaxLen::new_unchecked(changes), !args.dry_run)
        .call()
        .await;
    match result {
        Ok(change) => {
            println!("{}", change_text(change, &args.ids, applied));
            Ok(())
        }
        Err(CallError::Device(ReworkErrorOwned::UnknownRework { id })) => {
            let known = known_rework_ids(&client).await;
            bail!("unknown rework {id}: this firmware knows {known}; update firmware first")
        }
        Err(CallError::Device(e)) => bail!("{e}"),
        Err(CallError::Unimplemented { resource }) => {
            bail!("the device doesn't implement {resource}")
        }
        Err(CallError::Client(e)) => Err(not_found(e)),
    }
}

/// The numbers the firmware's own table knows, from `REWORKS` (`title` is `Some` for those), `1-5, 8` or `none`.
async fn known_rework_ids(client: &BoardInfoClient) -> String {
    let Ok(list) = client.read_REWORKS().read().await else {
        return "unknown".to_string();
    };
    let set = list
        .iter()
        .filter(|r| r.title.is_some())
        .fold(ReworkSet::EMPTY, |s, r| s.with(r.id, true));
    if set.is_empty() {
        "none".to_string()
    } else {
        IdRanges(set).to_string()
    }
}

fn change_text(change: ReworkChange, ids: &[u16], applied: bool) -> String {
    let ids = ids
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let (verb, past) = if applied {
        ("apply", "applied")
    } else {
        ("undo", "undone")
    };
    match change {
        ReworkChange::Unchanged => format!("{} rework(s) {ids}: already {past}", style("=").dim()),
        ReworkChange::WouldWrite => format!(
            "{} would {verb} rework(s) {ids} (dry run, nothing written)",
            style("?").yellow()
        ),
        ReworkChange::Written => format!(
            "{} {past} rework(s) {ids}; the board runs on it from the next boot",
            style("\u{2714}").green()
        ),
        other => format!("{other:?}"),
    }
}

fn not_found(e: Error) -> anyhow::Error {
    match &e {
        Error::RemoteError(r) if matches!(r.kind, ErrorKindOwned::TraitNotFound) => anyhow!(
            "the device doesn't implement ww_board_info::BoardInfo (or not in a compatible version)"
        ),
        _ => e.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestArgs {
        #[command(flatten)]
        args: ReworkArgs,
    }

    #[test]
    fn ids_are_comma_separated_and_dry_run_is_a_flag() {
        let t = TestArgs::try_parse_from(["x", "1,3,5", "--dry-run"]).unwrap();
        assert_eq!(t.args.ids, [1, 3, 5]);
        assert!(t.args.dry_run);
    }

    #[test]
    fn at_least_one_id_is_required() {
        assert!(TestArgs::try_parse_from(["x"]).is_err());
    }

    #[test]
    fn change_text_of_each_outcome() {
        console::set_colors_enabled(false);
        assert_eq!(
            change_text(ReworkChange::Unchanged, &[7], true),
            "= rework(s) 7: already applied"
        );
        assert_eq!(
            change_text(ReworkChange::Unchanged, &[5, 8], false),
            "= rework(s) 5, 8: already undone"
        );
        assert_eq!(
            change_text(ReworkChange::WouldWrite, &[7], true),
            "? would apply rework(s) 7 (dry run, nothing written)"
        );
        assert_eq!(
            change_text(ReworkChange::WouldWrite, &[5], false),
            "? would undo rework(s) 5 (dry run, nothing written)"
        );
        assert_eq!(
            change_text(ReworkChange::Written, &[7], true),
            "\u{2714} applied rework(s) 7; the board runs on it from the next boot"
        );
        assert_eq!(
            change_text(ReworkChange::Written, &[5], false),
            "\u{2714} undone rework(s) 5; the board runs on it from the next boot"
        );
    }

    fn set(ids: &[u16]) -> ReworkSet {
        ids.iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
    }

    #[test]
    fn covers_unknown_exactly_or_with_extra() {
        assert_eq!(covers_unknown(set(&[7, 9]), set(&[7, 9])), Ok(()));
        // covering more than needed is fine (StoredAllowance decision 6)
        assert_eq!(covers_unknown(set(&[7, 9, 11]), set(&[7, 9])), Ok(()));
        assert_eq!(covers_unknown(set(&[]), set(&[])), Ok(()));
        assert_eq!(covers_unknown(set(&[7]), set(&[7, 9])), Err(set(&[9])));
    }

    #[test]
    fn numbers_are_comma_joined() {
        assert_eq!(numbers(set(&[9])), "9");
        assert_eq!(numbers(set(&[7, 9])), "7,9");
        assert_eq!(numbers(ReworkSet::EMPTY), "");
    }

    #[derive(Parser)]
    struct TestAllowArgs {
        #[command(flatten)]
        args: AllowArgs,
    }

    #[derive(Parser)]
    struct TestDisallowArgs {
        #[command(flatten)]
        args: DisallowArgs,
    }

    #[test]
    fn allow_ids_are_comma_separated_and_dry_run_is_a_flag() {
        let t = TestAllowArgs::try_parse_from(["x", "7,9", "--dry-run"]).unwrap();
        assert_eq!(t.args.ids, [7, 9]);
        assert!(t.args.dry_run);
        assert!(TestAllowArgs::try_parse_from(["x"]).is_err());
    }

    #[test]
    fn disallow_takes_only_dry_run() {
        let t = TestDisallowArgs::try_parse_from(["x", "--dry-run"]).unwrap();
        assert!(t.args.dry_run);
        assert!(
            !TestDisallowArgs::try_parse_from(["x"])
                .unwrap()
                .args
                .dry_run
        );
    }
}
