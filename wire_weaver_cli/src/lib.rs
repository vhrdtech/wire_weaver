use crate::cli::{Cli, Commands};
use crate::cmd::config::ConfigCommand;
use crate::cmd::dfu::{DeviceDfu, DfuCommand};
use crate::cmd::hub::HubCommand;
use crate::cmd::tap::{Attached, Layer, LiveTap};
use crate::device::Selection;
use anyhow::{Context, Result, anyhow};
use clap::FromArgMatches;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use wire_weaver_client::{ClientMeta, DynClient};

mod api_tree;
mod cli;
pub(crate) mod cmd;
pub mod complete;
pub mod device;
mod dfu_client;
pub mod plugin;
pub mod untrusted;
mod util;
mod value_fmt;

/// The `ww` command line: what the `ww` binary runs.
pub fn main() -> Result<()> {
    // started by a client as the user's hub (WW_HUB_SERVE set): serves until idle and exits, otherwise does nothing
    ww_hub::main_if_hub();
    // answers shell completion requests (COMPLETE=<shell> set) and exits, otherwise does nothing
    complete::complete(|| complete::with_plugins(Cli::command_fitted));
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<()> {
    let matches = Cli::command_fitted().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    // keep connection progress logs out of the printed tree
    let max_level: tracing::level_filters::LevelFilter = match cli.command {
        // the hub closes the connection on us by design
        Commands::Hub(HubCommand::Stop) => tracing::level_filters::LevelFilter::OFF,
        Commands::Introspect(_)
        | Commands::Explain(_)
        | Commands::Observe(_)
        | Commands::Stream(_)
        | Commands::Monitor(_)
        | Commands::Stats(_)
        | Commands::Capture(_)
        | Commands::Pins(_)
        | Commands::Sync(_)
        // the hub's own log is what `serve` is for; the others just talk to it (`list` may, too)
        | Commands::List(_)
        | Commands::Hub(HubCommand::Status | HubCommand::Who { .. } | HubCommand::Keep { .. }) => {
            tracing::Level::WARN.into()
        }
        _ => tracing::Level::INFO.into(),
    };
    // probe-rs says what it does with the probe (`Halting core 0...`) at INFO, `--rtt` shows its warnings only
    let probe_rs_level = max_level.min(tracing::Level::WARN.into());
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::filter::Targets::new()
                .with_default(max_level)
                .with_target("probe_rs", probe_rs_level),
        )
        .init();
    cli.check_tap()?;
    if let Commands::Explain(args) = &cli.command {
        args.check()?;
    }
    let selection = match &cli.command {
        Commands::List(_)
        | Commands::Sync(_)
        | Commands::Config(_)
        | Commands::External(_)
        | Commands::Dfu(
            DfuCommand::Rp(_) | DfuCommand::Stm32(_) | DfuCommand::Device(DeviceDfu::Flash { .. }),
        ) => Some(Selection::resolve(
            &matches,
            matches!(cli.command, Commands::Config(ConfigCommand::Save)),
        )?),
        _ if cli.need_device() => Some(Selection::resolve(&matches, false)?),
        _ => None,
    };
    // the tap commands record the connection from its start: the recorder is subscribed to before connecting
    let tap = match &cli.command {
        Commands::Monitor(args) => Some(LiveTap::new(args.layer())),
        Commands::Stats(args) if args.file.is_none() => Some(LiveTap::new(Layer::Frame)),
        Commands::Capture(args) => Some(LiveTap::new(args.layer())),
        _ => None,
    };
    // `--monitor` / `--capture` of a command that connects: its own tap, from the start of the connection
    let flag_tap = cli
        .tap
        .active()
        .then(|| Attached::start(&cli.tap))
        .transpose()?;
    let recorder = match (&tap, &flag_tap) {
        (Some(tap), _) => Some(tap.recorder.clone()),
        (None, Some(attached)) => Some(attached.recorder()),
        _ => None,
    };
    // a tapped run ends cleanly on SIGINT / SIGTERM: the capture file is finished and renamed, not left `.partial`
    let stop = (tap.is_some() || flag_tap.is_some()).then(util::StopSignal::install);
    // who this program is to the device and a hub: one client instance for all its connections, so a claim it takes
    // in the hub (`ww dfu`, SRV-32) lets its own calls through
    let meta = ClientMeta::new();
    let device = match &selection {
        Some(selection) if cli.need_device() => {
            let config = if cli.traits_only() {
                selection.client_config().traits_only()
            } else {
                selection.client_config()
            };
            let config = config.client_meta(meta.clone());
            let config = match recorder {
                Some(recorder) => config.tap(recorder),
                None => config,
            };
            let connect = async {
                DynClient::from_config(config)
                    .connect()
                    .await
                    .with_context(|| {
                        format!(
                            "connecting to device ({selection}), run 'ww list{}' to see connected {}",
                            if selection.rtt().is_some() { " --probes" } else { "" },
                            if selection.rtt().is_some() { "debug probes" } else { "devices" },
                        )
                    })
            };
            Some(
                util::guard(stop.clone(), connect)
                    .await
                    .unwrap_or_else(|| Err(anyhow!("stopped while connecting"))),
            )
        }
        _ => None,
    };
    // over RTT the device behind the probe is only known now
    let device = match (device, &selection) {
        (Some(Ok(device)), Some(selection)) => Some(
            selection
                .check_connected(&device.device_api_info().user_api_version)
                .map(|_| device),
        ),
        (device, _) => device,
    };
    let mut device = match device {
        Some(Ok(device)) => Some(device),
        Some(Err(e)) => {
            // the tap saw the failed connect too: let it print and close its file
            if let Some(attached) = flag_tap
                && let Err(tap_err) = attached.finish()
            {
                eprintln!("tap: {tap_err:#}");
            }
            return Err(e);
        }
        None => None,
    };
    if let (Some(attached), Some(device)) = (&flag_tap, &device) {
        attached.add_bundles(device);
    }

    // a flagged command that is stopped (`observe` runs until it is) ends as if it was done: the tap is finished below
    let command_result = util::guard(
        flag_tap.is_some().then(|| stop.clone()).flatten(),
        run_command(
            cli.command,
            &selection,
            matches.get_flag("no_config"),
            &mut device,
            &meta,
            tap,
            stop,
        ),
    )
    .await
    .unwrap_or(Ok(Ran {
        disconnect: true,
        passed: true,
    }));
    if let Some(attached) = flag_tap {
        // the recording ends with the disconnect
        if let Some(device) = device.take() {
            _ = device.disconnect().asynch().await;
        }
        let tap_result = attached.finish();
        match (&command_result, tap_result) {
            (Ok(_), Err(e)) => return Err(e),
            (Err(_), Err(e)) => eprintln!("tap: {e:#}"),
            _ => {}
        }
    }
    let ran = command_result?;
    if ran.disconnect
        && let Some(device) = device
    {
        device.disconnect().asynch().await?;
    }
    if !ran.passed {
        // a test that found a fault, after disconnecting
        std::process::exit(1);
    }
    Ok(())
}

/// How a command ended.
struct Ran {
    /// `false` when the device is gone or about to be (nothing to disconnect from).
    disconnect: bool,
    /// `false` when a test found a fault (`ww pins test`): exit code 1.
    passed: bool,
}

/// Runs the command.
async fn run_command(
    command: Commands,
    selection: &Option<Selection>,
    no_config: bool,
    device: &mut Option<DynClient>,
    meta: &ClientMeta,
    tap: Option<LiveTap>,
    stop: Option<util::StopSignal>,
) -> Result<Ran> {
    let mut passed = true;
    match command {
        Commands::List(args) => cmd::list::list(args, selection.as_ref().unwrap()).await?,
        Commands::Sync(args) => {
            let stop = stop.unwrap_or_else(util::StopSignal::install);
            cmd::sync::sync(args, selection.as_ref().unwrap(), stop).await?
        }
        Commands::USBLoopback {
            duration_sec,
            packet_size,
            tuning,
        } => {
            cmd::usb_loopback::usb_loopback(
                device.as_mut().unwrap(),
                duration_sec,
                packet_size,
                tuning.params(),
            )
            .await?
        }
        Commands::Api(api_cmd) => cmd::api::api(api_cmd)?,
        Commands::Introspect(args) => {
            cmd::introspect::introspect(args, device.as_mut().unwrap()).await?
        }
        Commands::Explain(args) => cmd::explain::explain_cmd(args, device.as_ref())?,
        Commands::Observe(args) => cmd::observe::observe(args, device.as_ref().unwrap()).await?,
        Commands::Stream(args) => cmd::stream::stream(args, device.as_ref().unwrap()).await?,
        // these disconnect themselves, so the recording ends with the disconnect
        Commands::Monitor(args) => {
            let (device, tap) = (device.take().unwrap(), tap.unwrap());
            cmd::tap::monitor(args, device, tap, stop.unwrap()).await?
        }
        Commands::Stats(args) => match &args.file {
            Some(_) => cmd::tap::stats_file(&args)?,
            None => {
                let (device, tap) = (device.take().unwrap(), tap.unwrap());
                cmd::tap::stats(args, device, tap, stop.unwrap()).await?
            }
        },
        Commands::Capture(args) => {
            let (device, tap) = (device.take().unwrap(), tap.unwrap());
            cmd::tap::capture(args, device, tap, stop.unwrap()).await?
        }
        Commands::Dfu(DfuCommand::Rp(rp_cmd)) => {
            cmd::bootsel::bootsel(rp_cmd, selection.as_ref().unwrap()).await?
        }
        Commands::Dfu(DfuCommand::Stm32(stm32_cmd)) => {
            cmd::stm32dfu::stm32dfu(stm32_cmd, selection.as_ref().unwrap()).await?
        }
        Commands::Dfu(DfuCommand::Device(DeviceDfu::Flash { args })) => {
            let mut plugin_args = vec!["flash".into()];
            plugin_args.extend(args);
            match run_plugin("chunky", plugin_args, selection, no_config)? {}
        }
        Commands::External(mut args) => {
            let name = args.remove(0).to_string_lossy().into_owned();
            match run_plugin(&name, args, selection, no_config)? {}
        }
        Commands::Dfu(DfuCommand::Device(dfu_cmd)) => {
            let resets_device = dfu_cmd.resets_device();
            cmd::dfu::dfu(
                dfu_cmd,
                device.take().unwrap(),
                selection.as_ref().unwrap(),
                meta,
            )
            .await?;
            if resets_device {
                // the device is gone or about to, nothing to disconnect from
                return Ok(Ran {
                    disconnect: false,
                    passed,
                });
            }
        }
        Commands::Counters(mut args) => {
            args.elf = selection.as_ref().and_then(Selection::elf_flag);
            cmd::counters::counters(args, device.as_ref().unwrap()).await?
        }
        Commands::Info(args) => cmd::info::info(args, device.as_ref().unwrap()).await?,
        Commands::Report(mut args) => {
            args.elf = selection.as_ref().and_then(Selection::elf_flag);
            cmd::info::report(args, device.as_ref().unwrap()).await?
        }
        Commands::Pins(pins_cmd) => {
            passed = cmd::pins::pins(pins_cmd, device.as_ref().unwrap()).await?
        }
        Commands::Elf(elf_cmd) => cmd::elf::elf(elf_cmd)?,
        Commands::Config(config_cmd) => {
            cmd::config::config(config_cmd, selection.as_ref().unwrap())?
        }
        Commands::Settings(settings_cmd) => cmd::settings::settings(settings_cmd)?,
        Commands::Hub(hub_cmd) => cmd::hub::hub(hub_cmd).await?,

        #[cfg(target_os = "linux")]
        Commands::Udev => {
            todo!()
        }
    }

    Ok(Ran {
        disconnect: true,
        passed,
    })
}

/// `ww NAME ARGS..` -> `ww-NAME ARGS..`, with the device selection of this command line in its environment.
fn run_plugin(
    name: &str,
    args: Vec<std::ffi::OsString>,
    selection: &Option<Selection>,
    no_config: bool,
) -> Result<std::convert::Infallible> {
    let env = plugin::selection_env(selection.as_ref().unwrap(), no_config);
    plugin::run(name, &args, &env, || match name {
        "chunky" => anyhow!(
            "`ww dfu flash` runs the ww-chunky plugin, which is not on PATH: install it with `just install` in \
             chunky_mcu_updater (or `cargo install --git https://github.com/romixlab/chunky_mcu_updater ww_chunky --locked`)"
        ),
        _ => anyhow!(
            "no such command: `ww {name}`, and no `ww-{name}` on PATH (see 'Plugins' in the CLI reference)"
        ),
    })
}
