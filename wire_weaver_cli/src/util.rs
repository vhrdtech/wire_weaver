use anyhow::{Result, anyhow};
use proc_macro2::{Ident, Span};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use wire_weaver_core::ast::api::ApiLevel;
use wire_weaver_core::ast::trait_macro_args::ImplTraitLocation;
use wire_weaver_core::transform::load::load_api_level_recursive;
use wire_weaver_usb_host::usb_worker;
use wire_weaver_usb_host::wire_weaver_client_common::ww_version::{FullVersionOwned, VersionOwned};
use wire_weaver_usb_host::wire_weaver_client_common::{CommandSender, DeviceFilter, OnError};

pub async fn connect_usb_dyn_api(filter: DeviceFilter) -> Result<CommandSender> {
    let (transport_cmd_tx, transport_cmd_rx) = mpsc::unbounded_channel();
    let (dispatcher_msg_tx, dispatcher_msg_rx) = mpsc::unbounded_channel();
    let mut cmd_tx = CommandSender::new(transport_cmd_tx, dispatcher_msg_rx);
    tokio::spawn(async move {
        usb_worker(transport_cmd_rx, dispatcher_msg_tx).await;
    });
    cmd_tx
        .connect(
            filter,
            FullVersionOwned::new("".into(), VersionOwned::new(0, 1, 0)),
            OnError::ExitImmediately,
        )
        .await?;
    Ok(cmd_tx)
}

pub(crate) fn load_level(path: PathBuf, name: Option<String>) -> anyhow::Result<ApiLevel> {
    // do some gymnastics to point base_dir at crate root (where Cargo.toml is)
    let mut base_dir = path.clone();
    base_dir.pop(); // pop ww.rs
    base_dir.pop(); // pop src

    let parent = path
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap(); // likely src folder
    let file_name = path.file_name().unwrap().to_str().unwrap(); // likely ww.rs or src.rs

    let mut cache = HashMap::new();
    let level = load_api_level_recursive(
        &ImplTraitLocation::AnotherFile {
            path: format!("{parent}/{file_name}"),
            part_of_crate: Ident::new("crate", Span::call_site()),
        },
        name.map(|n| Ident::new(n.as_str(), Span::call_site())),
        None,
        base_dir.as_path(),
        &mut cache,
    )
    .map_err(|e| anyhow!(e))?;
    Ok(level)
}
