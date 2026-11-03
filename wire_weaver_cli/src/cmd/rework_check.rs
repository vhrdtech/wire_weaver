//! The rework check of `ww dfu rp flash` and `ww dfu stm32 flash` (TOOL-29): before writing, the board's applied
//! reworks are compared with the reworks the new image knows (`ww_report::rework_check`). Given the serial of a
//! running device, the flasher reads the board there, stores or clears the allowance and reboots the device into its
//! boot ROM itself, so the check always runs before anything is written.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Result, bail};
use clap::Args;
use console::style;
use wire_weaver_client::follow::Found;
use wire_weaver_client::{ClientMeta, DynClient};
use ww_board_info::ImageId;
use ww_board_info::table::{IdRanges, ReworkSet};
use ww_dfu::{BootTarget, Profile};
use ww_report::clients::BoardInfoClient;
use ww_report::rework_check::{
    BoardRead, BoardReworks, ImageReworks, Verdict, clear_allowance, guard_line, read_board,
    store_allowance, verdict,
};

use crate::device::Selection;
use crate::dfu_client::Dfu;
use crate::untrusted::escape;

#[derive(Args, Default)]
pub(crate) struct ReworkCheckArgs {
    /// Let the image run the board with exactly these reworks it doesn't know (`7` or `7,9`), without asking
    ///
    /// The allowance is stored on the running device for this image's build id before it is written. The numbers
    /// must be the ones the check finds lacking, no more, no fewer.
    #[arg(long, value_delimiter = ',', value_name = "N,..")]
    pub allow_unknown_reworks: Vec<u16>,

    /// Write an image for another board, from before the rework guard, or that boots guarded, without asking
    #[arg(long)]
    pub yes: bool,
}

impl ReworkCheckArgs {
    fn allowed(&self) -> ReworkSet {
        self.allow_unknown_reworks
            .iter()
            .fold(ReworkSet::EMPTY, |s, &id| {
                s.with(ww_board_info::ReworkId(id), true)
            })
    }
}

/// The boot ROM a flasher writes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Rom {
    Rp,
    Stm32,
}

impl Rom {
    fn is(self, found: &Found) -> bool {
        match self {
            Rom::Rp => matches!(found, Found::RpBootsel(_)),
            Rom::Stm32 => matches!(found, Found::Stm32Dfu(_)),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Rom::Rp => "BOOTSEL",
            Rom::Stm32 => "the STM32 bootloader",
        }
    }
}

/// Run the check before a boot-ROM flash of `flash` (the image's flash contents). With a boot ROM of the `serial`
/// present already, the board can't be read: a note, nothing else. With a running WireWeaver device of the selection
/// instead: check, store or clear the allowance, reboot it into its boot ROM, and return its full serial to wait for.
pub(crate) async fn before_rom_flash(
    rom: Rom,
    flash: &[u8],
    args: &ReworkCheckArgs,
    selection: &Selection,
    serial: Option<&str>,
) -> Result<Option<String>> {
    let image = ImageReworks::from_flash(flash);
    let not_checked = |why: &str| {
        if image.guards() || !args.allow_unknown_reworks.is_empty() {
            eprintln!(
                "{} {why}: the board's reworks are not checked; the image boots guarded if it lacks one applied (`ww info` shows it)",
                style("note").yellow().bold()
            );
        }
    };
    let Some(serial) = serial else {
        not_checked("no --serial of a running device");
        return Ok(None);
    };
    let devices: Vec<Found> = wire_weaver_client::list_all_usb_devices()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(Found::classify)
        .collect();
    let has_serial = |f: &Found| {
        f.info()
            .serials
            .iter()
            .any(|s| s.to_lowercase().contains(&serial.to_lowercase()))
    };
    if devices.iter().any(|f| rom.is(f) && has_serial(f)) {
        not_checked(&format!("{serial} is in {} already", rom.name()));
        return Ok(None);
    }
    let config = selection.client_config();
    let running: Vec<&Found> = devices
        .iter()
        .filter(|f| matches!(f, Found::WireWeaver(_)) && config.matches(f.info()))
        .collect();
    let device = match running.as_slice() {
        [] => return Ok(None),
        [one] => one.info(),
        many => bail!(
            "{} running devices match --serial {serial}, give more of it: {}",
            many.len(),
            many.iter()
                .map(|f| escape(f.info().serials.first().map_or("?", |s| s.as_str())))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let Some(full_serial) = device.serials.first().cloned() else {
        bail!(
            "the running device has no serial number to follow into {}",
            rom.name()
        );
    };
    let meta = ClientMeta::new();
    let purpose = match rom {
        Rom::Rp => "ww dfu rp flash",
        Rom::Stm32 => "ww dfu stm32 flash",
    };
    let exclusive = crate::plugin::take_exclusive(selection, &meta, purpose).await?;
    let client = match DynClient::from_config(config.traits_only().client_meta(meta))
        .connect()
        .await
    {
        Ok(client) => client,
        // on its way into the boot ROM already (`ww dfu reboot` just before), say
        Err(e) => {
            not_checked(&format!(
                "connecting to {} failed ({e:#})",
                escape(&full_serial)
            ));
            if let Some(exclusive) = exclusive {
                _ = exclusive.release().await;
            }
            return Ok(None);
        }
    };
    let r = check_and_reboot(rom, &image, args, &client).await;
    _ = client.disconnect().asynch().await;
    if let Some(exclusive) = exclusive
        && let Err(e) = exclusive.release().await
    {
        eprintln!("{} {e:#}", style("note").yellow());
    }
    r?;
    Ok(Some(full_serial))
}

async fn check_and_reboot(
    rom: Rom,
    image: &ImageReworks,
    args: &ReworkCheckArgs,
    device: &DynClient,
) -> Result<()> {
    // the reboot has to reach the boot ROM: checked before the allowance is stored
    let dfu = Dfu::new(device);
    if let Ok(node) = dfu.node.node().call().await
        && !matches!(node.profile, Profile::AB(_))
    {
        bail!(
            "the device updates through an updater of its own, not {}: use `ww dfu flash`",
            rom.name()
        );
    }
    let board = BoardInfoClient::new(device);
    match read_board(&board).await? {
        BoardRead::Board(b) => check(image, &b, args, &board).await?,
        BoardRead::NoBoardInfo => {
            eprintln!(
                "{} the device doesn't serve ww_board_info: no reworks to check",
                style("note").dim()
            )
        }
        BoardRead::NoReworks { name, revision } => eprintln!(
            "{} {} {}: the running firmware doesn't report reworks (from before STD-98); the image boots guarded if it lacks one applied",
            style("note").yellow().bold(),
            escape(&name),
            escape(&revision)
        ),
    }
    dfu.boot
        .reboot(BootTarget::Updater)
        .call()
        .await
        .map_err(|e| anyhow::anyhow!("rebooting the device into {}: {e:?}", rom.name()))?;
    eprintln!("{} rebooting into {}", style("✔").green(), rom.name());
    Ok(())
}

/// Compare, ask, then store or clear the allowance: nothing is written to flash before this returns `Ok`.
async fn check(
    image: &ImageReworks,
    board: &BoardReworks,
    args: &ReworkCheckArgs,
    client: &BoardInfoClient,
) -> Result<()> {
    let v = verdict(image, board);
    eprintln!(
        "{} {} {}, reworks {}{}",
        style("board").bold(),
        escape(&board.name),
        escape(&board.revision),
        if board.applied.is_empty() {
            "none".to_string()
        } else {
            IdRanges(board.applied).to_string()
        },
        board
            .guard
            .as_ref()
            .map(|g| format!(", {}", guard_line(g)))
            .unwrap_or_default()
    );
    let step = plan(&v, board.guard.is_some(), args);
    let allow = match step {
        Step::Write { allow } => allow,
        Step::Refuse(why) => bail!("{why}; nothing written"),
        Step::Ask { question, allow } => {
            eprintln!("{} {v}", style("warning").yellow().bold());
            if !ask(&question)? {
                bail!("nothing written");
            }
            allow
        }
    };
    match allow {
        Some((id, reworks)) => {
            store_allowance(client, id, reworks).await?;
            eprintln!(
                "{} rework(s) {} allowed for image {id}",
                style("✔").green(),
                IdRanges(reworks)
            );
        }
        None => {
            if let Some(ww_board_info::ReworkChange::Written) = clear_allowance(client).await? {
                eprintln!("{} the stored allowance is cleared", style("✔").green());
            }
        }
    }
    Ok(())
}

/// What to do about a verdict.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// Write, storing this allowance first (else clearing a stored one).
    Write {
        allow: Option<(ImageId, ReworkSet)>,
    },
    /// Ask the user, then as [Step::Write].
    Ask {
        question: String,
        allow: Option<(ImageId, ReworkSet)>,
    },
    Refuse(String),
}

/// `can_store`: the running firmware serves the guard (and so `allow_unknown_reworks`).
fn plan(v: &Verdict, can_store: bool, args: &ReworkCheckArgs) -> Step {
    let allowed = args.allowed();
    let flag = |set: ReworkSet| format!("--allow-unknown-reworks {}", Ids(set));
    if let Verdict::Lacking {
        lacking,
        image: Some(id),
    } = v
        && can_store
    {
        let allow = Some((*id, *lacking));
        return if allowed == *lacking {
            Step::Write { allow }
        } else if !allowed.is_empty() {
            Step::Refuse(format!(
                "{v}; the image lacks exactly {}: {}",
                IdRanges(*lacking),
                flag(*lacking)
            ))
        } else {
            Step::Ask {
                question: format!(
                    "allow rework(s) {} for this image and write it?",
                    IdRanges(*lacking)
                ),
                allow,
            }
        };
    }
    if !allowed.is_empty() {
        let why = match v {
            Verdict::Lacking { image: Some(_), .. } => {
                "the running firmware can't store an allowance (from before the rework guard)"
            }
            Verdict::Lacking { image: None, .. } => "the image has no build id to allow them for",
            _ => "the image lacks no rework",
        };
        return Step::Refuse(format!("{v}; --allow-unknown-reworks: {why}"));
    }
    match v {
        Verdict::Ok => Step::Write { allow: None },
        _ if args.yes => Step::Write { allow: None },
        Verdict::Lacking { .. } => Step::Ask {
            question: "write it anyway? it boots guarded (allow the reworks for it with `ww board rework allow` once it runs)".into(),
            allow: None,
        },
        _ => Step::Ask {
            question: "write it anyway?".into(),
            allow: None,
        },
    }
}

/// `7,9`, as `--allow-unknown-reworks` takes them.
struct Ids(ReworkSet);

impl std::fmt::Display for Ids {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<String> = self.0.iter().map(|id| id.0.to_string()).collect();
        f.write_str(&ids.join(","))
    }
}

/// A yes/no question on the terminal; refused without one (a script passes the flags instead).
fn ask(question: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!(
            "{question} (no terminal to ask on: pass --allow-unknown-reworks with the numbers, or --yes for the other warnings)"
        );
    }
    eprint!("{question} [y/N] ");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_board_info::ReworkId;
    use ww_board_info::image::ImageTableError;

    const ID: ImageId = ImageId([0x3f, 0xa2, 1, 2, 3, 4, 5, 6]);

    fn set(ids: &[u16]) -> ReworkSet {
        ids.iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
    }

    fn args(allow: &[u16], yes: bool) -> ReworkCheckArgs {
        ReworkCheckArgs {
            allow_unknown_reworks: allow.to_vec(),
            yes,
        }
    }

    fn lacking(ids: &[u16], image: Option<ImageId>) -> Verdict {
        Verdict::Lacking {
            lacking: set(ids),
            image,
        }
    }

    #[test]
    fn ok_writes_and_clears() {
        assert_eq!(
            plan(&Verdict::Ok, true, &args(&[], false)),
            Step::Write { allow: None }
        );
        assert!(matches!(
            plan(&Verdict::Ok, true, &args(&[7], false)),
            Step::Refuse(_)
        ));
    }

    #[test]
    fn lacking_is_asked_or_allowed_by_exact_numbers() {
        let v = lacking(&[7, 9], Some(ID));
        assert_eq!(
            plan(&v, true, &args(&[], false)),
            Step::Ask {
                question: "allow rework(s) 7, 9 for this image and write it?".into(),
                allow: Some((ID, set(&[7, 9])))
            }
        );
        // --yes doesn't allow reworks: only their numbers do
        assert!(matches!(plan(&v, true, &args(&[], true)), Step::Ask { .. }));
        assert_eq!(
            plan(&v, true, &args(&[9, 7], false)),
            Step::Write {
                allow: Some((ID, set(&[7, 9])))
            }
        );
        let Step::Refuse(why) = plan(&v, true, &args(&[7], false)) else {
            panic!("fewer numbers")
        };
        assert!(why.ends_with("--allow-unknown-reworks 7,9"), "{why}");
        assert!(matches!(
            plan(&v, true, &args(&[7, 9, 10], false)),
            Step::Refuse(_)
        ));
    }

    #[test]
    fn lacking_without_an_allowance_boots_guarded() {
        // firmware from before the guard, or an image without a build id: nothing to store
        for (v, can_store) in [
            (lacking(&[7], Some(ID)), false),
            (lacking(&[7], None), true),
        ] {
            assert!(matches!(
                plan(&v, can_store, &args(&[], false)),
                Step::Ask { allow: None, .. }
            ));
            assert_eq!(
                plan(&v, can_store, &args(&[], true)),
                Step::Write { allow: None }
            );
            assert!(matches!(
                plan(&v, can_store, &args(&[7], false)),
                Step::Refuse(_)
            ));
        }
    }

    #[test]
    fn other_board_old_image_and_bad_record_need_a_yes() {
        for v in [
            Verdict::OtherBoard {
                boards: "B135 A".into(),
            },
            Verdict::NoRecord { applied: set(&[3]) },
            Verdict::BadRecord(ImageTableError::Corrupt),
        ] {
            assert_eq!(
                plan(&v, true, &args(&[], false)),
                Step::Ask {
                    question: "write it anyway?".into(),
                    allow: None
                }
            );
            assert_eq!(
                plan(&v, true, &args(&[], true)),
                Step::Write { allow: None }
            );
        }
    }

    #[test]
    fn numbers_are_comma_separated() {
        use clap::Parser;
        #[derive(Parser)]
        struct T {
            #[command(flatten)]
            a: ReworkCheckArgs,
        }
        let t = T::try_parse_from(["x", "--allow-unknown-reworks", "7,9", "--yes"]).unwrap();
        assert_eq!(t.a.allow_unknown_reworks, [7, 9]);
        assert!(t.a.yes);
    }
}
