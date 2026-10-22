//! `ww hub`: the per-user hub that holds devices and serves every program (SRV-13, TOOL-17). `serve` runs it in the
//! foreground, `status`, `stop`, `keep` and `who` look at and steer a running one, `take` has it hold a device
//! (CLT-39), `lend` hands a device to a program that isn't a WireWeaver one and takes it back (SRV-32).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Subcommand, ValueHint};
use clap_complete::ArgValueCandidates;
use console::{measure_text_width, style};
use wire_weaver_client::holders::{self, DeviceRef, Holder};
use wire_weaver_client::hub::hub_running;
use ww_hub::client::HubClient;
use ww_hub_api::{ClientEntryOwned, DeviceEntryOwned, DeviceStateOwned};

use crate::complete;
use crate::untrusted::escape;

#[derive(Subcommand)]
pub(crate) enum HubCommand {
    /// Run the hub in the foreground until it is idle or stopped (programs start it by themselves when it pays off;
    /// this is for watching its log or debugging)
    Serve {
        /// Exit after this many seconds without connections (default 300, or WW_HUB_IDLE_EXIT)
        #[arg(long)]
        idle_exit: Option<u64>,
    },
    /// Have the hub open a device now and keep it, so every program reaches it through the hub from its first call
    /// (starts the hub if none runs)
    Take {
        /// Device key as the hub lists it (usb:VID:PID:SERIAL), or the serial number of a WireWeaver USB device
        #[arg(add = ArgValueCandidates::new(complete::device_keys))]
        device: String,
    },
    /// Show the running hub: version, process, who started it, uptime, devices, programs, idle timers.
    /// Exits with 1 when no hub runs
    Status,
    /// Stop the running hub: programs connected through it see their devices go, devices are released
    Stop,
    /// Who holds each device: the hub, a ww program using it directly, or another program (Linux only, from /proc)
    Who {
        /// Only this device: its key, serial, label or product
        #[arg(add = ArgValueCandidates::new(complete::device_keys))]
        device: Option<String>,
    },
    /// Lend a device to a program that isn't a WireWeaver one (probe-rs, dfu-util, a serial terminal): the hub
    /// closes it (programs reaching it through the hub see their connection end and wait), runs the command, and
    /// opens it again after. Ctrl+C goes to the command. Exits with the command's exit code
    ///
    /// Without a running hub the command just runs. Example: ww hub lend 0B135A01 -- probe-rs reset --chip STM32H725
    Lend {
        /// Device key as the hub lists it (usb:VID:PID:SERIAL), or the serial number of a WireWeaver USB device
        #[arg(add = ArgValueCandidates::new(complete::device_keys))]
        device: String,
        /// The command and its arguments, after `--`
        #[arg(last = true, required = true, num_args = 1.., value_hint = ValueHint::CommandWithArguments)]
        command: Vec<String>,
    },
    /// Keep the hub (and what it holds open) up without programs for a while, for a bench: `30m`, `2h`, `1d`, plain
    /// seconds, `forever` until stopped, `off` back to the idle timeout
    Keep {
        /// How long (default: forever)
        #[arg(default_value = "forever", add = ArgValueCandidates::new(keep_candidates))]
        duration: String,
    },
}

pub(crate) async fn hub(cmd: HubCommand) -> Result<()> {
    match cmd {
        HubCommand::Serve { idle_exit } => {
            let mut config = ww_hub::HubConfig::from_env()?;
            if let Some(s) = idle_exit {
                config.idle_exit = Duration::from_secs(s);
            }
            // on a thread and runtime of its own, see `run_blocking`
            match tokio::task::spawn_blocking(move || ww_hub::run_blocking_with(config)).await?? {
                ww_hub::Exit::AlreadyRunning => {
                    anyhow::bail!("a hub runs for this user already")
                }
                exit => eprintln!("ww hub: {exit:?}"),
            }
        }
        HubCommand::Take { device } => {
            let key = device_key(&device).await?;
            wire_weaver_client::hub::take(&key).await?;
            eprintln!("the ww hub holds {key}");
        }
        HubCommand::Status => return status().await,
        HubCommand::Stop => stop().await?,
        HubCommand::Who { device } => who(device.as_deref()).await?,
        HubCommand::Keep { duration } => keep(&duration).await?,
        HubCommand::Lend { device, command } => lend(&device, &command).await?,
    }
    Ok(())
}

/// Longest purpose a lend shows others: the command line, cut.
const LEND_PURPOSE_CHARS: usize = 80;

async fn lend(device: &str, command: &[String]) -> Result<()> {
    let key = device_key(device).await?;
    let shown = command.join(" ");
    let purpose: String = shown.chars().take(LEND_PURPOSE_CHARS).collect();
    let lent = wire_weaver_client::hub::Lend::take(&key, &purpose).await?;
    match &lent {
        Some(_) => eprintln!(
            "{} {key} lent to `{shown}`, the hub opens it again after",
            style("ww hub").bold()
        ),
        None => eprintln!(
            "{} no hub runs: `{shown}` runs as it is",
            style("ww hub").bold()
        ),
    }
    let status = run_passing_ctrl_c(command).await;
    if let Some(lent) = lent {
        match lent.release().await {
            Ok(()) => eprintln!("{} {key} returned to the hub", style("ww hub").bold()),
            // the hub returns it by itself once this program is gone
            Err(e) => eprintln!("{} returning {key}: {e:#}", style("note").yellow()),
        }
    }
    let code = exit_code(status?);
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

/// Runs `command` with this terminal and waits for it. Ctrl+C reaches it from the terminal (its process group, its
/// console); this program only keeps running meanwhile, to give the device back after.
async fn run_passing_ctrl_c(command: &[String]) -> Result<std::process::ExitStatus> {
    let (program, args) = command.split_first().context("no command to run")?;
    let mut child = std::process::Command::new(program)
        .args(args)
        .spawn()
        .with_context(|| format!("starting {program}"))?;
    let wait = tokio::task::spawn_blocking(move || child.wait());
    tokio::pin!(wait);
    loop {
        tokio::select! {
            status = &mut wait => return status?.with_context(|| format!("waiting for {program}")),
            // the command got it too and decides what it means
            _ = tokio::signal::ctrl_c() => {}
        }
    }
}

/// The command's exit code, 128 + the signal that ended it (Unix).
fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}

/// A key as given, or the key of the one WireWeaver USB device with this serial number.
async fn device_key(device: &str) -> Result<String> {
    if device.contains(':') {
        return Ok(device.to_string());
    }
    let devices = wire_weaver_client::list_usb_devices().await?;
    let mut keys = devices
        .iter()
        .filter(|d| d.serials.iter().any(|s| s == device))
        .filter_map(wire_weaver_client::hub::key_of);
    match (keys.next(), keys.next()) {
        (Some(key), None) => Ok(key),
        (None, _) => anyhow::bail!("no WireWeaver USB device with serial {device}"),
        (Some(_), Some(_)) => {
            anyhow::bail!("several devices with serial {device}, give the key (usb:VID:PID:SERIAL)")
        }
    }
}

/// A client of the running hub's API, `None` when no hub runs.
pub(crate) async fn connect_hub() -> Result<Option<HubClient>> {
    if !hub_running() {
        return Ok(None);
    }
    let client = tokio::time::timeout(Duration::from_secs(3), HubClient::config(|c| c).connect())
        .await
        .context("the hub holds hub.lock but did not answer within 3 s")?
        .context("connecting to the hub")?;
    Ok(Some(client))
}

async fn need_hub() -> Result<HubClient> {
    connect_hub().await?.context(
        "no hub is running (programs start it when it pays off, `ww hub serve` runs one by hand)",
    )
}

async fn status() -> Result<()> {
    let Some(hub) = connect_hub().await? else {
        println!("{}", style("no hub running").yellow());
        std::process::exit(1);
    };
    let info = hub.info().call().await?;
    let st = hub.status().call().await?;
    let devices = hub.registry().devices().call().await?;
    let clients = hub.registry().clients().call().await?;
    let uptime = now_ms().saturating_sub(info.started_ms);

    let key = |k: &str| style(format!("{k:<11}")).dim().to_string();
    println!(
        "{} {} {}",
        style("ww hub").bold(),
        style(&info.hub_version).bold(),
        style("running").green()
    );
    println!("{}wire_weaver {}", key("built with"), info.ww_version);
    println!("{}{}", key("pid"), info.pid);
    println!("{}{}", key("exe"), info.exe);
    println!("{}{}", key("socket"), info.socket);
    let started_by = if st.started_by.is_empty() {
        "unknown".to_string()
    } else {
        escape(&st.started_by).into_owned()
    };
    println!(
        "{}{started_by}, up {}",
        key("started by"),
        fmt_secs(uptime / 1000)
    );
    println!(
        "{}{} program{}, {} connection{}",
        key("clients"),
        clients.len(),
        plural(clients.len()),
        st.connections,
        plural(st.connections as usize)
    );
    let open = devices
        .iter()
        .filter(|d| matches!(d.state, DeviceStateOwned::Open))
        .count();
    println!(
        "{}{} known, {open} open through the hub",
        key("devices"),
        devices.len()
    );
    println!("{}{}", key("idle exit"), idle_line(&st));

    if !devices.is_empty() {
        println!();
        print_table(
            &["KEY", "PRODUCT", "STATE", "CONNECTIONS"],
            devices
                .iter()
                .map(|d| {
                    vec![
                        d.key.clone(),
                        escape(&d.product).into_owned(),
                        device_state(d),
                        d.tunnels.to_string(),
                    ]
                })
                .collect(),
        );
    }
    if !clients.is_empty() {
        println!();
        print_clients(&clients);
    }
    Ok(())
}

fn idle_line(st: &ww_hub_api::HubStatusOwned) -> String {
    let after = format!("{} without connections", fmt_secs(st.idle_exit_s as u64));
    match (st.keep_left_s, st.exits_in_s) {
        (Some(left), _) if left >= u32::MAX - 60 => "never, kept up until stopped".to_string(),
        (Some(left), _) => format!(
            "kept up for {} more, then after {after}",
            fmt_secs(left as u64)
        ),
        (None, Some(in_s)) => format!("in {} (after {after})", fmt_secs(in_s as u64)),
        (None, None) => format!("after {after}, counted from the last program leaving"),
    }
}

fn device_state(d: &DeviceEntryOwned) -> String {
    match &d.state {
        DeviceStateOwned::Absent => style("absent").dim().to_string(),
        DeviceStateOwned::Present => "free".to_string(),
        DeviceStateOwned::Open => style("open (hub)").green().to_string(),
        DeviceStateOwned::HeldElsewhere { holder } => style(format!(
            "held by {} (pid {}, {})",
            escape(&holder.program),
            holder.pid,
            holder.mode
        ))
        .yellow()
        .to_string(),
        DeviceStateOwned::Faulted { why } => {
            style(format!("faulted: {}", escape(why))).red().to_string()
        }
        DeviceStateOwned::Lent { claim } => style(super::list::lent_text(claim)).cyan().to_string(),
        // a state a newer hub knows
        DeviceStateOwned::Unknown(_) => "unknown".to_string(),
    }
}

fn print_clients(clients: &[ClientEntryOwned]) {
    let now = now_ms();
    print_table(
        &["ID", "PID", "PROGRAM", "KIND", "USER@HOST", "CONNECTED"],
        clients
            .iter()
            .map(|c| {
                vec![
                    c.id.to_string(),
                    if c.pid == 0 {
                        String::new()
                    } else {
                        c.pid.to_string()
                    },
                    escape(&c.program).into_owned(),
                    escape(&c.kind).into_owned(),
                    if c.user.is_empty() && c.host.is_empty() {
                        String::new()
                    } else {
                        format!("{}@{}", escape(&c.user), escape(&c.host))
                    },
                    format!("{} ago", fmt_secs(now.saturating_sub(c.since_ms) / 1000)),
                ]
            })
            .collect(),
    );
}

async fn stop() -> Result<()> {
    let Some(hub) = connect_hub().await? else {
        println!("no hub running");
        return Ok(());
    };
    // the hub may close the connection before it answers
    let _ = hub.shutdown().call().await;
    // the hub waits for its connections to close before it lets go of hub.lock (500 ms at most): this one too
    drop(hub);
    for _ in 0..60 {
        if !hub_running() {
            println!("{}", style("hub stopped").green());
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    bail!("the hub was asked to stop but still runs after 3 s")
}

async fn keep(duration: &str) -> Result<()> {
    let seconds = parse_keep(duration)?;
    let mut hub = need_hub().await?;
    hub.keep(seconds).call().await?;
    match seconds {
        0 => println!("hub back to its idle timeout"),
        u32::MAX => println!("hub kept up until stopped"),
        s => println!("hub kept up for {}", fmt_secs(s as u64)),
    }
    Ok(())
}

/// Seconds for `Hub::keep`: `30m`, `2h`, `1d`, `90s`, `45` (seconds), `forever` (`u32::MAX`), `off` / `0` (back to the
/// idle timeout).
fn parse_keep(s: &str) -> Result<u32> {
    let s = s.trim();
    match s {
        "forever" => return Ok(u32::MAX),
        "off" => return Ok(0),
        _ => {}
    }
    let (num, unit) = match s.find(|c: char| !c.is_ascii_digit()) {
        Some(i) => s.split_at(i),
        None => (s, "s"),
    };
    let n: u64 = num
        .parse()
        .with_context(|| format!("{s:?}: expected a duration like 30m, 2h, 1d, forever or off"))?;
    let factor = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => bail!("{s:?}: unit {unit:?} not known, use s, m, h or d"),
    };
    u32::try_from(n * factor).map_err(|_| anyhow::anyhow!("{s:?} is too long, use `forever`"))
}

fn keep_candidates() -> Vec<clap_complete::CompletionCandidate> {
    use clap_complete::CompletionCandidate as C;
    [
        ("30m", "half an hour"),
        ("2h", "two hours"),
        ("8h", "a work day"),
        ("1d", "a day"),
        ("forever", "until `ww hub stop`"),
        ("off", "back to the idle timeout"),
    ]
    .into_iter()
    .map(|(v, h)| C::new(v).help(Some(h.into())))
    .collect()
}

/// A device and whoever holds it right now.
#[derive(Debug, PartialEq, Eq)]
struct WhoRow {
    key: String,
    product: String,
    label: String,
    serial: String,
    holder: Option<String>,
}

async fn who(filter: Option<&str>) -> Result<()> {
    let devices = wire_weaver_client::list_usb_devices()
        .await
        .unwrap_or_default();
    let held = holders::list();
    let mut rows: Vec<WhoRow> = Vec::new();
    for d in &devices {
        let serial = d.serials.first().cloned().unwrap_or_default();
        let key = match &d.usb {
            Some(u) => holders::key_for(DeviceRef::Usb {
                vid: u.vid,
                pid: u.pid,
                serial: &serial,
                iface: None,
            }),
            None => continue,
        };
        let mut holder = held_by(&held, &key);
        if holder.is_none() {
            let u = d.usb.as_ref().unwrap();
            holder = holders::holder_of_usb_device(u.vid, u.pid, Some(&serial))
                .map(|who| format!("{}, not a ww program", escape(&who)));
        }
        rows.push(WhoRow {
            key,
            product: d.product.clone(),
            label: d.user_label.clone(),
            serial,
            holder,
        });
    }
    // devices known from their lock file only (an RTT probe)
    for (key, h) in &held {
        if !rows
            .iter()
            .any(|r| key == &r.key || key.starts_with(&format!("{}:", r.key)))
        {
            rows.push(WhoRow {
                key: key.clone(),
                product: String::new(),
                label: String::new(),
                serial: String::new(),
                holder: Some(describe_holder(h)),
            });
        }
    }
    if let Some(f) = filter {
        rows.retain(|r| row_matches(r, f));
    }
    rows.sort_by(|a, b| a.key.cmp(&b.key));

    if rows.is_empty() {
        eprintln!("No matching devices found");
    } else {
        print_table(
            &["KEY", "PRODUCT", "LABEL", "HELD BY"],
            rows.iter()
                .map(|r| {
                    vec![
                        r.key.clone(),
                        escape(&r.product).into_owned(),
                        escape(&r.label).into_owned(),
                        match &r.holder {
                            Some(h) => style(h).yellow().to_string(),
                            None => style("free").green().to_string(),
                        },
                    ]
                })
                .collect(),
        );
    }
    // programs connected to the hub, when one runs
    if let Some(hub) = connect_hub().await? {
        let clients = hub.registry().clients().call().await?;
        if !clients.is_empty() {
            println!("\nconnected to the hub:");
            print_clients(&clients);
        }
    }
    Ok(())
}

fn row_matches(r: &WhoRow, f: &str) -> bool {
    r.key == f
        || r.key.starts_with(&format!("{f}:"))
        || (!f.is_empty() && (r.serial == f || r.label == f || r.product == f))
}

/// The holder of `key` (a lock file's key may carry the interface number, `key:<iface>`).
fn held_by(held: &[(String, Holder)], key: &str) -> Option<String> {
    held.iter()
        .find(|(k, _)| k == key || k.starts_with(&format!("{key}:")))
        .map(|(_, h)| describe_holder(h))
}

fn describe_holder(h: &Holder) -> String {
    let age = SystemTime::now()
        .duration_since(h.since)
        .unwrap_or_default()
        .as_secs();
    let program = if h.mode == holders::Mode::Hub {
        "ww hub".to_string()
    } else {
        escape(&h.program).into_owned()
    };
    format!(
        "{program} (pid {}, {}, for {})",
        h.pid,
        h.mode,
        fmt_secs(age)
    )
}

/// `3s`, `4m 05s`, `2h 10m`, `3d 4h`.
fn fmt_secs(s: u64) -> String {
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {:02}s", s / 60, s % 60),
        3600..86400 => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
        _ => format!("{}d {}h", s / 86400, s % 86400 / 3600),
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Left aligned columns, bold header; cells may carry colour (widths ignore escape codes).
pub(crate) fn print_table(header: &[&str], rows: Vec<Vec<String>>) {
    let mut widths: Vec<usize> = header.iter().map(|h| measure_text_width(h)).collect();
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(measure_text_width(cell));
        }
    }
    let line = |cells: Vec<String>| {
        let line = cells
            .iter()
            .zip(&widths)
            .map(|(cell, w)| format!("{cell}{}", " ".repeat(w - measure_text_width(cell))))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{}", line.trim_end());
    };
    line(header.iter().map(|h| style(h).bold().to_string()).collect());
    for row in rows {
        line(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_durations() {
        assert_eq!(parse_keep("30m").unwrap(), 1800);
        assert_eq!(parse_keep("2h").unwrap(), 7200);
        assert_eq!(parse_keep("1d").unwrap(), 86400);
        assert_eq!(parse_keep("90s").unwrap(), 90);
        assert_eq!(parse_keep("45").unwrap(), 45);
        assert_eq!(parse_keep("forever").unwrap(), u32::MAX);
        assert_eq!(parse_keep("off").unwrap(), 0);
        assert!(parse_keep("5x").is_err());
        assert!(parse_keep("soon").is_err());
        assert!(parse_keep("999999999d").is_err());
    }

    #[test]
    fn seconds_read_short() {
        assert_eq!(fmt_secs(3), "3s");
        assert_eq!(fmt_secs(245), "4m 05s");
        assert_eq!(fmt_secs(7800), "2h 10m");
        assert_eq!(fmt_secs(100_000), "1d 3h");
    }

    #[test]
    fn who_filter_takes_key_serial_label_or_product() {
        let row = WhoRow {
            key: "usb:c0de:cafe:123".into(),
            product: "Bench PSU".into(),
            label: "left".into(),
            serial: "123".into(),
            holder: None,
        };
        for f in [
            "usb:c0de:cafe:123",
            "usb:c0de:cafe",
            "123",
            "left",
            "Bench PSU",
        ] {
            assert!(row_matches(&row, f), "{f}");
        }
        assert!(!row_matches(&row, "usb:c0de:cafe:12"));
        assert!(!row_matches(&row, "right"));
        assert!(!row_matches(&row, ""));
    }
}
