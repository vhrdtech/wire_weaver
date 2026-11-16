//! The rework check of `ww dfu rp flash` and `ww dfu stm32 flash` (TOOL-29): before writing, the board's applied
//! reworks are compared with the reworks the new image knows (`ww_report::rework_check`). It runs with the chip in
//! its boot ROM, the board's record read straight from flash: a running device of the `--serial` is rebooted there
//! first ([before_rom_flash]), one already there is checked as it is ([in_rom]). The allowance is written or
//! cleared there too, before the image.

use std::collections::BTreeMap;
use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Result, bail};
use clap::Args;
use console::style;
use wire_weaver_client::follow::Found;
use wire_weaver_client::{ClientMeta, DynClient};
use ww_board_info::ReworkId;
use ww_board_info::guard::{ImageId, StoredAllowance};
use ww_board_info::table::{IdRanges, ReworkSet};
use ww_dfu::{BootTarget, Profile};
use ww_report::clients::BoardInfoClient;
use ww_report::rework_check::{
    ImageReworks, RomFlash, Verdict, allowance_line, read_board, scan_installed, set_allowance,
    verdict,
};

use crate::device::Selection;
use crate::dfu_client::Dfu;
use crate::untrusted::escape;

#[derive(Args, Default)]
pub(crate) struct ReworkCheckArgs {
    /// Let the image run the board with exactly these reworks it doesn't know (`7` or `7,9`), without asking
    ///
    /// The allowance is stored on the board for this image's build id, through the boot ROM, before the image is
    /// written. The numbers must be the ones the check finds lacking, no more, no fewer.
    #[arg(long, value_delimiter = ',', value_name = "N,..")]
    pub allow_unknown_reworks: Vec<u16>,

    /// Write an image for another board, from before the rework guard, or that refuses to boot, without asking
    #[arg(long)]
    pub yes: bool,

    /// The board's name (`B200`), when neither the image in flash nor the new one names exactly one
    #[arg(long, value_name = "NAME")]
    pub board: Option<String>,
}

impl ReworkCheckArgs {
    fn allowed(&self) -> ReworkSet {
        self.allow_unknown_reworks
            .iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
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

/// Titles of the board's reworks, from a running firmware's `REWORKS`, for the question.
pub(crate) type Titles = BTreeMap<ReworkId, String>;

/// A running WireWeaver device of the `serial`: read the titles of its reworks and reboot it into its boot ROM;
/// `Some((full serial to wait for, titles))`. `None` without a serial, or when the device is in the boot ROM
/// already or not found: the check then runs on whatever is in the ROM.
pub(crate) async fn before_rom_flash(
    rom: Rom,
    selection: &Selection,
    serial: Option<&str>,
) -> Result<Option<(String, Titles)>> {
    let Some(serial) = serial else {
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
        Err(e) => {
            if let Some(exclusive) = exclusive {
                _ = exclusive.release().await;
            }
            bail!("connecting to {} failed: {e:#}", escape(&full_serial));
        }
    };
    let r = titles_and_reboot(rom, &client).await;
    _ = client.disconnect().asynch().await;
    if let Some(exclusive) = exclusive
        && let Err(e) = exclusive.release().await
    {
        eprintln!("{} {e:#}", style("note").yellow());
    }
    let titles = r?;
    Ok(Some((full_serial, titles)))
}

async fn titles_and_reboot(rom: Rom, device: &DynClient) -> Result<Titles> {
    // the reboot has to reach the boot ROM: checked first
    let dfu = Dfu::new(device);
    if let Ok(node) = dfu.node.node().call().await
        && !matches!(node.profile, Profile::AB(_))
    {
        bail!(
            "the device updates through an updater of its own, not {}: use `ww dfu flash`",
            rom.name()
        );
    }
    let mut titles = Titles::new();
    // best effort: an older firmware without REWORKS gives numbers only
    if let Ok(list) = BoardInfoClient::new(device).read_REWORKS().read().await {
        for r in list.iter() {
            if let Some(title) = &r.title {
                titles.insert(r.id, title.to_string());
            }
        }
    }
    dfu.boot
        .reboot(BootTarget::Updater)
        .call()
        .await
        .map_err(|e| anyhow::anyhow!("rebooting the device into {}: {e:?}", rom.name()))?;
    eprintln!("{} rebooting into {}", style("✔").green(), rom.name());
    Ok(titles)
}

/// The check with the chip in its boot ROM: read the board's record through `io`, compare with the image (`flash`,
/// its flash contents), ask, then store or clear the allowance. Nothing of the image is written before this returns
/// `Ok`. `flash_base`, `flash_len`: where the image in flash is, to find its table (at most `flash_len` is read).
pub(crate) async fn in_rom(
    io: &mut impl RomFlash,
    flash_base: u32,
    flash_len: u32,
    flash: &[u8],
    args: &ReworkCheckArgs,
    titles: &Titles,
) -> Result<()> {
    let image = ImageReworks::from_flash(flash);
    // the image area ends where the store starts
    let scan_len = image
        .locator()
        .map_or(flash_len, |l| l.store_start.min(flash_len));
    let installed = scan_installed(io, flash_base, scan_len).await?;
    let Some(board) = read_board(io, &image, installed.as_ref(), args.board.as_deref()).await?
    else {
        if !args.allow_unknown_reworks.is_empty() {
            bail!(
                "--allow-unknown-reworks: neither image names a board record to store it in; nothing written"
            );
        }
        eprintln!(
            "{} neither image names a board record: no reworks to check",
            style("note").dim()
        );
        return Ok(());
    };
    let v = verdict(&image, &board);
    eprintln!(
        "{} {} {}, reworks {}, allowance {}",
        style("board").bold(),
        escape(&board.name),
        if board.revision.is_empty() {
            "?"
        } else {
            &board.revision
        },
        if board.applied.is_empty() {
            "none".to_string()
        } else {
            IdRanges(board.applied).to_string()
        },
        allowance_line(&board.allowance)
    );
    let allow = match plan(&v, args) {
        Step::Write { allow } => allow,
        Step::Refuse(why) => bail!("{why}; nothing written"),
        Step::Ask { question, allow } => {
            eprintln!("{} {v}", style("warning").yellow().bold());
            if let Verdict::Lacking { lacking, .. } = &v {
                for id in lacking.iter() {
                    match titles.get(&id) {
                        Some(t) => eprintln!("  rework {id}: {}", escape(t)),
                        None => eprintln!("  rework {id}"),
                    }
                }
            }
            if !ask(&question)? {
                bail!("nothing written");
            }
            allow
        }
    };
    match allow {
        Some((id, reworks)) => {
            set_allowance(io, &board, StoredAllowance::new(id, reworks)).await?;
            eprintln!(
                "{} rework(s) {} allowed for image {id}",
                style("✔").green(),
                IdRanges(reworks)
            );
        }
        None => {
            if set_allowance(io, &board, StoredAllowance::NONE).await? {
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

fn plan(v: &Verdict, args: &ReworkCheckArgs) -> Step {
    let allowed = args.allowed();
    let flag = |set: ReworkSet| format!("--allow-unknown-reworks {}", Ids(set));
    if let Verdict::Lacking {
        lacking,
        image: Some(id),
    } = v
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
            Verdict::Lacking { image: None, .. } => "the image has no build id to allow them for",
            _ => "the image lacks no rework",
        };
        return Step::Refuse(format!("{v}; --allow-unknown-reworks: {why}"));
    }
    match v {
        Verdict::Ok => Step::Write { allow: None },
        _ if args.yes => Step::Write { allow: None },
        Verdict::Lacking { .. } => Step::Ask {
            question: "write it anyway? it refuses to boot on this board".into(),
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
            board: None,
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
            plan(&Verdict::Ok, &args(&[], false)),
            Step::Write { allow: None }
        );
        assert!(matches!(
            plan(&Verdict::Ok, &args(&[7], false)),
            Step::Refuse(_)
        ));
    }

    #[test]
    fn lacking_is_asked_or_allowed_by_exact_numbers() {
        let v = lacking(&[7, 9], Some(ID));
        assert_eq!(
            plan(&v, &args(&[], false)),
            Step::Ask {
                question: "allow rework(s) 7, 9 for this image and write it?".into(),
                allow: Some((ID, set(&[7, 9])))
            }
        );
        // --yes doesn't allow reworks: only their numbers do
        assert!(matches!(plan(&v, &args(&[], true)), Step::Ask { .. }));
        assert_eq!(
            plan(&v, &args(&[9, 7], false)),
            Step::Write {
                allow: Some((ID, set(&[7, 9])))
            }
        );
        let Step::Refuse(why) = plan(&v, &args(&[7], false)) else {
            panic!("fewer numbers")
        };
        assert!(why.ends_with("--allow-unknown-reworks 7,9"), "{why}");
        assert!(matches!(
            plan(&v, &args(&[7, 9, 10], false)),
            Step::Refuse(_)
        ));
    }

    #[test]
    fn lacking_without_a_build_id_refuses_to_boot() {
        // an image without a build id: no allowance can be its own
        let v = lacking(&[7], None);
        assert!(matches!(
            plan(&v, &args(&[], false)),
            Step::Ask { allow: None, .. }
        ));
        assert_eq!(plan(&v, &args(&[], true)), Step::Write { allow: None });
        assert!(matches!(plan(&v, &args(&[7], false)), Step::Refuse(_)));
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
                plan(&v, &args(&[], false)),
                Step::Ask {
                    question: "write it anyway?".into(),
                    allow: None
                }
            );
            assert_eq!(plan(&v, &args(&[], true)), Step::Write { allow: None });
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
        let t = T::try_parse_from([
            "x",
            "--allow-unknown-reworks",
            "7,9",
            "--yes",
            "--board",
            "B200",
        ])
        .unwrap();
        assert_eq!(t.a.allow_unknown_reworks, [7, 9]);
        assert!(t.a.yes);
        assert_eq!(t.a.board.as_deref(), Some("B200"));
    }
}
