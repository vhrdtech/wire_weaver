//! `ww sync`: several devices kept on the host's UTC at once (TOOL-16), with `ww_clock_sync`.

use crate::complete;
use crate::device::Selection;
use crate::util::StopSignal;
use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use clap_complete::ArgValueCandidates;
use console::{Term, measure_text_width, style};
use std::time::Duration;
use wire_weaver_client::{Commander, DeviceInfo, DynClient};
use ww_clock_sync::{ClockSync, Options, Status};
use ww_timebase::{SyncSource, SyncState};

#[derive(Args)]
pub(crate) struct SyncArgs {
    /// Devices to sync, each a serial number or user label or a part of one (case-insensitive) that matches
    /// exactly one connected device. None: every device the device selection matches
    #[arg(value_name = "DEVICE", add = ArgValueCandidates::new(complete::serials_and_labels))]
    devices: Vec<String>,

    /// Also write this indication mode (ww_indication) on every device; their animations line up on UTC
    #[arg(long, value_enum)]
    mode: Option<IndicationMode>,

    /// Stop after this many seconds (default: until Ctrl+C); the devices go on in holdover
    #[arg(long, value_name = "S")]
    duration: Option<u64>,

    /// Time between two readings of each device's clock
    #[arg(long, value_name = "MS", default_value_t = 200)]
    poll_ms: u64,

    /// Time between two steer points sent to each device
    #[arg(long, value_name = "MS", default_value_t = 1000)]
    steer_ms: u64,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum IndicationMode {
    /// Every indicator off, alerts included
    AllOff,
    /// Only indicators that must stay visible (faults, power)
    OnlyEssential,
    /// Every indicator as designed
    Normal,
}

impl From<IndicationMode> for ww_indication::Mode {
    fn from(m: IndicationMode) -> Self {
        match m {
            IndicationMode::AllOff => ww_indication::Mode::AllOff,
            IndicationMode::OnlyEssential => ww_indication::Mode::OnlyEssential,
            IndicationMode::Normal => ww_indication::Mode::Normal,
        }
    }
}

/// Client of `ww_indication::Indication` wherever the device implements it.
struct IndicationClient {
    cmd: Commander,
}

mod indication_client {
    use super::*;
    wire_weaver::ww_codegen!(
        ww_indication::Indication for IndicationClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
    );
}

/// One device being synced.
struct Synced {
    name: String,
    client: DynClient,
    sync: Option<ClockSync>,
    /// Why it isn't synced, or what went wrong with `--mode`.
    note: Option<String>,
}

pub(crate) async fn sync(
    args: SyncArgs,
    selection: &Selection,
    mut stop: StopSignal,
) -> Result<()> {
    let config = selection.client_config();
    let listed: Vec<_> = wire_weaver_client::list_usb_devices()
        .await?
        .into_iter()
        .filter(|d| config.matches(d))
        .collect();
    let picked = pick(&listed, &args.devices)?;
    if picked.is_empty() {
        bail!("no matching devices ({selection}), run 'ww list' to see connected devices");
    }
    let options = Options {
        poll: Duration::from_millis(args.poll_ms.max(1)),
        steer_every: Duration::from_millis(args.steer_ms.max(1)),
        ..Options::default()
    };

    let mut devices = Vec::new();
    for device in picked {
        let name = name(device);
        let Some(usb) = &device.usb else {
            continue;
        };
        let config = config
            .clone()
            .traits_only()
            .usb_port_chain(usb.bus_id.clone(), usb.port_chain.clone());
        let client = DynClient::from_config(config)
            .connect()
            .await
            .with_context(|| format!("connecting to {name}"))?;
        let (sync, note) = match ClockSync::start(client.cmd(), options).await {
            Ok(sync) => (Some(sync), None),
            Err(e) => (None, Some(format!("not synced: {e}"))),
        };
        devices.push(Synced {
            name,
            client,
            sync,
            note,
        });
    }

    if let Some(mode) = args.mode {
        for device in &mut devices {
            let indication = IndicationClient {
                cmd: device.client.cmd().clone(),
            };
            if let Err(e) = indication.write_mode(mode.into()).write().await {
                device.note = Some(format!("mode not set: {e}"));
            }
        }
    }

    let term = Term::stdout();
    let live = term.is_term();
    let mut shown = 0;
    let mut every = tokio::time::interval(Duration::from_secs(if live { 1 } else { 5 }));
    let run = async {
        loop {
            every.tick().await;
            let table = render(&rows(&devices));
            if live && shown > 0 {
                _ = term.clear_last_lines(shown);
            }
            print!("{table}");
            shown = table.lines().count();
        }
    };
    match args.duration {
        Some(s) => {
            _ = stop
                .guard(tokio::time::timeout(Duration::from_secs(s), run))
                .await
        }
        None => _ = stop.guard(run).await,
    }
    for device in devices {
        drop(device.sync);
        _ = device.client.disconnect().asynch().await;
    }
    eprintln!("Stopped: the devices go on in holdover on their last steer point");
    Ok(())
}

/// The devices the arguments name, each matching exactly one, or all of them.
fn pick<'a>(listed: &'a [DeviceInfo], wanted: &[String]) -> Result<Vec<&'a DeviceInfo>> {
    if wanted.is_empty() {
        return Ok(listed.iter().collect());
    }
    let mut picked: Vec<&DeviceInfo> = Vec::new();
    for w in wanted {
        let needle = w.to_lowercase();
        let found: Vec<_> = listed
            .iter()
            .filter(|d| {
                d.serials
                    .iter()
                    .chain(std::iter::once(&d.user_label))
                    .any(|s| !s.is_empty() && s.to_lowercase().contains(&needle))
            })
            .collect();
        match found.as_slice() {
            [] => bail!("no connected device has a serial or label matching '{w}'"),
            [one] => {
                if !picked.iter().any(|p| std::ptr::eq(*p, *one)) {
                    picked.push(one);
                }
            }
            many => bail!(
                "'{w}' matches {} devices ({}), give more of the serial or label",
                many.len(),
                many.iter().map(|d| name(d)).collect::<Vec<_>>().join(", ")
            ),
        }
    }
    Ok(picked)
}

/// Product with label or serial, for the table.
fn name(d: &DeviceInfo) -> String {
    let id = if d.user_label.is_empty() {
        d.serials.first().cloned().unwrap_or_default()
    } else {
        d.user_label.clone()
    };
    format!("{} {id}", d.product).trim().to_string()
}

/// One table row: device, state, source, rate, round trip, residual, samples, note.
type Row = [String; 8];

const HEADER: [&str; 8] = [
    "DEVICE", "STATE", "SOURCE", "RATE", "RTT", "RESIDUAL", "SAMPLES", "NOTE",
];

fn rows(devices: &[Synced]) -> Vec<Row> {
    devices
        .iter()
        .map(|d| match &d.sync {
            Some(sync) => row(&d.name, &sync.status(), d.note.as_deref()),
            None => [
                d.name.clone(),
                "-".into(),
                "-".into(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                d.note.clone().unwrap_or_default(),
            ],
        })
        .collect()
}

fn row(name: &str, s: &Status, note: Option<&str>) -> Row {
    let state = match s.device.map(|d| d.state) {
        Some(SyncState::Locked) => style("locked").green().to_string(),
        Some(SyncState::Holdover) => style("holdover").yellow().to_string(),
        Some(SyncState::Conflict) => style("conflict").red().to_string(),
        Some(SyncState::Acquiring) => "acquiring".into(),
        Some(SyncState::Free) => "free".into(),
        Some(SyncState::Unknown(n)) => format!("<unknown {n}>"),
        None if s.fit.is_some() => "fitted".into(),
        None => "fitting".into(),
    };
    let source = match s.source {
        Some(SyncSource::Host) => "host".into(),
        Some(SyncSource::Line { format }) => format!("line {format:?}"),
        Some(SyncSource::Unknown(n)) => format!("<unknown {n}>"),
        // its source list leaves the host out
        None if s.answered => "none".into(),
        None => "-".into(),
    };
    let rate = s
        .fit
        .map(|f| format!("{:+.3} ppm", f.rate_ppb() as f64 / 1000.0))
        .unwrap_or_default();
    let rtt = s
        .fit
        .map(|f| format!("{} µs", f.min_rtt / 1000))
        .unwrap_or_default();
    let residual = s
        .residual
        .map(|r| format!("{:+} µs", r / 1000))
        .unwrap_or_default();
    let note = s
        .error
        .clone()
        .or(note.map(str::to_string))
        .unwrap_or_default();
    [
        name.to_string(),
        state,
        source,
        rate,
        rtt,
        residual,
        s.samples.to_string(),
        note,
    ]
}

/// The table, columns padded to their widest cell (colour codes not counted).
fn render(rows: &[Row]) -> String {
    let mut widths = HEADER.map(measure_text_width);
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(measure_text_width(cell));
        }
    }
    let line = |cells: Vec<String>| {
        let s = cells
            .iter()
            .zip(widths)
            .map(|(c, w)| format!("{c}{}", " ".repeat(w - measure_text_width(c))))
            .collect::<Vec<_>>()
            .join("  ");
        format!("{}\n", s.trim_end())
    };
    let mut out = line(HEADER.map(|h| style(h).bold().to_string()).to_vec());
    for row in rows {
        out += &line(row.to_vec());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_clock_sync::Fit;

    fn device(serial: &str, label: &str) -> DeviceInfo {
        DeviceInfo {
            location: format!("usb {serial}"),
            manufacturer: "vhrd".into(),
            product: "Donguru".into(),
            serials: vec![serial.into()],
            user_label: label.into(),
            api: None,
            usb: None,
        }
    }

    #[test]
    fn pick_by_serial_or_label() {
        let listed = [
            device("A1B2", ""),
            device("C3D4", "bench left"),
            device("C3E5", ""),
        ];
        assert_eq!(pick(&listed, &[]).unwrap().len(), 3);
        let p = pick(&listed, &["a1".into(), "LEFT".into()]).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].serials[0], "C3D4");
        // the same device twice is synced once
        assert_eq!(
            pick(&listed, &["a1b".into(), "A1".into()]).unwrap().len(),
            1
        );
        let many = pick(&listed, &["c3".into()]).unwrap_err().to_string();
        assert!(many.contains("matches 2 devices"), "{many}");
        assert!(pick(&listed, &["zz".into()]).is_err());
    }

    #[test]
    fn table_rows() {
        console::set_colors_enabled(false);
        let fit = Fit {
            local: 0,
            shared: 0.0,
            slope: 1.0 - 9_900e-6,
            min_rtt: 412_000,
            used: 40,
            residual_rms: 50_000.0,
        };
        let status = Status {
            fit: Some(fit),
            samples: 80,
            steers: 3,
            source: Some(SyncSource::Host),
            residual: Some(-37_400),
            ..Status::default()
        };
        let table = render(&[row("Donguru A1B2", &status, None)]);
        let lines: Vec<_> = table.lines().collect();
        assert_eq!(lines[0].split_whitespace().next(), Some("DEVICE"));
        assert!(lines[1].contains("fitted"), "{table}");
        assert!(lines[1].contains("-9900.000 ppm"), "{table}");
        assert!(lines[1].contains("412 µs"), "{table}");
        assert!(lines[1].contains("-37 µs"), "{table}");
        assert!(lines[1].contains("host"), "{table}");
    }
}
