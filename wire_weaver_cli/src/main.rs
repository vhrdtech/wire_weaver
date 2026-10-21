use crate::cli::{Cli, Commands};
use crate::cmd::config::ConfigCommand;
use crate::cmd::tap::{Attached, Layer, LiveTap};
use crate::device::Selection;
use anyhow::{Context, Result, anyhow};
use clap::{CommandFactory, FromArgMatches};
use wire_weaver_client::DynClient;

mod api_tree;
mod cli;
pub(crate) mod cmd;
mod complete;
mod device;
mod util;
mod value_fmt;

fn main() -> Result<()> {
    // answers shell completion requests (COMPLETE=<shell> set) and exits, otherwise does nothing
    complete::complete(Cli::command);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<()> {
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    // keep connection progress logs out of the printed tree
    let max_level = match cli.command {
        Commands::Introspect(_)
        | Commands::Explain(_)
        | Commands::Observe(_)
        | Commands::Stream(_)
        | Commands::Monitor(_)
        | Commands::Stats(_)
        | Commands::Capture(_)
        | Commands::Pins(_) => tracing::Level::WARN,
        _ => tracing::Level::INFO,
    };
    tracing_subscriber::fmt().with_max_level(max_level).init();
    cli.check_tap()?;
    if let Commands::Explain(args) = &cli.command {
        args.check()?;
    }
    let selection = match &cli.command {
        Commands::List(_) | Commands::Config(_) | Commands::Bootsel(_) | Commands::Stm32dfu(_) => {
            Some(Selection::resolve(
                &matches,
                matches!(cli.command, Commands::Config(ConfigCommand::Save)),
            )?)
        }
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
    let device = match &selection {
        Some(selection) if cli.need_device() => {
            let config = if cli.traits_only() {
                selection.client_config().traits_only()
            } else {
                selection.client_config()
            };
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
                            "connecting to device ({selection}), run 'ww list' to see connected devices"
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
        run_command(cli.command, &selection, &mut device, tap, stop),
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
    device: &mut Option<DynClient>,
    tap: Option<LiveTap>,
    stop: Option<util::StopSignal>,
) -> Result<Ran> {
    let mut passed = true;
    match command {
        Commands::List(args) => cmd::list::list(args, selection.as_ref().unwrap()).await?,
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
        Commands::Dfu(dfu_cmd) => {
            let resets_device = dfu_cmd.resets_device();
            cmd::dfu::dfu(dfu_cmd, device.take().unwrap(), selection.as_ref().unwrap()).await?;
            if resets_device {
                // the device is gone or about to, nothing to disconnect from
                return Ok(Ran {
                    disconnect: false,
                    passed,
                });
            }
        }
        Commands::Counters(args) => cmd::counters::counters(args, device.as_ref().unwrap()).await?,
        Commands::Info(args) => cmd::info::info(args, device.as_ref().unwrap()).await?,
        Commands::Report(args) => cmd::info::report(args, device.as_ref().unwrap()).await?,
        Commands::Pins(pins_cmd) => {
            passed = cmd::pins::pins(pins_cmd, device.as_ref().unwrap()).await?
        }
        Commands::Elf(elf_cmd) => cmd::elf::elf(elf_cmd)?,
        Commands::Config(config_cmd) => {
            cmd::config::config(config_cmd, selection.as_ref().unwrap())?
        }
        Commands::Settings(settings_cmd) => cmd::settings::settings(settings_cmd)?,
        Commands::Bootsel(bootsel_cmd) => {
            cmd::bootsel::bootsel(bootsel_cmd, selection.as_ref().unwrap()).await?
        }
        Commands::Stm32dfu(dfu_cmd) => {
            cmd::stm32dfu::stm32dfu(dfu_cmd, selection.as_ref().unwrap()).await?
        }

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
