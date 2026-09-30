use crate::device::Selection;
use anyhow::Result;
use clap::Args;
use console::measure_text_width;
use wire_weaver_client::DeviceInfo;

#[derive(Args)]
pub(crate) struct ListArgs {
    /// Also list USB devices not reporting a WireWeaver API id
    #[arg(short, long)]
    all: bool,

    /// One device per line, without table formatting
    #[arg(long)]
    plain: bool,
}

pub(crate) async fn list(args: ListArgs, selection: &Selection) -> Result<()> {
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
