use crate::api_tree;
use anyhow::{Result, anyhow};
use clap::Args;
use console::style;
use shrink_wrap::SerializeShrinkWrapOwned;
use std::collections::HashMap;
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
    let introspect = device
        .device_introspect()
        .ok_or_else(|| anyhow!("device did not provide introspection data"))?;
    let bundle = &introspect.api_bundle;
    if args.raw {
        println!("{bundle:#?}");
        return Ok(());
    }
    if args.raw_as_sent {
        println!("{:#?}", introspect.sent_api_bundle);
        return Ok(());
    }

    let values = if args.no_values {
        HashMap::new()
    } else {
        const_values(device, DynResource::root(bundle.clone())).await
    };
    print!(
        "{}",
        api_tree::render_with_values(bundle, args.skip_docs, &values)
    );
    if args.types {
        println!();
        print!("{}", api_tree::render_types(bundle, args.skip_docs));
    }
    println!();
    let full_size = bundle.to_ww_bytes_owned()?.len();
    println!(
        "{}, {full_size} bytes full, {} bytes sent",
        api_tree::summary(bundle),
        introspect.sent_size
    );
    let crates = bundle
        .ext_crates
        .iter()
        .map(|v| format!("{v:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!("{} {crates}", style("crates:").dim());
    println!(
        "{} {}",
        style("api hash:").dim(),
        introspect.api_hash.no_docs
    );
    Ok(())
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
            let path = child.index_chain().iter().map(|id| id.0).collect();
            let value = match child.read(device.cmd()).await {
                Ok(value) => crate::value_fmt::fmt_value(&value),
                Err(e) => style(format!("<{e}>")).red().to_string(),
            };
            values.insert(path, value);
        }
    }
    values
}
