//! `ww sync`: several devices kept on the host's UTC at once (TOOL-16), with `wire_weaver_client::clock_sync`. With a
//! ww hub running the hub steers (SRV-31) and `ww sync` only shows what the hub does, never a second steerer.

use crate::complete;
use crate::device::Selection;
use crate::untrusted::escape;
use crate::util::{QuitKey, StopSignal};
use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use clap_complete::ArgValueCandidates;
use console::{Term, measure_text_width, style};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wire_weaver_client::clock_sync::{ClockSync, Options, RateCache, Status};
use wire_weaver_client::hub::{HubClient, HubMode};
use wire_weaver_client::{ClientConfig, Commander, DeviceInfo, DynClient};
use ww_hub_api::{SyncEntryOwned, SyncPhaseOwned};
use ww_si::Prefix;
use ww_timebase::{SteererOwned, SyncSource, SyncState};

#[derive(Args)]
pub(crate) struct SyncArgs {
    /// Devices to sync, each a serial number or user label or a part of one (case-insensitive) that matches
    /// exactly one connected device. None: every device the device selection matches
    #[arg(value_name = "DEVICE", add = ArgValueCandidates::new(complete::serials_and_labels))]
    devices: Vec<String>,

    /// Also write this indication mode (ww_indication) on every device; their animations line up on UTC
    #[arg(long, value_enum)]
    mode: Option<IndicationMode>,

    /// Stop after this many seconds (default: until Ctrl+C or q)
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
    /// Steered by this process
    sync: Option<ClockSync>,
    /// Steered by the hub: the device's key there
    key: Option<String>,
    /// Why it isn't synced, or what went wrong with `--mode`.
    note: Option<String>,
}

/// The hub's `ClockSync` entries by device key, kept current from its `stats` stream.
type HubEntries = Arc<Mutex<BTreeMap<String, SyncEntryOwned>>>;

/// What the table and the convergence look at, from this process's own sync or from the hub's.
#[derive(Clone, Debug, Default)]
struct View {
    /// The device's state at the last check
    state: Option<SyncState>,
    /// There is a fit of its clock
    fitted: bool,
    source: Option<SyncSource>,
    /// The device answered a steer point: `source` is its answer
    answered: bool,
    rate_ppb: Option<i32>,
    /// Shortest round trip of the fit, ns
    min_rtt: Option<u64>,
    /// Residual at the last check, ns
    residual: Option<i64>,
    /// Round trip of the last check, ns
    check_rtt: Option<u64>,
    /// The device's error bound, µs
    error_us: Option<i64>,
    slew_us: Option<i32>,
    samples: usize,
    error: Option<String>,
    /// Whom the device follows, and another steerer it only notes, as it says
    steerer: Option<SteererOwned>,
    noted: Option<SteererOwned>,
}

impl From<&Status> for View {
    fn from(s: &Status) -> Self {
        let device = s.device.as_ref();
        View {
            state: device.map(|d| d.state),
            fitted: s.fit.is_some(),
            source: s.source,
            answered: s.answered,
            rate_ppb: s.fit.map(|f| f.rate_ppb()),
            min_rtt: s.fit.map(|f| f.min_rtt),
            residual: s.residual,
            check_rtt: s.check_rtt,
            error_us: device
                .and_then(|d| d.error)
                .map(|e| e.to_f64_in(Prefix::Micro) as i64),
            slew_us: device.and_then(|d| d.slew_us),
            samples: s.samples,
            error: s.error.clone(),
            steerer: device.and_then(|d| d.steerer.clone()),
            noted: device.and_then(|d| d.noted.clone()),
        }
    }
}

impl View {
    /// The hub's entry of a device: a view while the hub steers it, else why not.
    fn of_hub(e: &SyncEntryOwned) -> std::result::Result<View, String> {
        match &e.phase {
            SyncPhaseOwned::Steering => Ok(View {
                state: e.state,
                fitted: e.rate_ppb.is_some(),
                source: e.source,
                answered: e.steers > 0,
                rate_ppb: e.rate_ppb,
                min_rtt: e.min_rtt_ns,
                residual: e.residual_ns,
                check_rtt: e.check_rtt_ns,
                error_us: e.error_us.map(i64::from),
                slew_us: e.slew_us,
                samples: e.samples as usize,
                error: e.error.clone(),
                steerer: e.steerer.clone(),
                noted: e.noted.clone(),
            }),
            SyncPhaseOwned::Starting => Err("the hub looks at its API".into()),
            SyncPhaseOwned::Off => Err("steering is off in the hub".into()),
            SyncPhaseOwned::Paused => {
                Err("the hub paused steering: a program has it to itself".into())
            }
            SyncPhaseOwned::Unsupported => Err("no ww_timebase::Clock with host sync".into()),
            SyncPhaseOwned::Failed { why } => {
                Err(format!("the hub can't steer it: {}", escape(why)))
            }
            SyncPhaseOwned::Unknown(n) => Err(format!("hub phase <unknown {n}>")),
        }
    }
}

/// Who steers, for the table: program and pid, with the host when it isn't this one.
fn steerer_name(s: &SteererOwned) -> String {
    let this_host = wire_weaver_client::ClientMeta::new().resolve().host;
    match s.host == this_host {
        true => format!("{} pid {}", escape(&s.program), s.pid),
        false => format!(
            "{} pid {} on {}",
            escape(&s.program),
            s.pid,
            escape(&s.host)
        ),
    }
}

pub(crate) async fn sync(
    args: SyncArgs,
    selection: &Selection,
    mut stop: StopSignal,
) -> Result<()> {
    let targets = targets(selection, &args.devices).await?;
    let options = Options {
        poll: Duration::from_millis(args.poll_ms.max(1)),
        steer_every: Duration::from_millis(args.steer_ms.max(1)),
        ..Options::default()
    };
    // with a hub the hub steers, ww sync only looks (SRV-31); WW_HUB=off steers here
    let hub = match HubMode::from_env() != HubMode::Direct && wire_weaver_client::hub::hub_running()
    {
        true => Some(
            HubClient::connect()
                .await
                .context("connecting to the ww hub")?,
        ),
        false => None,
    };
    // on a terminal one live view from the start: who steers, the table, how far they converged; into a pipe the
    // same as lines (who steers, a line per convergence change) and the table at the end
    let term = Term::stdout();
    let live = term.is_term();
    // short for the live view's header, a terminal line wide
    let (steering, more) = match hub {
        Some(_) => (
            "Steered by the ww hub: it keeps steering them after ww sync stops",
            "--poll-ms and --steer-ms are the hub's own, WW_HUB=off steers from here",
        ),
        None => (
            "Steered by ww sync (no ww hub runs): they go on in holdover once it stops",
            "a ww hub keeps them steered: ww hub take DEVICE",
        ),
    };
    let mut shown = 0;
    match live {
        true => {
            let connecting = format!("Connecting to {} device(s)…", targets.len());
            _ = term.write_line(&style(connecting).dim().to_string());
            shown = 1;
        }
        false => eprintln!("{steering} ({more})"),
    }
    let entries = HubEntries::default();
    if let Some(hub) = &hub {
        let mut stats = hub.sync().stats().call().await.context(
            "the running ww hub doesn't steer clocks (SRV-31): update it, or WW_HUB=off",
        )?;
        let entries = entries.clone();
        tokio::spawn(async move {
            while let Ok(e) = stats.recv().await {
                entries.lock().unwrap().insert(e.key.clone(), e);
            }
        });
    }

    let mut devices = Vec::new();
    for Target {
        name,
        config,
        key,
        rate,
    } in targets
    {
        if hub.is_some() {
            let Some(key) = key else {
                continue;
            };
            // a tunnel: the hub opens the device and steers it, and keeps it steered after ww sync exits
            let client =
                DynClient::from_config(ClientConfig::new().traits_only().hub_device(key.clone()))
                    .connect()
                    .await
                    .with_context(|| format!("connecting to {name} through the ww hub"))?;
            devices.push(Synced {
                name,
                client,
                sync: None,
                key: Some(key),
                note: None,
            });
            continue;
        }
        // steered by a sync of its own with --poll-ms and --steer-ms, not the connection's (CLT-46)
        let config = config.traits_only().hub(HubMode::Direct).clock_sync(false);
        let client = DynClient::from_config(config)
            .connect()
            .await
            .with_context(|| format!("connecting to {name}"))?;
        // the rate this PC measured last time, shared with the hub (SRV-31)
        let options = Options {
            rate_cache: rate.and_then(|rate| RateCache::for_device(&rate).ok()),
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
            key: None,
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

    let names: Vec<_> = devices.iter().map(|d| d.name.clone()).collect();
    let mut convergence = Convergence::new(args.within_us as i64 * 1000, devices.len());
    let started = Instant::now();
    let quit_key = live.then(|| QuitKey::listen(&stop)).flatten();
    let footer = match quit_key {
        Some(_) => "q or Ctrl+C to stop",
        None => "Ctrl+C to stop",
    };
    let frame = |convergence: &Convergence, footer: Option<&str>| {
        let views = views(&devices, &entries);
        let statuses: Vec<_> = views.iter().map(|(v, _)| v.clone()).collect();
        let now = started.elapsed().as_nanos() as u64;
        let mut out = match live {
            true => format!("{}\n", style(steering).dim()),
            false => String::new(),
        };
        out += &render(&rows(&devices, &views));
        out += &convergence.status_line(&names, args.within_us, now);
        out += &phase_line(&statuses);
        if let Some(footer) = footer {
            out += &format!("{}\n", style(footer).dim());
        }
        out
    };
    let mut every = tokio::time::interval(Duration::from_millis(if live { 1000 } else { 200 }));
    let run = async {
        loop {
            every.tick().await;
            let views = views(&devices, &entries);
            let statuses: Vec<_> = views.iter().map(|(v, _)| v.clone()).collect();
            let events = convergence.update(started.elapsed().as_nanos() as u64, &statuses);
            match live {
                true => {
                    let frame = frame(&convergence, Some(footer));
                    _ = term.clear_last_lines(shown);
                    print!("{frame}");
                    shown = frame.lines().count();
                }
                false => {
                    for e in &events {
                        println!("{}", e.line(&names, args.within_us));
                    }
                }
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
    drop(quit_key);
    // the last state stays on screen, without the footer
    if live {
        _ = term.clear_last_lines(shown);
    }
    print!("{}", frame(&convergence, None));
    for device in devices {
        drop(device.sync);
        _ = device.client.disconnect().asynch().await;
    }
    match hub {
        Some(_) => {
            eprintln!("Stopped: the ww hub keeps steering them for its sync linger (ww hub status)")
        }
        // a hub started while ww sync ran (by another program) steers what it holds now
        None if wire_weaver_client::hub::hub_running() => eprintln!(
            "Stopped: a ww hub runs now and steers the devices it holds (ww hub status); \
             the others go on in holdover from their last steer point"
        ),
        None => eprintln!(
            "Stopped: nothing steers the devices now, they go on in holdover from their last steer point"
        ),
    }
    Ok(())
}

/// A device to sync, before connecting to it.
struct Target {
    name: String,
    /// How to reach it directly
    config: ClientConfig,
    /// Its key in the hub, see [hub::key_of](wire_weaver_client::hub::key_of)
    key: Option<String>,
    /// Its rate cache name, see [rate_key](wire_weaver_client::clock_sync::rate_key)
    rate: Option<String>,
}

/// USB devices the selection matches and `wanted` picks, or the one device behind the debug probe with `--rtt`.
async fn targets(selection: &Selection, wanted: &[String]) -> Result<Vec<Target>> {
    let config = selection.client_config();
    if let Some(chip) = selection.rtt() {
        if !wanted.is_empty() {
            bail!(
                "with --rtt the device is the one behind the probe: pick the probe with --probe, not DEVICE"
            );
        }
        let probe = selection.rtt_probe()?;
        let key = selection.rtt_key()?;
        return Ok(vec![Target {
            name: format!(
                "{chip} via {}",
                escape(probe.serial.as_deref().unwrap_or(&probe.name))
            ),
            rate: key
                .as_deref()
                .and_then(wire_weaver_client::clock_sync::rate_key),
            key,
            config,
        }]);
    }
    let listed: Vec<_> = wire_weaver_client::list_usb_devices()
        .await?
        .into_iter()
        .filter(|d| config.matches(d))
        .collect();
    let picked = pick(&listed, wanted)?;
    if picked.is_empty() {
        bail!("no matching devices ({selection}), run 'ww list' to see connected devices");
    }
    Ok(picked
        .into_iter()
        .filter_map(|device| {
            let usb = device.usb.as_ref()?;
            Some(Target {
                name: name(device),
                config: config
                    .clone()
                    .usb_port_chain(usb.bus_id.clone(), usb.port_chain.clone()),
                key: wire_weaver_client::hub::key_of(device),
                rate: device.serials.first().cloned(),
            })
        })
        .collect())
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
    format!("{} {}", escape(&d.product), escape(&id))
        .trim()
        .to_string()
}

/// One table row: device, state, source, steerer, residual, error, rate, slew left, round trip, samples, note.
type Row = [String; 11];

const HEADER: [&str; 11] = [
    "DEVICE",
    "STATE",
    "SOURCE",
    "STEERED BY",
    "RESIDUAL",
    "ERROR",
    "RATE",
    "SLEW",
    "RTT",
    "SAMPLES",
    "NOTE",
];

/// Each device's view, `None` while nothing steers it, with a note (why not, or what went wrong).
fn views(devices: &[Synced], entries: &HubEntries) -> Vec<(Option<View>, Option<String>)> {
    let entries = entries.lock().unwrap();
    devices
        .iter()
        .map(|d| {
            if let Some(sync) = &d.sync {
                return (Some(View::from(&sync.status())), d.note.clone());
            }
            match d.key.as_ref().map(|k| entries.get(k)) {
                Some(Some(e)) => match View::of_hub(e) {
                    Ok(v) => (Some(v), d.note.clone()),
                    Err(why) => (None, Some(why)),
                },
                Some(None) => (None, Some("waiting for the hub".into())),
                None => (None, d.note.clone()),
            }
        })
        .collect()
}

fn rows(devices: &[Synced], views: &[(Option<View>, Option<String>)]) -> Vec<Row> {
    devices
        .iter()
        .zip(views)
        .map(|(d, (view, note))| match view {
            Some(v) => row(&d.name, v, note.as_deref()),
            None => {
                let mut row = Row::default();
                row[0] = d.name.clone();
                row[1] = "-".into();
                row[2] = "-".into();
                row[10] = note.clone().unwrap_or_default();
                row
            }
        })
        .collect()
}

fn row(name: &str, s: &View, note: Option<&str>) -> Row {
    let state = match s.state {
        Some(SyncState::Locked) => style("locked").green().to_string(),
        Some(SyncState::Holdover) => style("holdover").yellow().to_string(),
        Some(SyncState::Conflict) => style("conflict").red().to_string(),
        Some(SyncState::Acquiring) => style("acquiring").yellow().to_string(),
        Some(SyncState::Free) => "free".into(),
        Some(SyncState::Unknown(n)) => format!("<unknown {n}>"),
        None if s.fitted => "fitted".into(),
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
    let steerer = s.steerer.as_ref().map(steerer_name).unwrap_or_default();
    let rate = s
        .rate_ppb
        .map(|r| format!("{:+.3} ppm", r as f64 / 1000.0))
        .unwrap_or_default();
    let rtt = s
        .min_rtt
        .map(|r| format!("{} µs", r / 1000))
        .unwrap_or_default();
    // good to half the check's round trip
    let residual = s
        .residual
        .map(|r| match s.check_rtt {
            Some(rtt) => format!("{} ±{}", signed_us(r / 1000), us(rtt as i64 / 2000)),
            None => signed_us(r / 1000),
        })
        .unwrap_or_default();
    let error = s.error_us.map(us).unwrap_or_default();
    let slew = s.slew_us.map(|v| signed_us(v as i64)).unwrap_or_default();
    let mut note = s
        .error
        .clone()
        .or(note.map(str::to_string))
        .unwrap_or_default();
    if let Some(other) = &s.noted {
        if !note.is_empty() {
            note += "; ";
        }
        note += &format!("also steered by {}, only noted", steerer_name(other));
    }
    [
        name.to_string(),
        state,
        source,
        steerer,
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

/// The estimated phase difference between the devices: the spread of their residuals, good to half the longest
/// check's round trip. Anything a device starts on a UTC multiple of a period (an animation, a sample, a frame) is
/// that far apart between them. Empty below two devices with a residual.
fn phase_line(statuses: &[Option<View>]) -> String {
    let Some((spread, uncertainty, n)) = spread(statuses) else {
        return String::new();
    };
    format!(
        "Phase: {} apart across {n} devices (±{})\n",
        us(spread / 1000),
        us(uncertainty / 1000)
    )
}

/// Largest minus smallest residual, half the longest check round trip, and how many devices had one; ns.
fn spread(statuses: &[Option<View>]) -> Option<(i64, i64, usize)> {
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
    /// Which devices had a status at the last update.
    synced: Vec<bool>,
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
            synced: vec![false; devices],
            all_since: None,
        }
    }

    fn update(&mut self, now: u64, statuses: &[Option<View>]) -> Vec<Event> {
        let mut events = Vec::new();
        for (index, (since, status)) in self.since.iter_mut().zip(statuses).enumerate() {
            let Some(s) = status else { continue };
            let state = s.state;
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
        self.synced = statuses.iter().map(Option::is_some).collect();
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

impl Convergence {
    /// How far the devices converged, for the live view: `2 of 3 within 2.00 ms, waiting for Donguru`, or `all 3
    /// within 2.00 ms for 4.1 s` (`now` ns since the start). Empty before any device has a status.
    fn status_line(&self, names: &[String], within_us: u64, now: u64) -> String {
        let synced: Vec<_> = self
            .synced
            .iter()
            .enumerate()
            .filter(|(_, s)| **s)
            .map(|(i, _)| i)
            .collect();
        if synced.is_empty() {
            return String::new();
        }
        let within = us(within_us as i64);
        if let Some(since) = self.all_since {
            return format!(
                "{} all {} within {within} for {:.1} s\n",
                style("✓").green().bold(),
                synced.len(),
                now.saturating_sub(since) as f64 / 1e9
            );
        }
        let waiting: Vec<_> = synced
            .iter()
            .filter(|i| self.since[**i].is_none())
            .map(|i| names[*i].as_str())
            .collect();
        format!(
            "{} of {} within {within}, waiting for {}\n",
            synced.len() - waiting.len(),
            synced.len(),
            waiting.join(", ")
        )
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
                    format!(", phase {} apart (±{})", us(s / 1000), us(u / 1000))
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
        let view = View {
            fitted: true,
            rate_ppb: Some(-9_900_000),
            min_rtt: Some(412_000),
            samples: 80,
            source: Some(SyncSource::Host),
            answered: true,
            residual: Some(-37_400),
            ..View::default()
        };
        let table = render(&[row("Donguru A1B2", &view, None)]);
        let lines: Vec<_> = table.lines().collect();
        assert_eq!(lines[0].split_whitespace().next(), Some("DEVICE"));
        assert!(lines[1].contains("fitted"), "{table}");
        assert!(lines[1].contains("-9900.000 ppm"), "{table}");
        assert!(lines[1].contains("412 µs"), "{table}");
        assert!(lines[1].contains("-37 µs"), "{table}");
        assert!(lines[1].contains("host"), "{table}");
        let view = View {
            state: Some(SyncState::Acquiring),
            error_us: Some(1_250),
            slew_us: Some(-48_000),
            check_rtt: Some(1_400_000),
            steerer: Some(SteererOwned {
                id: 1,
                host: "lab-pc".into(),
                program: "ww hub".into(),
                pid: 4711,
            }),
            noted: Some(SteererOwned {
                id: 2,
                host: "lab-pc".into(),
                program: "app".into(),
                pid: 7,
            }),
            ..view
        };
        let table = render(&[row("Donguru A1B2", &view, None)]);
        assert!(table.contains("acquiring"), "{table}");
        assert!(table.contains("-37 µs ±700 µs"), "{table}");
        assert!(table.contains("1.25 ms"), "{table}");
        assert!(table.contains("-48.00 ms"), "{table}");
        assert!(table.contains("ww hub pid 4711 on lab-pc"), "{table}");
        assert!(
            table.contains("also steered by app pid 7 on lab-pc, only noted"),
            "{table}"
        );
    }

    #[test]
    fn hub_entries_as_views() {
        let mut e = SyncEntryOwned {
            key: "usb:c0de:cafe:A1".into(),
            phase: SyncPhaseOwned::Steering,
            state: Some(SyncState::Locked),
            source: Some(SyncSource::Host),
            steerer: None,
            noted: None,
            followed: Some(true),
            rate_ppb: Some(-12_000),
            rate_from_cache: true,
            min_rtt_ns: Some(300_000),
            residual_ns: Some(-50_000),
            check_rtt_ns: Some(400_000),
            error_us: Some(900),
            slew_us: Some(3),
            samples: 12,
            steers: 4,
            restarts: 0,
            error: None,
        };
        let v = View::of_hub(&e).unwrap();
        assert_eq!(v.state, Some(SyncState::Locked));
        assert_eq!(
            (v.rate_ppb, v.min_rtt, v.error_us),
            (Some(-12_000), Some(300_000), Some(900))
        );
        assert!(v.answered && v.fitted);
        e.phase = SyncPhaseOwned::Off;
        assert_eq!(View::of_hub(&e).unwrap_err(), "steering is off in the hub");
    }

    fn status(state: SyncState, residual_us: i64) -> Option<View> {
        Some(View {
            state: Some(state),
            source: Some(SyncSource::Host),
            slew_us: Some(0),
            residual: Some(residual_us * 1000),
            check_rtt: Some(200_000),
            ..View::default()
        })
    }

    #[test]
    fn convergence_per_device_then_all() {
        console::set_colors_enabled(false);
        let s = 1_000_000_000;
        let mut c = Convergence::new(2_000_000, 3);
        let names = ["Donguru".to_string(), "rePort".into(), "x".into()];
        // the live view's status line: nothing before any status
        assert_eq!(c.status_line(&names, 2000, 0), "");
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
        assert_eq!(
            c.status_line(&names, 2000, 2 * s),
            "1 of 2 within 2.00 ms, waiting for Donguru\n"
        );
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
        assert_eq!(
            c.status_line(&names, 2000, 7 * s),
            "✓ all 2 within 2.00 ms for 2.0 s\n"
        );
        assert_eq!(
            events[1].line(&names, 2000),
            "✓ all 2 within 2.00 ms after 5.0 s, phase 1.20 ms apart (±100 µs)"
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
            "Phase: 1.00 ms apart across 2 devices (±100 µs)\n"
        );
    }
}
