use crate::api_tree;
use anyhow::{Result, anyhow};
use clap::Args;
use console::style;
use shrink_wrap::SerializeShrinkWrapOwned;
use wire_weaver_client::DynClient;

#[derive(Args)]
pub(crate) struct IntrospectArgs {
    /// Print raw introspection data (Rust debug format) instead of the resource tree
    #[arg(long)]
    raw: bool,

    /// Do not print documentation for each resource
    #[arg(short('d'), long)]
    skip_docs: bool,
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

    print!("{}", api_tree::render(bundle, args.skip_docs));
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
