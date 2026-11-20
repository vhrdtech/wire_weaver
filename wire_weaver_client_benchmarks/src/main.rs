//! `connect_bench`: times the steps of a connect (`wire_weaver_client::connect_spans`) to blinky devices, round after
//! round, and prints a table per round.
//!
//! Without `--here` it runs itself twice, each a new process: first with an empty cache (`WIRE_WEAVER_HOME` in a new
//! temporary directory), then with this machine's own. In each, round 1 is the process's first connect and later
//! rounds are warm.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use blinky::Blinky;
use clap::{Parser, ValueEnum};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use wire_weaver_client::{ClientConfig, DynClient};
use wire_weaver_client_benchmarks::{StepTimes, sim, table};

/// Times the steps of a connect to blinky devices, in-process or on USB, cold and warm.
#[derive(Parser, Clone)]
#[command(
    name = "connect_bench",
    version,
    after_help = "Examples:\n  connect_bench --clients 50 --calls 20\n  connect_bench --transport usb --client blocking"
)]
struct Args {
    /// Where the devices are
    #[arg(long, value_enum, default_value_t = Transport::InProcess)]
    transport: Transport,
    /// Devices and clients, connecting all at once (in-process only; USB has one device)
    #[arg(long, default_value_t = 1)]
    clients: usize,
    /// Rounds of connect, call, disconnect
    #[arg(long, default_value_t = 5)]
    rounds: usize,
    /// Calls each generated client makes after it connected, one after another
    #[arg(long, default_value_t = 1)]
    calls: usize,
    /// The client that connects
    #[arg(long, value_enum, default_value_t = Client::Async)]
    client: Client,
    /// USB packet size of the in-process devices (512: high speed, 64: full speed)
    #[arg(long, default_value_t = sim::HIGH_SPEED_PACKET_BYTES)]
    packet_bytes: usize,
    /// USB serial number of the device, when more than one blinky is plugged in
    #[arg(long)]
    serial: Option<String>,
    /// The resolver the clients find snapshots through
    #[arg(long, value_enum, default_value_t = ResolverKind::Host)]
    resolver: ResolverKind,
    /// Tokio worker threads of the client's runtime
    #[arg(long, default_value_t = 4)]
    worker_threads: usize,
    /// Run in this process, with the cache the environment names, instead of an empty-cache and a home-cache process
    #[arg(long)]
    here: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Transport {
    /// Blinky devices on threads of this process, over simulated USB
    InProcess,
    /// The blinky firmware on a USB device (`0xc0de:0xcafe`)
    Usb,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum ResolverKind {
    /// The process's host resolver, as a program connects by default
    Host,
    /// The offline resolver over this workspace's `Cargo.lock` that tests connect through (`tests_common::resolver`)
    Offline,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Client {
    /// The generated blinky client, `connect().await`
    Async,
    /// The generated blinky client, `connect_blocking()` on a thread per client
    Blocking,
    /// `DynClient`, which knows no API and takes the device's introspection data (from the cache or the device)
    Dynamic,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let result = if args.here {
        run_here(&args)
    } else {
        run_both()
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("connect_bench: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the bench in a new process with an empty cache, then in one with this machine's.
fn run_both() -> Result<()> {
    let empty = std::env::temp_dir().join(format!("connect_bench_{}", std::process::id()));
    std::fs::create_dir_all(&empty).with_context(|| format!("creating {}", empty.display()))?;
    let cold = run_child(Some(&empty));
    std::fs::remove_dir_all(&empty).with_context(|| format!("removing {}", empty.display()))?;
    cold?;
    run_child(None)
}

/// Runs this binary with `--here` and the same arguments; `home` replaces `WIRE_WEAVER_HOME`.
fn run_child(home: Option<&PathBuf>) -> Result<()> {
    let exe = std::env::current_exe().context("finding this binary")?;
    let mut child = std::process::Command::new(exe);
    child.args(std::env::args_os().skip(1)).arg("--here");
    match home {
        Some(home) => {
            println!("== new process, empty cache ({})", home.display());
            child.env("WIRE_WEAVER_HOME", home);
        }
        None => println!("== new process, this machine's cache"),
    }
    let status = child.status().context("running the bench")?;
    if !status.success() {
        bail!("the bench process failed: {status}");
    }
    Ok(())
}

fn run_here(args: &Args) -> Result<()> {
    if args.transport == Transport::Usb && args.clients != 1 {
        bail!("one USB device: --clients 1");
    }
    let steps = StepTimes::default();
    tracing_subscriber::registry()
        .with(steps.clone())
        .with(tracing_subscriber::fmt::layer().with_filter(LevelFilter::WARN))
        .init();
    let started = Instant::now();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(args.worker_threads)
        .enable_all()
        .build()?;
    let _devices: Vec<_> = match args.transport {
        Transport::InProcess => (0..args.clients)
            .map(|n| sim::start(&path(n), args.packet_bytes))
            .collect(),
        Transport::Usb => vec![],
    };
    println!(
        "{} client, {}, {} client(s), {} worker threads; runtime and devices up in {:.1} ms",
        client_name(args.client),
        match args.transport {
            Transport::InProcess => format!("in-process USB, {} B packets", args.packet_bytes),
            Transport::Usb => "USB".into(),
        },
        args.clients,
        args.worker_threads,
        started.elapsed().as_secs_f64() * 1e3
    );
    for round in 1..=args.rounds {
        let round_start = Instant::now();
        let wall = round_trip(args, &rt)?;
        let all = round_start.elapsed();
        let times = steps.take();
        println!(
            "round {round}: {} connect(s) in {:.2} ms wall, with {} call(s) each and the disconnect {:.2} ms\n{}",
            args.clients,
            wall.as_secs_f64() * 1e3,
            args.calls,
            all.as_secs_f64() * 1e3,
            table(&times)
        );
    }
    Ok(())
}

fn client_name(client: Client) -> &'static str {
    match client {
        Client::Async => "async",
        Client::Blocking => "blocking",
        Client::Dynamic => "dynamic",
    }
}

fn path(n: usize) -> String {
    format!("connect_bench/{n}")
}

/// The config of client `n`.
fn config(args: &Args, n: usize, c: ClientConfig) -> ClientConfig {
    let c = match args.resolver {
        ResolverKind::Host => c,
        ResolverKind::Offline => c.resolver(tests_common::resolver()),
    };
    match args.transport {
        Transport::InProcess => c.no_ww_usb().in_process_path(path(n)),
        Transport::Usb => match &args.serial {
            Some(serial) => c.usb_vid_pid(0xc0de, 0xcafe).serial_eq(serial.clone()),
            None => c.usb_vid_pid(0xc0de, 0xcafe),
        },
    }
}

/// Every client connects at once, makes its calls and disconnects; the time until the last one connected.
fn round_trip(args: &Args, rt: &tokio::runtime::Runtime) -> Result<Duration> {
    let start = Instant::now();
    match args.client {
        Client::Async | Client::Dynamic => rt.block_on(async {
            let tasks: Vec<_> = (0..args.clients)
                .map(|n| tokio::spawn(connect_async(args.clone(), n)))
                .collect();
            let mut wall = Duration::ZERO;
            for task in tasks {
                wall = wall.max(task.await??);
            }
            Ok(wall)
        }),
        Client::Blocking => std::thread::scope(|s| {
            let threads: Vec<_> = (0..args.clients)
                .map(|n| {
                    let handle = rt.handle().clone();
                    s.spawn(move || {
                        let _rt = handle.enter();
                        let client = Blinky::config(|c| config(args, n, c)).connect_blocking()?;
                        let connected = start.elapsed();
                        for _ in 0..args.calls {
                            client.led_on().blocking_call()?;
                        }
                        client.disconnect().blocking()?;
                        anyhow::Ok(connected)
                    })
                })
                .collect();
            let mut wall = Duration::ZERO;
            for thread in threads {
                let connected = thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("a client thread panicked"))??;
                wall = wall.max(connected);
            }
            Ok(wall)
        }),
    }
}

/// Client `n` connects, makes one call (the generated client) and disconnects; the time it took to connect.
async fn connect_async(args: Args, n: usize) -> Result<Duration> {
    let start = Instant::now();
    if args.client == Client::Dynamic {
        let client = DynClient::from_config(config(&args, n, ClientConfig::new()))
            .connect()
            .await?;
        let connected = start.elapsed();
        client.disconnect().asynch().await?;
        return Ok(connected);
    }
    let client = Blinky::config(|c| config(&args, n, c)).connect().await?;
    let connected = start.elapsed();
    for _ in 0..args.calls {
        client.led_on().call().await?;
    }
    client.disconnect().asynch().await?;
    Ok(connected)
}
