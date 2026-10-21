//! `ww sync`: several devices kept on the host's UTC at once (TOOL-16), with `ww_clock_sync`.

use crate::complete;
use crate::device::Selection;
use crate::util::StopSignal;
use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use clap_complete::ArgValueCandidates;
use console::{Term, measure_text_width, style};
use std::time::{Duration, Instant};
use wire_weaver_client::{Commander, DeviceInfo, DynClient};
use ww_clock_sync::{ClockSync, Options, RateCache, Status};
use ww_si::Prefix;
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

    /// Time between two readings of each device's clock once it is locked (ten times as often before)
    #[arg(long, value_name = "MS", default_value_t = 200)]
    poll_ms: u64,

    /// Time between two steer points sent to each device once it is locked (ten times as often before)
    #[arg(long, value_name = "MS", default_value_t = 1000)]
    steer_ms: u64,

    /// A device has converged once it says locked and its residual against the host's UTC is within this
    #[arg(long, value_name = "US", default_value_t = 2000)]
    within_us: u64,
}

/// LED cycle the phase spread is shown against: the bedrock breath (bedrock_led::look::CYCLE_MS).
const LED_CYCLE_MS: f64 = 3000.0;

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
        // the rate this PC measured last time, shared with the hub (SRV-31)
        let options = Options {
            rate_cache: device
                .serials
                .first()
                .and_then(|serial| RateCache::for_device(serial).ok()),
            ..options.clone()
        };
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

    // on a terminal a live table with the events above it; into a pipe only the events, and the table at the end
    let term = Term::stdout();
    let live = term.is_term();
    let names: Vec<_> = devices.iter().map(|d| d.name.clone()).collect();
    let mut convergence = Convergence::new(args.within_us as i64 * 1000, devices.len());
    let started = Instant::now();
    let mut shown = 0;
    let mut every = tokio::time::interval(Duration::from_millis(if live { 1000 } else { 200 }));
    let run = async {
        loop {
            every.tick().await;
            let statuses: Vec<_> = devices
                .iter()
                .map(|d| d.sync.as_ref().map(ClockSync::status))
                .collect();
            let events = convergence.update(started.elapsed().as_nanos() as u64, &statuses);
            if live && shown > 0 {
                _ = term.clear_last_lines(shown);
                shown = 0;
            }
            for e in &events {
                println!("{}", e.line(&names, args.within_us));
            }
            if live {
                let table = render(&rows(&devices)) + &phase_line(&statuses);
                print!("{table}");
                shown = table.lines().count();
            }
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
    if !live {
        let statuses: Vec<_> = devices
            .iter()
            .map(|d| d.sync.as_ref().map(ClockSync::status))
            .collect();
        print!("{}{}", render(&rows(&devices)), phase_line(&statuses));
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

/// One table row: device, state, source, residual, error, rate, slew left, round trip, samples, note.
type Row = [String; 10];

const HEADER: [&str; 10] = [
    "DEVICE", "STATE", "SOURCE", "RESIDUAL", "ERROR", "RATE", "SLEW", "RTT", "SAMPLES", "NOTE",
];

fn rows(devices: &[Synced]) -> Vec<Row> {
    devices
        .iter()
        .map(|d| match &d.sync {
            Some(sync) => row(&d.name, &sync.status(), d.note.as_deref()),
            None => {
                let mut row = Row::default();
                row[0] = d.name.clone();
                row[1] = "-".into();
                row[2] = "-".into();
                row[9] = d.note.clone().unwrap_or_default();
                row
            }
        })
        .collect()
}

fn row(name: &str, s: &Status, note: Option<&str>) -> Row {
    let state = match s.device.as_ref().map(|d| d.state) {
        Some(SyncState::Locked) => style("locked").green().to_string(),
        Some(SyncState::Holdover) => style("holdover").yellow().to_string(),
        Some(SyncState::Conflict) => style("conflict").red().to_string(),
        Some(SyncState::Acquiring) => style("acquiring").yellow().to_string(),
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
    // good to half the check's round trip
    let residual = s
        .residual
        .map(|r| match s.check_rtt {
            Some(rtt) => format!("{} ±{}", signed_us(r / 1000), us(rtt as i64 / 2000)),
            None => signed_us(r / 1000),
        })
        .unwrap_or_default();
    let error = s
        .device
        .as_ref()
        .and_then(|d| d.error)
        .map(|e| us(e.to_f64_in(Prefix::Micro) as i64))
        .unwrap_or_default();
    let slew = s
        .device
        .as_ref()
        .and_then(|d| d.slew_us)
        .map(|v| signed_us(v as i64))
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
        residual,
        error,
        rate,
        slew,
        rtt,
        s.samples.to_string(),
        note,
    ]
}

/// µs, as µs, ms or s by size.
fn us(v: i64) -> String {
    let a = v.unsigned_abs() as f64;
    let sign = if v < 0 { "-" } else { "" };
    if a < 1000.0 {
        format!("{v} µs")
    } else if a < 1e6 {
        format!("{sign}{:.2} ms", a / 1e3)
    } else {
        format!("{sign}{:.3} s", a / 1e6)
    }
}

/// [us] with a sign always.
fn signed_us(v: i64) -> String {
    if v >= 0 { format!("+{}", us(v)) } else { us(v) }
}

/// The estimated LED phase difference between the devices: the spread of their residuals (each LED cycle starts on
/// a UTC multiple of its period, so a device's residual is its cycle's lead on UTC), good to half the longest
/// check's round trip. Empty below two devices with a residual.
fn phase_line(statuses: &[Option<Status>]) -> String {
    let Some((spread, uncertainty, n)) = spread(statuses) else {
        return String::new();
    };
    format!(
        "LED phase: {} apart across {n} devices (±{}), {:.3} % of a {} s breath\n",
        us(spread / 1000),
        us(uncertainty / 1000),
        spread as f64 / 1e4 / LED_CYCLE_MS,
        LED_CYCLE_MS / 1000.0
    )
}

/// Largest minus smallest residual, half the longest check round trip, and how many devices had one; ns.
fn spread(statuses: &[Option<Status>]) -> Option<(i64, i64, usize)> {
    let measured: Vec<_> = statuses
        .iter()
        .flatten()
        .filter_map(|s| s.residual.map(|r| (r, s.check_rtt.unwrap_or(0) as i64 / 2)))
        .collect();
    if measured.len() < 2 {
        return None;
    }
    let max = measured.iter().map(|m| m.0).max()?;
    let min = measured.iter().map(|m| m.0).min()?;
    let uncertainty = measured.iter().map(|m| m.1).max()?;
    Some((max - min, uncertainty, measured.len()))
}

/// When each device, and then all of them, converged: `Locked` by their own word, and the residual within the
/// tolerance by the host's. A check whose half round trip is beyond the tolerance can't tell either way (a USB
/// hiccup): it changes nothing. Fed the statuses with the time since the start (ns), so tests drive it.
struct Convergence {
    within_ns: i64,
    since: Vec<Option<u64>>,
    all_since: Option<u64>,
}

/// A change [Convergence::update] found.
#[derive(Debug, PartialEq)]
enum Event {
    /// Device `index` converged after this many ns, at this residual (ns).
    Converged {
        index: usize,
        after: u64,
        residual: i64,
    },
    /// Device `index` is no longer within: its state now, residual and half the check's round trip.
    Left {
        index: usize,
        state: Option<SyncState>,
        residual: Option<i64>,
        half_rtt: Option<i64>,
    },
    /// Every synced device converged, the last one after this many ns; their residuals' spread and uncertainty.
    All {
        count: usize,
        after: u64,
        spread: Option<(i64, i64)>,
    },
}

impl Convergence {
    fn new(within_ns: i64, devices: usize) -> Self {
        Convergence {
            within_ns,
            since: vec![None; devices],
            all_since: None,
        }
    }

    fn update(&mut self, now: u64, statuses: &[Option<Status>]) -> Vec<Event> {
        let mut events = Vec::new();
        for (index, (since, status)) in self.since.iter_mut().zip(statuses).enumerate() {
            let Some(s) = status else { continue };
            let state = s.device.as_ref().map(|d| d.state);
            let half_rtt = s.check_rtt.map(|r| r as i64 / 2);
            if state == Some(SyncState::Locked) && half_rtt.is_some_and(|h| h > self.within_ns) {
                continue;
            }
            let within = state == Some(SyncState::Locked)
                && s.residual.is_some_and(|r| r.abs() <= self.within_ns);
            match (within, *since) {
                (true, None) => {
                    *since = Some(now);
                    events.push(Event::Converged {
                        index,
                        after: now,
                        residual: s.residual.unwrap_or(0),
                    });
                }
                (false, Some(_)) => {
                    *since = None;
                    events.push(Event::Left {
                        index,
                        state,
                        residual: s.residual,
                        half_rtt,
                    });
                }
                _ => {}
            }
        }
        let synced = statuses.iter().filter(|s| s.is_some()).count();
        let all = synced > 0
            && self
                .since
                .iter()
                .zip(statuses)
                .all(|(since, s)| s.is_none() || since.is_some());
        match (all, self.all_since) {
            (true, None) => {
                self.all_since = Some(now);
                events.push(Event::All {
                    count: synced,
                    after: now,
                    spread: spread(statuses).map(|(s, u, _)| (s, u)),
                });
            }
            (false, Some(_)) => self.all_since = None,
            _ => {}
        }
        events
    }
}

impl Event {
    fn line(&self, names: &[String], within_us: u64) -> String {
        let secs = |ns: u64| format!("{:.1} s", ns as f64 / 1e9);
        match self {
            Event::Converged {
                index,
                after,
                residual,
            } => format!(
                "{} {} converged after {}: locked, residual {} (within {})",
                style("✓").green(),
                names[*index],
                secs(*after),
                signed_us(residual / 1000),
                us(within_us as i64)
            ),
            Event::Left {
                index,
                state,
                residual,
                half_rtt,
            } => format!(
                "{} {} left: {}, residual {}{}",
                style("✗").yellow(),
                names[*index],
                match state {
                    Some(s) => format!("{s:?}").to_lowercase(),
                    None => "no status".into(),
                },
                residual.map_or("unknown".into(), |r| signed_us(r / 1000)),
                half_rtt.map_or(String::new(), |h| format!(" ±{}", us(h / 1000)))
            ),
            Event::All {
                count,
                after,
                spread,
            } => {
                let phase = spread.map_or(String::new(), |(s, u)| {
                    format!(", LED phase {} apart (±{})", us(s / 1000), us(u / 1000))
                });
                format!(
                    "{} all {count} within {} after {}{phase}",
                    style("✓").green().bold(),
                    us(within_us as i64),
                    secs(*after)
                )
            }
        }
    }
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
    use ww_si::Second;
    use ww_timebase::SyncStatusOwned;

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
            prior: false,
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
        let device = SyncStatusOwned {
            state: SyncState::Acquiring,
            offset: None,
            last_lock: None,
            source: Some(SyncSource::Host),
            error: Some(Second::micro(1_250u32)),
            slew_us: Some(-48_000),
            steerer: None,
            noted: None,
        };
        let status = Status {
            device: Some(device),
            check_rtt: Some(1_400_000),
            ..status
        };
        let table = render(&[row("Donguru A1B2", &status, None)]);
        assert!(table.contains("acquiring"), "{table}");
        assert!(table.contains("-37 µs ±700 µs"), "{table}");
        assert!(table.contains("1.25 ms"), "{table}");
        assert!(table.contains("-48.00 ms"), "{table}");
    }

    fn status(state: SyncState, residual_us: i64) -> Option<Status> {
        Some(Status {
            device: Some(SyncStatusOwned {
                state,
                offset: None,
                last_lock: None,
                source: Some(SyncSource::Host),
                error: None,
                slew_us: Some(0),
                steerer: None,
                noted: None,
            }),
            residual: Some(residual_us * 1000),
            check_rtt: Some(200_000),
            ..Status::default()
        })
    }

    #[test]
    fn convergence_per_device_then_all() {
        console::set_colors_enabled(false);
        let s = 1_000_000_000;
        let mut c = Convergence::new(2_000_000, 3);
        // the third device has no clock: it doesn't hold up "all"
        let mut now = vec![
            status(SyncState::Acquiring, 40_000),
            status(SyncState::Locked, 300),
            None,
        ];
        assert_eq!(
            c.update(s, &now),
            [Event::Converged {
                index: 1,
                after: s,
                residual: 300_000
            }]
        );
        assert_eq!(c.update(2 * s, &now), []);
        // locked but the host still sees it 3 ms off: not yet
        now[0] = status(SyncState::Locked, 3_000);
        assert_eq!(c.update(3 * s, &now), []);
        now[0] = status(SyncState::Locked, -900);
        let events = c.update(5 * s, &now);
        assert_eq!(
            events,
            [
                Event::Converged {
                    index: 0,
                    after: 5 * s,
                    residual: -900_000
                },
                Event::All {
                    count: 2,
                    after: 5 * s,
                    spread: Some((1_200_000, 100_000))
                }
            ]
        );
        let names = ["Donguru".to_string(), "rePort".into(), "x".into()];
        assert_eq!(
            events[1].line(&names, 2000),
            "✓ all 2 within 2.00 ms after 5.0 s, LED phase 1.20 ms apart (±100 µs)"
        );
        assert_eq!(
            events[0].line(&names, 2000),
            "✓ Donguru converged after 5.0 s: locked, residual -900 µs (within 2.00 ms)"
        );
        // one falls out, and in again: "all" again
        now[1] = status(SyncState::Acquiring, 5_000);
        let events = c.update(6 * s, &now);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].line(&names, 2000),
            "✗ rePort left: acquiring, residual +5.00 ms ±100 µs"
        );
        now[1] = status(SyncState::Locked, 100);
        assert_eq!(c.update(7 * s, &now).len(), 2);
        // a check with a 6 ms round trip says nothing about 9 ms off: still converged
        let mut slow = status(SyncState::Locked, -9_000);
        slow.as_mut().unwrap().check_rtt = Some(6_000_000);
        assert_eq!(c.update(8 * s, &[now[0].clone(), slow, None]), []);
        assert_eq!(
            phase_line(&now),
            "LED phase: 1.00 ms apart across 2 devices (±100 µs), 0.033 % of a 3 s breath\n"
        );
    }
}
