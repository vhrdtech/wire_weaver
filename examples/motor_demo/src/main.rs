//! `motor_demo`: run the demo device, or observe its properties and see what it costs.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{ArgValueCandidates, CompleteEnv, CompletionCandidate};
use console::style;
use motor_demo::args::{self, mode_str, si};
use motor_demo::client::MotorDemo;
use motor_demo::device::{self, Medium, Options};
use motor_demo::host::{LevelRequest, Session, observe};
use motor_demo::stats::{Level, PROPS, Stats, find};
use motor_demo_api::{ControlMode, Load, Preset, Profile};
use ww_client_server::observe::ObserveMode;

#[derive(Parser)]
#[command(
    name = "motor_demo",
    version = env!("MOTOR_DEMO_VERSION"),
    about = "A VESC-like FOC controller on a simulated motor, its ~50 variables observed over wire_weaver",
    after_help = "Shell completion: source <(COMPLETE=bash motor_demo) (or zsh, fish)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the device on a UDP socket until stopped.
    Device(DeviceArgs),
    /// Observe properties and show per property the rate they arrive at, per level updates, bytes against the
    /// budget and missed sends.
    Observe(ObserveArgs),
    /// List the observable properties.
    List,
}

#[derive(Args)]
struct SimArgs {
    /// Simulated seconds per real second (0.01: a hundred times slower).
    #[arg(long)]
    time_scale: Option<f32>,
    /// Motor and inverter.
    #[arg(long, value_enum)]
    motor: Option<MotorArg>,
    /// Load on the shaft: none, locked, viscous:B, fan:K, constant:T (N·m·s/rad, N·m·s²/rad², N·m).
    #[arg(long, value_parser = args::load)]
    load: Option<Load>,
    /// Built-in speed steps (on by default), off when a setpoint is given.
    #[arg(long, value_enum)]
    profile: Option<ProfileArg>,
}

#[derive(Args)]
struct DeviceArgs {
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1:9000")]
    listen: SocketAddr,
    #[command(flatten)]
    sim: SimArgs,
    /// Seed of the sensor noise.
    #[arg(long, default_value_t = 1)]
    seed: u64,
}

#[derive(Args)]
struct ObserveArgs {
    /// Device to connect to over UDP (`motor_demo device`); without it a device is started in this process.
    #[arg(long)]
    udp: Option<SocketAddr>,
    /// Mode of the 25 kHz properties: off, every:1ms (or 1ms), change:1ms, beat:1ms/100ms. Device time.
    #[arg(long, default_value = "every:10ms", value_parser = args::mode)]
    foc: ObserveMode,
    /// Mode of the 1 kHz properties.
    #[arg(long, default_value = "every:10ms", value_parser = args::mode)]
    control: ObserveMode,
    /// Mode of the 50 Hz properties.
    #[arg(long, default_value = "change:1ms", value_parser = args::mode)]
    telemetry: ObserveMode,
    /// Observe only these properties (`i_q`, `foc.i_q`), each with its level's mode or its own (`i_q=every:40us`).
    #[arg(short, long, value_delimiter = ',', add = ArgValueCandidates::new(prop_candidates))]
    prop: Vec<String>,
    /// Bytes per second each level may take on the link (real time); the device's default without it.
    #[arg(long)]
    budget: Option<u32>,
    /// Stop after this long (real time), e.g. 10s; runs until Ctrl-C without it.
    #[arg(long, value_parser = args::duration)]
    duration: Option<Duration>,
    /// Print the levels this often (real time).
    #[arg(long, default_value = "1s", value_parser = args::duration)]
    every: Duration,
    /// Also print every observed property each time, not only at the end.
    #[arg(short, long)]
    verbose: bool,
    /// Compare with one StreamData event per property and value, as `stream_data_ser()` sends them.
    #[arg(long)]
    compare: bool,
    #[command(flatten)]
    sim: SimArgs,
    /// Control mode to set (turns the profile off).
    #[arg(long, value_enum)]
    mode: Option<ModeArg>,
    /// Speed setpoint, electrical rpm (with --mode speed).
    #[arg(long, allow_hyphen_values = true)]
    erpm: Option<f32>,
    /// q current setpoint, A (with --mode current).
    #[arg(long, allow_hyphen_values = true)]
    current: Option<f32>,
    /// Duty setpoint, -0.95..=0.95 (with --mode duty).
    #[arg(long, allow_hyphen_values = true)]
    duty: Option<f32>,
}

#[derive(Clone, Copy, ValueEnum)]
enum MotorArg {
    /// VESC's default hobby outrunner, 3 cells
    Vesc,
    /// 48 V industrial servo
    Spm,
    /// 48 V interior magnet motor
    Ipm,
}

#[derive(Clone, Copy, ValueEnum)]
enum ProfileArg {
    Off,
    /// 2000, 8000, -4000, 12000 erpm, half a second each
    Steps,
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Off,
    Duty,
    Current,
    Speed,
}

/// Property names with level, unit and rate tier, for shell completion.
fn prop_candidates() -> Vec<CompletionCandidate> {
    PROPS
        .iter()
        .map(|p| {
            let help = format!("{} {} Hz {}", p.level.name(), p.level.tier_hz(), p.unit);
            CompletionCandidate::new(p.name).help(Some(help.trim().to_string().into()))
        })
        .collect()
}

fn main() -> anyhow::Result<()> {
    // answers shell completion requests (COMPLETE=<shell> set) and exits, otherwise does nothing
    CompleteEnv::with_factory(Cli::command).complete();
    let cli = Cli::parse();
    if std::env::var_os("NO_COLOR").is_some() {
        console::set_colors_enabled(false);
    }
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::WARN)
        .with_writer(std::io::stderr)
        .init();
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(async {
            match cli.command {
                Command::Device(a) => run_device(a).await,
                Command::Observe(a) => run_observe(a).await,
                Command::List => {
                    list();
                    Ok(())
                }
            }
        })
}

fn list() {
    for level in Level::ALL {
        println!("{} ({} Hz)", style(level.name()).bold(), level.tier_hz());
        for p in level.props() {
            println!("  {:<22} {}", p.name, style(p.unit).dim());
        }
    }
}

async fn run_device(a: DeviceArgs) -> anyhow::Result<()> {
    let mut opts = Options {
        seed: a.seed,
        ..Options::default()
    };
    if let Some(s) = a.sim.time_scale {
        opts.time_scale = s;
    }
    if let Some(m) = a.sim.motor {
        opts.preset = preset(m);
    }
    if let Some(l) = a.sim.load {
        opts.load = l;
    }
    if let Some(p) = a.sim.profile {
        opts.profile = profile(p);
    }
    let dev = device::spawn(Medium::Udp(a.listen), opts)?;
    println!(
        "{} listening on {}, connect with: motor_demo observe --udp {}",
        style("motor_demo device").bold(),
        dev.udp_addr.unwrap(),
        dev.udp_addr.unwrap()
    );
    tokio::task::spawn_blocking(move || dev.join()).await?
}

fn preset(m: MotorArg) -> Preset {
    match m {
        MotorArg::Vesc => Preset::VescOutrunner,
        MotorArg::Spm => Preset::IndustrialSpm,
        MotorArg::Ipm => Preset::Ipm,
    }
}

fn profile(p: ProfileArg) -> Profile {
    match p {
        ProfileArg::Off => Profile::Off,
        ProfileArg::Steps => Profile::SpeedSteps,
    }
}

/// What to observe per level, from the level modes and `--prop`.
fn requests(a: &ObserveArgs, time_scale: f32) -> anyhow::Result<Vec<LevelRequest>> {
    let level_mode = |l: Level| match l {
        Level::Foc => a.foc,
        Level::Control => a.control,
        Level::Telemetry => a.telemetry,
    };
    let mut modes: BTreeMap<Level, BTreeMap<u32, ObserveMode>> = BTreeMap::new();
    if a.prop.is_empty() {
        for p in PROPS {
            modes
                .entry(p.level)
                .or_default()
                .insert(p.index, level_mode(p.level));
        }
    } else {
        for spec in &a.prop {
            let (name, mode) = match spec.split_once('=') {
                Some((n, m)) => (n, Some(args::mode(m)?)),
                None => (spec.as_str(), None),
            };
            let p = find(name).with_context(|| format!("no property {name} (motor_demo list)"))?;
            let mode = mode.unwrap_or(level_mode(p.level));
            modes.entry(p.level).or_default().insert(p.index, mode);
        }
    }
    Ok(modes
        .into_iter()
        .map(|(level, mut modes)| {
            modes.retain(|_, m| !matches!(m, ObserveMode::Off));
            LevelRequest {
                level,
                modes,
                // the device counts bytes per second of its own, simulated, time
                budget: a.budget.map(|b| (b as f64 / time_scale as f64) as u32),
            }
        })
        .collect())
}

async fn run_observe(a: ObserveArgs) -> anyhow::Result<()> {
    let (_dev, client) = match a.udp {
        Some(addr) => {
            let client = MotorDemo::config(|c| c.udp_addr(addr.to_string()))
                .connect()
                .await
                .with_context(|| format!("connect to {addr}"))?;
            (None, client)
        }
        None => {
            let path = format!("motor_demo/{}", std::process::id());
            let dev = device::spawn(Medium::InProcess(path.clone()), Options::default())?;
            let client = MotorDemo::config(|c| c.in_process_path(path.clone()))
                .connect()
                .await?;
            (Some(dev), client)
        }
    };
    setup(&client, &a).await?;
    let time_scale = client.sim().read_time_scale().read().await?;
    let session = observe(&client, &requests(&a, time_scale)?).await?;
    print_granted(&session, time_scale);

    let start = Instant::now();
    let mut total = Stats::new();
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + a.every, a.every);
    loop {
        let stop = tokio::select! {
            _ = tick.tick() => false,
            _ = tokio::signal::ctrl_c() => true,
        };
        let stats = session.take();
        print_levels(&stats, &session, time_scale, a.compare, start.elapsed());
        if a.verbose {
            print_props(&stats, &session, time_scale);
        }
        merge(&mut total, stats);
        if stop || a.duration.is_some_and(|d| start.elapsed() >= d) {
            break;
        }
    }
    total.since = start;
    println!("\n{}", style("Total").bold());
    print_levels(&total, &session, time_scale, a.compare, start.elapsed());
    print_props(&total, &session, time_scale);
    Ok(())
}

/// Write the sim settings and setpoints given on the command line.
async fn setup(client: &MotorDemo, a: &ObserveArgs) -> anyhow::Result<()> {
    let sim = client.sim();
    if let Some(s) = a.sim.time_scale {
        sim.write_time_scale(s).write().await?;
    }
    if let Some(m) = a.sim.motor {
        sim.write_motor(preset(m)).write().await?;
    }
    if let Some(l) = a.sim.load {
        sim.write_load(l).write().await?;
    }
    if let Some(p) = a.sim.profile {
        sim.write_profile(profile(p)).write().await?;
    }
    let control = client.control();
    if let Some(v) = a.erpm {
        control.write_erpm_target(v).write().await?;
    }
    if let Some(v) = a.current {
        control.write_current_target(v).write().await?;
    }
    if let Some(v) = a.duty {
        control.write_duty_target(v).write().await?;
    }
    if let Some(m) = a.mode {
        let m = match m {
            ModeArg::Off => ControlMode::Off,
            ModeArg::Duty => ControlMode::Duty,
            ModeArg::Current => ControlMode::Current,
            ModeArg::Speed => ControlMode::Speed,
        };
        control.write_mode(m).write().await?;
    }
    Ok(())
}

fn merge(total: &mut Stats, s: Stats) {
    for (level, l) in s.levels {
        let t = total.levels.entry(level).or_default();
        t.updates += l.updates;
        t.bytes += l.bytes;
        t.missed += l.missed;
        if t.first_t.is_none() {
            t.first_t = l.first_t;
        }
        t.last_t = l.last_t.or(t.last_t);
        for (k, p) in l.props {
            let tp = t.props.entry(k).or_default();
            tp.count += p.count;
            tp.stream_data_bytes += p.stream_data_bytes;
            tp.last = p.last;
        }
    }
}

fn print_granted(session: &Session, time_scale: f32) {
    println!(
        "{} sim time ×{time_scale}; periods and budgets below are device (simulated) time, rates real time",
        style("Observing").bold()
    );
    for (level, g) in &session.granted {
        let mut by_mode: BTreeMap<String, usize> = BTreeMap::new();
        for m in g.modes.values() {
            *by_mode.entry(mode_str(m)).or_default() += 1;
        }
        let modes: Vec<_> = by_mode.iter().map(|(m, n)| format!("{n}× {m}")).collect();
        let refused = if g.refused > 0 {
            style(format!(", {} refused", g.refused)).red().to_string()
        } else {
            String::new()
        };
        println!(
            "  {:<10} {}, budget {}B/s{refused}",
            level.name(),
            modes.join(", "),
            si(g.budget as f64),
        );
    }
}

fn print_levels(
    stats: &Stats,
    session: &Session,
    time_scale: f32,
    compare: bool,
    elapsed: Duration,
) {
    let secs = stats.since.elapsed().as_secs_f64().max(1e-3);
    let mut header = format!(
        "{:>7}  {:<10} {:>9} {:>9} {:>11} {:>5} {:>9}",
        "t", "level", "updates/s", "values/s", "bytes/s", "use", "missed/s"
    );
    if compare {
        header += &format!(" {:>13} {:>7}", "StreamData/s", "saving");
    }
    println!("{}", style(header).dim());
    for (level, g) in &session.granted {
        let Some(l) = stats.levels.get(level) else {
            continue;
        };
        let bytes = l.bytes as f64 / secs;
        // the granted budget in real time
        let budget = g.budget as f64 * time_scale as f64;
        let used = bytes / budget.max(1.0) * 100.0;
        let used = if used > 95.0 {
            style(format!("{used:>4.0}%")).yellow()
        } else {
            style(format!("{used:>4.0}%")).green()
        };
        let missed = l.missed as f64 / secs;
        let missed = if missed > 0.0 {
            style(format!("{missed:>9.0}")).red()
        } else {
            style(format!("{missed:>9.0}"))
        };
        let mut line = format!(
            "{:>6.1}s  {:<10} {:>9.0} {:>9.0} {:>9}B/s {} {}",
            elapsed.as_secs_f64(),
            level.name(),
            l.updates as f64 / secs,
            l.values() as f64 / secs,
            si(bytes),
            used,
            missed,
        );
        if compare {
            let sd = l.stream_data_bytes() as f64 / secs;
            let saving = 100.0 * (1.0 - l.bytes as f64 / (l.stream_data_bytes() as f64).max(1.0));
            line += &format!(" {:>11}B/s {saving:>6.0}%", si(sd));
        }
        println!("{line}");
    }
}

fn print_props(stats: &Stats, session: &Session, time_scale: f32) {
    let secs = stats.since.elapsed().as_secs_f64().max(1e-3);
    println!(
        "{}",
        style(format!(
            "  {:<30} {:>14} {:>9} {:>11}  {}",
            "property", "mode", "rate Hz", "device Hz", "last value"
        ))
        .dim()
    );
    for (level, g) in &session.granted {
        let Some(l) = stats.levels.get(level) else {
            continue;
        };
        for p in level.props() {
            let Some(mode) = g.modes.get(&p.index) else {
                continue;
            };
            let s = l.props.get(&p.index).cloned().unwrap_or_default();
            let rate = s.count as f64 / secs;
            println!(
                "  {:<30} {:>14} {:>9.1} {:>11.1}  {} {}",
                p.path(),
                mode_str(mode),
                rate,
                rate / time_scale as f64,
                s.last,
                style(p.unit).dim()
            );
        }
    }
}
