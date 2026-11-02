//! `ww board`: tools around `ww_board_info::BoardInfo` (TOOL-28). `rework set|clear` records a rework done on the
//! bench, on any device serving `BoardInfo::set_reworks`, through a global trait attachment (API-20).

use anyhow::{Result, anyhow, bail};
use clap::{Args, Subcommand};
use console::style;
use wire_weaver::prelude::MaxLen;
use wire_weaver_client::ww_client_server::ErrorKindOwned;
use wire_weaver_client::{CallError, DynClient, Error};
use ww_board_info::table::{IdRanges, ReworkSet};
use ww_board_info::{ReworkChange, ReworkErrorOwned, ReworkId, ReworkState};
use ww_report::clients::BoardInfoClient;

#[derive(Subcommand)]
pub(crate) enum BoardCommand {
    /// Record reworks done on the bench (STD-98)
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

pub(crate) async fn board(cmd: BoardCommand, device: &DynClient) -> Result<()> {
    match cmd {
        BoardCommand::Rework(ReworkCommand::Set(args)) => set_reworks(args, true, device).await,
        BoardCommand::Rework(ReworkCommand::Clear(args)) => set_reworks(args, false, device).await,
    }
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
}
