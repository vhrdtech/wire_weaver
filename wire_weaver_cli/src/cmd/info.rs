//! `ww info`: what a device is. The link and API facts come from the connection itself; what the device says
//! about itself (firmware, board, ids, label, last reset: the standard library's device contract) is an
//! [`InfoExtension`] of the crate that knows those traits.

use crate::device::Selection;
use crate::registry::{BoxFuture, CommandDef, Connection, DeviceUse, GlobalFlags, Ran, Run};
use anyhow::{Context, Result};
use clap::{Args, FromArgMatches, ValueEnum};
use serde::Serialize;
use std::collections::BTreeMap;
use wire_weaver_client::DynClient;
use wire_weaver_client::ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, Effects, TRAIT_DEPTH_MAX,
};

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
    /// Also list what the API's items do (API-58): every item that changes something, by effect. Reads the
    /// device's introspection data.
    #[arg(long)]
    effects: bool,
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

/// The items of an API by what they do (API-58): the paths from the root (`periph[].gain`) of every item that is no
/// plain read, under each of its effects; `none declared` for an item without any (a model made before effects).
#[derive(Debug, Serialize)]
struct EffectsFacts {
    effects: BTreeMap<String, Vec<String>>,
}

impl EffectsFacts {
    fn of(bundle: &ApiBundleOwned) -> Self {
        let mut effects = BTreeMap::new();
        collect_effects(bundle, &bundle.root, "", 0, &mut effects);
        EffectsFacts { effects }
    }

    fn section(&self) -> Result<Section> {
        let mut text = String::new();
        for (i, (effect, paths)) in self.effects.iter().enumerate() {
            let label = if i == 0 { "Effects" } else { "" };
            text += &format!("{label:<10} {effect}: {}\n", paths.join(", "));
        }
        if text.is_empty() {
            text = "Effects    every item only reads\n".to_owned();
        }
        Section::new(text, self)
    }
}

/// Adds the items of `level` (and of the traits under it) to `out`, with `prefix` the path to `level`.
fn collect_effects(
    bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    prefix: &str,
    depth: usize,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    for item in &level.items {
        let array = if item.is_array() { "[]" } else { "" };
        let path = format!("{prefix}{}{array}", item.ident);
        if let ApiItemKindOwned::Trait { .. } = item.kind {
            // a trait left out of the data (skipped, not restored) has nothing to list
            if depth < TRAIT_DEPTH_MAX
                && let Ok(sub) = item.get_as_level(bundle)
            {
                collect_effects(bundle, sub, &format!("{path}."), depth + 1, out);
            }
            continue;
        }
        let effects = item.effects();
        if effects.is_read() {
            continue;
        }
        let names: Vec<String> = if effects == Effects::NONE {
            vec!["none declared".to_owned()]
        } else {
            effects.to_string().split(", ").map(str::to_owned).collect()
        };
        for name in names {
            out.entry(name).or_default().push(path.clone());
        }
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
             traits anywhere in its API; what it doesn't implement is said so. `--effects` adds what the API's items \
             do, from the device's introspection data.",
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
        if args.effects {
            let introspect = device
                .device_introspect()
                .context("the device sent no introspection data, which --effects reads")?;
            sections.push(EffectsFacts::of(&introspect.api_bundle).section()?);
        }
        print(args.format, sections)?;
        Ok(Ran::DONE)
    })
}

inventory::submit! {
    CommandDef::new("info", 150, command, run)
        .globals(&[("", GlobalFlags::Device)])
        .connection(|m| {
            // the effects are in the device's introspection data, the rest needs only its global traits
            let device_use = match m.get_flag("effects") {
                true => DeviceUse::FULL,
                false => DeviceUse::TRAITS_ONLY,
            };
            Ok(Connection::Device(device_use.on_demand()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn effects_list_what_changes_something() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ww_hub_api");
        let bundle = wire_weaver_core::load(&path, Some("Hub".into()), false).unwrap();
        let facts = EffectsFacts::of(&bundle);
        assert_eq!(
            facts.effects["reset"],
            ["handover", "shutdown", "claims.lend"]
        );
        assert_eq!(facts.effects["persist"], ["captures.save"]);
        assert!(facts.effects["edit"].contains(&"sync.linger".to_owned()));
        // reads are left out
        assert!(!facts.effects.values().flatten().any(|p| p == "info"));
        assert!(!facts.effects.contains_key("none declared"));
        let text = facts.section().unwrap().text;
        assert!(text.starts_with("Effects    "), "{text}");
    }
}
