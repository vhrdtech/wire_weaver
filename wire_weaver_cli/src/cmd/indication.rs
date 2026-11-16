//! `ww sync --mode`: the indication mode (`ww_indication`) written on every synced device, so their animations
//! line up on UTC. The standard library's part of `ww sync`, a [`SyncExtension`].

use crate::cmd::sync::SyncExtension;
use crate::registry::BoxFuture;
use clap::{ArgMatches, Args, FromArgMatches, ValueEnum};
use wire_weaver_client::{Commander, DynClient};

#[derive(Args)]
struct IndicationArgs {
    /// Also write this indication mode (ww_indication) on every device; their animations line up on UTC
    #[arg(long, value_enum)]
    mode: Option<IndicationMode>,
}

#[derive(Copy, Clone, ValueEnum)]
enum IndicationMode {
    /// Every indicator off, alerts included
    AllOff,
    /// Only indicators that must stay visible (faults, power)
    OnlyEssential,
    /// Every indicator as designed
    Normal,
}

impl From<IndicationMode> for ww_indication::Mode {
    fn from(m: IndicationMode) -> Self {
        match m {
            IndicationMode::AllOff => ww_indication::Mode::AllOff,
            IndicationMode::OnlyEssential => ww_indication::Mode::OnlyEssential,
            IndicationMode::Normal => ww_indication::Mode::Normal,
        }
    }
}

/// Client of `ww_indication::Indication` wherever the device implements it.
struct IndicationClient {
    cmd: Commander,
}

mod indication_client {
    use super::*;
    wire_weaver::ww_codegen!(
        ww_indication::Indication for IndicationClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
    );
}

/// Writes `--mode` on the device; the note for its row when that failed.
fn on_device<'a>(matches: &'a ArgMatches, device: &'a DynClient) -> BoxFuture<'a, Option<String>> {
    Box::pin(async move {
        let mode = IndicationArgs::from_arg_matches(matches).ok()?.mode?;
        let indication = IndicationClient {
            cmd: device.cmd().clone(),
        };
        match indication.write_mode(mode.into()).write().await {
            Ok(()) => None,
            Err(e) => Some(format!("mode not set: {e}")),
        }
    })
}

inventory::submit! {
    SyncExtension {
        args: IndicationArgs::augment_args,
        on_device,
    }
}
