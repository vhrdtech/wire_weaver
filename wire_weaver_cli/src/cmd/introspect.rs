use crate::api_tree;
use crate::registry::{BoxFuture, CommandDef, Connection, DeviceUse, GlobalFlags, Ran, Run};
use anyhow::{Result, anyhow};
use clap::{Args, FromArgMatches};
use console::style;
use shrink_wrap::SerializeShrinkWrapOwned;
use std::collections::HashMap;
use tracing::level_filters::LevelFilter;
use wire_weaver_client::ww_self::{ApiItemKindOwned, Multiplicity, PropertyAccess};
use wire_weaver_client::{DynClient, DynResource};

#[derive(Args)]
pub(crate) struct IntrospectArgs {
    /// Print raw introspection data (Rust debug format) instead of the resource tree
    #[arg(long, conflicts_with = "raw_as_sent")]
    raw: bool,

    /// Print raw introspection data as the device sent it, before traits and types it left out
    /// (known from snapshots) were put back
    #[arg(long)]
    raw_as_sent: bool,

    /// Also print all types the API refers to
    #[arg(short('t'), long)]
    types: bool,

    /// Do not print documentation for each resource
    #[arg(short('d'), long)]
    skip_docs: bool,

    /// Do not read the values of `const` items from the device
    #[arg(long)]
    no_values: bool,
}

pub(crate) async fn introspect(args: IntrospectArgs, device: &DynClient) -> Result<()> {
    if args.raw || args.raw_as_sent {
        let introspect = device
            .device_introspect()
            .ok_or_else(|| anyhow!("device did not provide introspection data"))?;
        if args.raw {
            println!("{:#?}", introspect.api_bundle);
        } else {
            println!("{:#?}", introspect.sent_api_bundle);
        }
        return Ok(());
    }
    let view = IntrospectView {
        types: args.types,
        skip_docs: args.skip_docs,
        no_values: args.no_values,
    };
    print!("{}", render(device, view).await?);
    Ok(())
}

/// What `ww introspect` shows of a device, the flags it takes besides `--raw`.
#[derive(Debug, Clone, Copy, Default)]
pub struct IntrospectView {
    /// Also the types the API refers to (`-t`).
    pub types: bool,
    /// No doc comments (`-d`).
    pub skip_docs: bool,
    /// No values of `const` items, so no reads (`--no-values`).
    pub no_values: bool,
}

/// The text `ww introspect` prints for `device`: the resource tree, then its size, crates and API hash. Colours
/// follow [console::colors_enabled]. For a program or a test that shows the same text, such as a book's transcript.
pub async fn render(device: &DynClient, view: IntrospectView) -> Result<String> {
    use std::fmt::Write;
    let introspect = device
        .device_introspect()
        .ok_or_else(|| anyhow!("device did not provide introspection data"))?;
    let bundle = &introspect.api_bundle;
    let values = if view.no_values {
        HashMap::new()
    } else {
        const_values(device, DynResource::root(bundle.clone())).await
    };
    let mut out = api_tree::render_with_values(bundle, view.skip_docs, &values);
    if view.types {
        out.push('\n');
        out.push_str(&api_tree::render_types(bundle, view.skip_docs));
    }
    out.push('\n');
    let full_size = bundle.to_ww_bytes_owned()?.len();
    writeln!(
        out,
        "{}, {full_size} bytes full, {} bytes sent",
        api_tree::summary(bundle),
        introspect.sent_size
    )?;
    let crates = bundle
        .ext_crates
        .iter()
        .map(|v| format!("{v:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(out, "{} {crates}", style("crates:").dim())?;
    writeln!(
        out,
        "{} {}",
        style("api hash:").dim(),
        introspect.api_hash.no_docs
    )?;
    Ok(out)
}

/// Values of the `const` items outside of resource arrays, by resource ids from the root, a read error in place of
/// the value. One read each, cached by the client for the rest of the connection (API-36).
async fn const_values(device: &DynClient, root: DynResource) -> HashMap<Vec<u32>, String> {
    let mut values = HashMap::new();
    let mut levels = vec![root];
    while let Some(level) = levels.pop() {
        for item in level.children() {
            if !matches!(item.multiplicity, Multiplicity::Flat) {
                continue;
            }
            let is_const = matches!(
                item.kind,
                ApiItemKindOwned::Property {
                    access: PropertyAccess::Const,
                    ..
                }
            );
            let is_trait = matches!(item.kind, ApiItemKindOwned::Trait { .. });
            if !is_const && !is_trait {
                continue;
            }
            let Ok(child) = level.child(&item.ident) else {
                continue;
            };
            if child.is_omitted() {
                continue;
            }
            if is_trait {
                levels.push(child);
                continue;
            }
            let ApiItemKindOwned::Property { ty, .. } = &item.kind else {
                continue;
            };
            let path = child.index_chain().iter().map(|id| id.get().0).collect();
            let value = match child.read(device.cmd()).await {
                Ok(value) => crate::value_fmt::fmt_value(&value, Some(ty), level.bundle()),
                Err(e) => style(format!("<{e}>")).red().to_string(),
            };
            values.insert(path, value);
        }
    }
    values
}

/// `ww introspect`.
fn command() -> clap::Command {
    IntrospectArgs::augment_args(clap::Command::new("introspect"))
        .about("Print the resource tree of the selected device's API, using its introspection data")
}

fn run(run: Run<'_>) -> BoxFuture<'_, Result<Ran>> {
    Box::pin(async move {
        let args = IntrospectArgs::from_arg_matches(&run.matches)?;
        introspect(args, run.device()).await?;
        Ok(Ran::DONE)
    })
}

inventory::submit! {
    CommandDef::new("introspect", 40, command, run)
        .globals(&[("", GlobalFlags::Device)])
        .connection(|_| Ok(Connection::Device(DeviceUse::FULL)))
        .log_level(|_| LevelFilter::WARN)
}
