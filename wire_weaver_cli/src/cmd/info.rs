//! `ww info`: what a device is. The link and API facts come from the connection itself; what the device says
//! about itself (firmware, board, ids, label, last reset: the standard library's device contract) is an
//! [`InfoExtension`] of the crate that knows those traits.

use crate::device::Selection;
use crate::registry::{BoxFuture, CommandDef, Connection, DeviceUse, GlobalFlags, Ran, Run};
use anyhow::Result;
use clap::{Args, FromArgMatches, ValueEnum};
use serde::Serialize;
use wire_weaver_client::DynClient;

/// `--format` of the commands that print for people or for scripts.
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug, ValueEnum)]
pub enum Format {
    /// For people
    #[default]
    Text,
    /// For scripts
    Json,
}

#[derive(Args)]
pub(crate) struct InfoArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t)]
    format: Format,
}

/// One part of `ww info`, for people and for scripts.
#[derive(Debug, Default)]
pub struct Section {
    /// Lines for `--format text`, each ending with a newline.
    pub text: String,
    /// Keys for `--format json`, merged into the one object `ww info` prints.
    pub json: serde_json::Map<String, serde_json::Value>,
}

impl Section {
    /// A section from a value that serializes as an object (`#[derive(Serialize)]` struct) and its text.
    pub fn new<T: Serialize>(text: String, value: &T) -> Result<Self> {
        let json = match serde_json::to_value(value)? {
            serde_json::Value::Object(map) => map,
            other => anyhow::bail!("an info section serializes as an object, not {other}"),
        };
        Ok(Section { text, json })
    }
}

/// What a crate linking this one adds to `ww info`.
pub struct InfoExtension {
    /// A device sitting in its boot ROM instead of running (`ww info` on a board in BOOTSEL): what to show of it,
    /// found by the device selection; `None` when there is no such device, so `ww info` connects as usual.
    pub rom: for<'a> fn(&'a Selection) -> BoxFuture<'a, Result<Option<Section>>>,
    /// What the connected device says about itself.
    pub device: for<'a> fn(&'a DynClient) -> BoxFuture<'a, Result<Section>>,
}

inventory::collect!(InfoExtension);

/// The link and API facts of the connection: the device's API root and wire version, and the hello generations
/// this connection agreed on.
#[derive(Debug, Serialize)]
struct ApiFacts {
    /// Crate name and version of the device's own API root
    api: String,
    /// The device's wire version (EVO-24), apart from the crate version above: `None` from firmware that declares
    /// neither it nor an introspection-only connection resolves.
    api_wire: Option<String>,
    /// Hello generations (EVO-23) this connection agreed on.
    generations: String,
}

impl ApiFacts {
    fn of(device: &DynClient) -> Self {
        let info = device.device_api_info();
        ApiFacts {
            api: format!("{:?}", info.user_api_version),
            api_wire: info.user_api_wire.map(|w| w.to_string()),
            generations: info.generations.to_string(),
        }
    }

    fn section(&self) -> Result<Section> {
        let mut text = match &self.api_wire {
            Some(wire) => format!("API        {} (wire {wire})\n", self.api),
            None => format!("API        {}\n", self.api),
        };
        text += &format!("Hello      {}\n", self.generations);
        Section::new(text, self)
    }
}

fn print(format: Format, sections: Vec<Section>) -> Result<()> {
    match format {
        Format::Text => {
            for s in sections {
                print!("{}", s.text);
            }
        }
        Format::Json => {
            let mut all = serde_json::Map::new();
            for s in sections {
                all.extend(s.json);
            }
            println!("{}", serde_json::to_string_pretty(&all)?);
        }
    }
    Ok(())
}

/// `ww info`.
fn command() -> clap::Command {
    InfoArgs::augment_args(clap::Command::new("info"))
        .about("What the device is: firmware, board, ids, label and last reset, whatever its API and version")
        .long_about(
            "What the device is: firmware, board, ids, label and last reset, whatever its API and version\n\n\
             Works with any device implementing the `ww_firmware_info`, `ww_board_info`, `ww_uid` and `ww_user_info` \
             traits anywhere in its API; what it doesn't implement is said so.",
        )
}

fn run(mut run: Run<'_>) -> BoxFuture<'_, Result<Ran>> {
    Box::pin(async move {
        let args = InfoArgs::from_arg_matches(&run.matches)?;
        // a device already sitting in its boot ROM (TOOL-30): what an extension reads through the ROM, no connection
        for ext in inventory::iter::<InfoExtension>() {
            if let Some(section) = (ext.rom)(run.selection()).await? {
                print(args.format, vec![section])?;
                return Ok(Ran::DONE);
            }
        }
        let device = run.connect().await?;
        let mut sections = vec![ApiFacts::of(device).section()?];
        for ext in inventory::iter::<InfoExtension>() {
            sections.push((ext.device)(device).await?);
        }
        print(args.format, sections)?;
        Ok(Ran::DONE)
    })
}

inventory::submit! {
    CommandDef::new("info", 150, command, run)
        .globals(&[("", GlobalFlags::DeviceAndTap)])
        .connection(|_| Ok(Connection::Device(DeviceUse::TRAITS_ONLY.on_demand())))
}
