use anyhow::{Context, Result};
use clap::Args;
use console::measure_text_width;
use semver::VersionReq;
use wire_weaver_client::DeviceInfo;

#[derive(Args)]
pub(crate) struct ListArgs {
    /// Only devices implementing this API, optionally with a version requirement: name[@req], e.g. blinky_api@^0.1
    #[arg(long)]
    api: Option<String>,

    /// Only devices with this user label (case-insensitive)
    #[arg(short, long)]
    label: Option<String>,

    /// Only devices with product description containing this substring (case-insensitive)
    #[arg(short, long)]
    product: Option<String>,

    /// Also list USB devices not reporting a WireWeaver API id
    #[arg(short, long)]
    all: bool,

    /// One device per line, without table formatting
    #[arg(long)]
    plain: bool,
}

pub(crate) async fn list(args: ListArgs, serial: Option<String>) -> Result<()> {
    let api = args.api.as_deref().map(parse_api).transpose()?;
    let devices = if args.all {
        wire_weaver_client::list_all_usb_devices().await?
    } else {
        wire_weaver_client::list_usb_devices().await?
    };
    let mut devices: Vec<_> = devices
        .into_iter()
        .filter(|d| {
            let api_match = api.as_ref().is_none_or(|(gid, req)| {
                d.api
                    .as_ref()
                    .is_some_and(|a| &a.gid == gid && req.matches(&a.version))
            });
            let serial_match = serial
                .as_ref()
                .is_none_or(|s| d.serials.iter().any(|ds| contains(ds, s)));
            let label_match = args
                .label
                .as_ref()
                .is_none_or(|l| d.user_label.to_lowercase() == l.to_lowercase());
            let product_match = args
                .product
                .as_ref()
                .is_none_or(|p| contains(&d.product, p));
            api_match && serial_match && label_match && product_match
        })
        .collect();
    devices.sort_by(|a, b| a.location.cmp(&b.location));

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

fn parse_api(s: &str) -> Result<(String, VersionReq)> {
    match s.split_once('@') {
        Some((gid, req)) => Ok((
            gid.to_string(),
            VersionReq::parse(req).context(format!("parsing version requirement '{req}'"))?,
        )),
        None => Ok((s.to_string(), VersionReq::STAR)),
    }
}

fn contains(s: &str, substring: &str) -> bool {
    s.to_lowercase().contains(&substring.to_lowercase())
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
