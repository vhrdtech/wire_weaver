use crate::device::Selection;
use anyhow::Result;
use clap::Args;
use console::measure_text_width;
use wire_weaver_client::DeviceInfo;
use ww_hub_api::{DeviceEntryOwned, DeviceStateOwned};

#[derive(Args)]
pub(crate) struct ListArgs {
    /// Also list USB devices not reporting a WireWeaver API id
    #[arg(short, long)]
    all: bool,

    /// One device per line, without table formatting
    #[arg(long)]
    plain: bool,

    /// Enumerate USB here even when a hub runs (which lists from its own cache and shows who holds what)
    #[arg(long)]
    direct: bool,
}

pub(crate) async fn list(args: ListArgs, selection: &Selection) -> Result<()> {
    // a running hub has the list already (devices it holds too), without touching USB; filters, `--all` and
    // `--direct` need the real thing
    if !args.all
        && !args.direct
        && selection.settings.is_empty()
        && let Some(hub) = crate::cmd::hub::connect_hub().await?
    {
        let entries = hub.registry().devices().call().await?;
        return print_hub_list(&entries, args.plain);
    }
    let devices = if args.all {
        wire_weaver_client::list_all_usb_devices().await?
    } else {
        wire_weaver_client::list_usb_devices().await?
    };
    let config = selection.client_config();
    let mut devices: Vec<_> = devices.into_iter().filter(|d| config.matches(d)).collect();
    devices.sort_by(|a, b| a.location.cmp(&b.location));

    if selection.uses_file() {
        eprintln!("Filtering by {selection}, use --no-config to ignore ww.toml");
    }
    if devices.is_empty() {
        eprintln!("No matching devices found");
        return Ok(());
    }
    if args.plain {
        for device in devices {
            println!("{device}");
        }
    } else {
        print_table(&devices);
    }
    Ok(())
}

/// The hub's registry in the layout of [print_table], with the device's state as a last column.
fn print_hub_list(entries: &[DeviceEntryOwned], plain: bool) -> Result<()> {
    if entries.is_empty() {
        eprintln!("No matching devices found");
        return Ok(());
    }
    if plain {
        for e in entries {
            let api = e.api.as_deref().unwrap_or("");
            println!(
                "{} \"{}\" serial={} api={api} label=\"{}\"",
                e.location.as_deref().unwrap_or(&e.key),
                e.product,
                e.serial,
                e.label.as_deref().unwrap_or("")
            );
        }
        return Ok(());
    }
    let rows = entries
        .iter()
        .map(|e| {
            vec![
                e.location.clone().unwrap_or_else(|| e.key.clone()),
                e.product.clone(),
                e.serial.clone(),
                e.api.clone().unwrap_or_default(),
                e.api_hash.clone().unwrap_or_default(),
                e.label.clone().unwrap_or_default(),
                state_text(&e.state),
            ]
        })
        .collect();
    crate::cmd::hub::print_table(
        &[
            "LOCATION", "PRODUCT", "SERIAL", "API", "HASH", "LABEL", "STATE",
        ],
        rows,
    );
    Ok(())
}

fn state_text(state: &DeviceStateOwned) -> String {
    match state {
        DeviceStateOwned::Absent => "absent".into(),
        DeviceStateOwned::Present => "free".into(),
        DeviceStateOwned::Open => "open (hub)".into(),
        DeviceStateOwned::HeldElsewhere { holder } => {
            format!("held by {} (pid {})", holder.program, holder.pid)
        }
        DeviceStateOwned::Faulted { why } => format!("faulted: {why}"),
        DeviceStateOwned::Unknown(_) => "unknown".into(),
    }
}

fn print_table(devices: &[DeviceInfo]) {
    let header = ["LOCATION", "PRODUCT", "SERIAL", "API", "HASH", "LABEL"];
    let rows: Vec<[String; 6]> = devices
        .iter()
        .map(|d| {
            let (api, hash) = match &d.api {
                Some(api) => (
                    format!("{}@{}", api.gid, api.version),
                    if api.signature.hash.is_empty() {
                        String::new()
                    } else {
                        api.signature.to_string()
                    },
                ),
                None => (String::new(), String::new()),
            };
            [
                d.location.clone(),
                d.product.clone(),
                d.serials.join(","),
                api,
                hash,
                d.user_label.clone(),
            ]
        })
        .collect();
    let mut widths = header.map(measure_text_width);
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(measure_text_width(cell));
        }
    }
    let print_row = |cells: &[&str]| {
        let line = cells
            .iter()
            .zip(widths)
            .map(|(cell, w)| format!("{cell}{}", " ".repeat(w - measure_text_width(cell))))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{}", line.trim_end());
    };
    print_row(&header);
    for row in &rows {
        print_row(&row.each_ref().map(String::as_str));
    }
}
