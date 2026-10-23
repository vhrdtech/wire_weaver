use crate::device::Selection;
use crate::untrusted::{Untrusted, escape};
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

    /// List debug probes for --rtt (also when --rtt is given), with who holds each
    #[arg(long)]
    probes: bool,
}

pub(crate) async fn list(args: ListArgs, selection: &Selection) -> Result<()> {
    if args.probes || selection.rtt().is_some() {
        return list_probes(args.plain);
    }
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
            // DeviceInfo's own Display interleaves our literal text (quotes, `serial=`, ..) with the device's own
            // manufacturer/product/serial/label strings; escaping the whole rendered line is safe here (and
            // simpler than picking the fields apart again) because this line never carries our own colour codes
            println!("{}", Untrusted(&device.to_string()));
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
            println!("{}", hub_list_plain_line(e));
        }
        return Ok(());
    }
    let rows = entries.iter().map(hub_list_row).collect();
    crate::cmd::hub::print_table(
        &[
            "LOCATION", "PRODUCT", "SERIAL", "API", "HASH", "LABEL", "STATE",
        ],
        rows,
    );
    Ok(())
}

/// Debug probes, with the WireWeaver program holding each (from the lock files: `rtt:<probe serial>:<chip>`).
fn list_probes(plain: bool) -> Result<()> {
    let probes = wire_weaver_client::list_probes();
    if probes.is_empty() {
        eprintln!("No debug probes found");
        return Ok(());
    }
    let held = wire_weaver_client::holders::list();
    let rows: Vec<Vec<String>> = probes
        .iter()
        .map(|p| {
            let serial = p.serial.as_deref().unwrap_or_default();
            let prefix = format!("rtt:{serial}:");
            let state = held
                .iter()
                .find(|(key, _)| key.starts_with(&prefix))
                .map(|(key, h)| {
                    format!(
                        "held by {} (pid {}) for {}",
                        escape(&h.program),
                        h.pid,
                        escape(&key[prefix.len()..])
                    )
                })
                .unwrap_or_else(|| "free".into());
            vec![
                escape(&p.name).into_owned(),
                format!("{:04x}:{:04x}", p.vid, p.pid),
                escape(serial).into_owned(),
                state,
            ]
        })
        .collect();
    if plain {
        for r in rows {
            println!("{} {} serial={} {}", r[0], r[1], r[2], r[3]);
        }
    } else {
        crate::cmd::hub::print_table(&["PROBE", "VID:PID", "SERIAL", "STATE"], rows);
    }
    Ok(())
}

/// One `ww list --plain` line of the hub's registry, with every device/hub-supplied field escaped (TOOL-19):
/// `location`/`key` carry the raw USB serial too, `product` and `label` are free text.
fn hub_list_plain_line(e: &DeviceEntryOwned) -> String {
    format!(
        "{} \"{}\" serial={} api={} label=\"{}\"",
        escape(e.location.as_deref().unwrap_or(&e.key)),
        escape(&e.product),
        escape(&e.serial),
        escape(e.api.as_deref().unwrap_or("")),
        escape(e.label.as_deref().unwrap_or(""))
    )
}

/// One table row of the hub's registry, same escaping as [hub_list_plain_line].
fn hub_list_row(e: &DeviceEntryOwned) -> Vec<String> {
    vec![
        escape(e.location.as_deref().unwrap_or(&e.key)).into_owned(),
        escape(&e.product).into_owned(),
        escape(&e.serial).into_owned(),
        e.api
            .as_deref()
            .map(|a| escape(a).into_owned())
            .unwrap_or_default(),
        e.api_hash.clone().unwrap_or_default(),
        e.label
            .as_deref()
            .map(|l| escape(l).into_owned())
            .unwrap_or_default(),
        state_text(&e.state),
    ]
}

/// `lent by ww (pid 7): probe-rs reset`: who lent the device out of the hub (SRV-32) and what for.
pub(crate) fn lent_text(claim: &ww_hub_api::ClaimEntryOwned) -> String {
    let mut text = format!(
        "lent by {} (pid {})",
        escape(&claim.holder.program),
        claim.holder.pid
    );
    if !claim.purpose.is_empty() {
        text.push_str(&format!(": {}", escape(&claim.purpose)));
    }
    text
}

fn state_text(state: &DeviceStateOwned) -> String {
    match state {
        DeviceStateOwned::Absent => "absent".into(),
        DeviceStateOwned::Present => "free".into(),
        DeviceStateOwned::Open => "open (hub)".into(),
        DeviceStateOwned::HeldElsewhere { holder } => {
            format!("held by {} (pid {})", escape(&holder.program), holder.pid)
        }
        DeviceStateOwned::Faulted { why } => format!("faulted: {}", escape(why)),
        DeviceStateOwned::Lent { claim } => lent_text(claim),
        DeviceStateOwned::Unknown(_) => "unknown".into(),
    }
}

fn print_table(devices: &[DeviceInfo]) {
    let header = ["LOCATION", "PRODUCT", "SERIAL", "API", "HASH", "LABEL"];
    let rows: Vec<[String; 6]> = devices.iter().map(device_row).collect();
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

/// One table row of a directly-enumerated USB device, `location`/`product`/`serials`/`label` escaped (TOOL-19).
fn device_row(d: &DeviceInfo) -> [String; 6] {
    let (api, hash) = match &d.api {
        Some(api) => (
            format!("{}@{}", escape(&api.gid), api.version),
            if api.signature.hash.is_empty() {
                String::new()
            } else {
                api.signature.to_string()
            },
        ),
        None => (String::new(), String::new()),
    };
    [
        escape(&d.location).into_owned(),
        escape(&d.product).into_owned(),
        escape(&d.serials.join(",")).into_owned(),
        api,
        hash,
        escape(&d.user_label).into_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device whose product string carries a terminal title-change escape (TOOL-19): the table row comes out
    /// with the ESC and BEL bytes escaped, keeping the rest of the text readable.
    #[test]
    fn device_row_escapes_a_hostile_product_string() {
        let d = DeviceInfo {
            location: "usb 3-1.2 c0de:cafe".into(),
            manufacturer: "Acme".into(),
            product: "\x1b]0;evil\x07Widget".into(),
            serials: vec!["0B135A01".into()],
            user_label: String::new(),
            api: None,
            usb: None,
        };
        let row = device_row(&d);
        assert_eq!(row[1], "\\u{1b}]0;evil\\u{7}Widget");
        assert!(!row[1].contains('\x1b'));
    }

    /// The hub registry's plain line escapes the same way, product and label both (TOOL-19).
    #[test]
    fn hub_list_plain_line_escapes_product_and_label() {
        let e = DeviceEntryOwned {
            key: "usb:c0de:cafe:0B135A01".into(),
            location: Some("usb 3-1.2 c0de:cafe".into()),
            product: "\x1b]0;evil\x07Widget".into(),
            serial: "0B135A01".into(),
            api: None,
            api_hash: None,
            label: Some("left\u{202e}bench".into()),
            state: DeviceStateOwned::Present,
            tunnels: 0,
        };
        let line = hub_list_plain_line(&e);
        assert!(line.contains("\\u{1b}]0;evil\\u{7}Widget"));
        assert!(line.contains("left\\u{202e}bench"));
        assert!(!line.contains('\x1b'));
        assert!(!line.contains('\u{202e}'));
    }
}
