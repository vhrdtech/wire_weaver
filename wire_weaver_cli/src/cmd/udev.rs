//! `ww udev`: meant to print a udev rule giving the user access to WireWeaver USB devices (Linux only). Not
//! implemented yet (TOOL-37): the command says so, the rule is written by hand (`docs/transport/usb.md`).

use crate::registry::{BoxFuture, CommandDef, Ran, Run};
use anyhow::{Result, bail};

/// `ww udev`.
fn command() -> clap::Command {
    clap::Command::new("udev")
        .about("Print udev rule to the stdout, run 'ww udev --help' for more information")
        .long_about(
            "Print udev rule to the stdout, run 'ww udev --help' for more information\n\n\
             Create udev rule:\n\
             ww udev | sudo tee /etc/udev/rules.d/70-ww_device.rules\n\n\
             Reload rules and trigger:\n\
             sudo udevadm control --reload-rules\n\
             sudo udevadm trigger",
        )
}

fn run(_run: Run<'_>) -> BoxFuture<'_, Result<Ran>> {
    Box::pin(async move {
        bail!(
            "`ww udev` is not implemented yet (TOOL-37): write the rule by hand, see the USB page of the docs"
        )
    })
}

inventory::submit! {
    CommandDef::new("udev", 220, command, run)
}
