//! `ww counters`: the device's `cnt` counters through `ww_counters::Counters` (STD-25), decoded with the firmware's
//! ELF found by its build id and shown with the same views as `cnt` over a probe (`cnt_view`).

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand, ValueHint};
use cnt_core::{Counters, Storage, elf};
use cnt_view::{Format, output};
use console::style;
use std::path::PathBuf;
use std::time::Duration;
use wire_weaver_client::{CallError, Commander, DynClient};
use ww_counters::CounterBuffer;
use ww_counters::host::{CountersDevice, WwSource};

#[derive(Args)]
pub(crate) struct CountersArgs {
    #[command(subcommand)]
    command: Option<CountersCommand>,

    /// Firmware ELF to decode the counters with. By default the one with the build id the device reports, from
    /// target/ of the cargo project here, WW_ELF_PATH or ~/.cache/ww/elf (`cnt elf add` fills it)
    #[arg(long, global = true, value_hint = ValueHint::FilePath)]
    elf: Option<PathBuf>,

    /// Use the ELF although the device runs other firmware (its build id differs), or the newest one under target/
    /// when none matches
    #[arg(long, global = true)]
    force: bool,

    /// Output format of `read`, `watch` (text or jsonl) and `reset`. JSON goes to stdout, diagnostics to stderr
    #[arg(long, global = true, value_enum, default_value_t)]
    format: Format,
}

#[derive(Subcommand, Default)]
pub(crate) enum CountersCommand {
    /// Read all counters once (default)
    #[default]
    Read,
    /// Print counters whose value changed, until q or Ctrl+C: all of them first, then changes (`reset` when all
    /// non-zero counters of a buffer went down, `decrease` when only some did)
    Watch {
        /// Time between reads, in milliseconds
        #[arg(long, default_value_t = 100)]
        interval: u64,
    },
    /// Terminal UI: a table per buffer, values highlighted as they change, r / R reset RAM / backup counters
    Tui,
    /// Reset counters to zero
    Reset {
        /// Reset the backup counters as well, if the device has them
        #[arg(long)]
        bkp: bool,
    },
    /// The device's buffers, cnt format and build id, and the ELF that matches it
    Info,
}

pub(crate) async fn counters(args: CountersArgs, device: &DynClient) -> Result<()> {
    let client = CountersClient {
        cmd: device
            .global_attachment(ww_counters::COUNTERS_FULL_GID, "Counters")
            .cmd_tx_take(),
    };
    let info = client.read_INFO().read().await.map_err(|e| {
        anyhow!("{e}").context(
            "the device doesn't implement ww_counters::Counters (or not in a compatible version)",
        )
    })?;
    let command = args.command.unwrap_or_default();
    let resolved = elf::resolve(args.elf.as_deref(), Some(&info.build_id), args.force)?;
    if let Some(warning) = &resolved.warning {
        eprintln!("⚠️ {}", style(warning).yellow());
    }
    eprintln!(
        "{} {}",
        style("Using ELF").dim(),
        style(resolved.path.display()).cyan()
    );
    let elf_path = resolved.path;
    let counters = Counters::load_elf(&elf_path)?;

    if let CountersCommand::Info = command {
        print_info(&info, &counters);
        return Ok(());
    }
    let source = WwSource::new(Device(client), info);
    if let Err(e) = source.check(&counters, &elf_path) {
        if !args.force {
            return Err(e.context("pass the device's ELF, or --force to use this one anyway"));
        }
        eprintln!(
            "⚠️ {}",
            style(format!("{e:#}, used anyway (--force)")).yellow()
        );
    }
    let format = args.format;
    // The views and the calls behind them block, and run until the user quits
    tokio::task::spawn_blocking(move || show(command, format, counters, source, &elf_path))
        .await
        .context("counters view panicked")?
}

fn show(
    command: CountersCommand,
    format: Format,
    mut counters: Counters,
    mut source: WwSource<Device>,
    elf_path: &std::path::Path,
) -> Result<()> {
    match command {
        CountersCommand::Read => {
            counters.read(&mut source)?;
            if format == Format::Text {
                cnt_view::print(&counters);
            } else {
                output::counters(format, &counters, elf_path)?;
            }
        }
        CountersCommand::Watch { interval } => {
            if format == Format::Json {
                bail!("watch prints a stream of changes, use --format jsonl or text");
            }
            cnt_view::watch(
                &mut counters,
                &mut source,
                format,
                Duration::from_millis(interval),
            )?;
        }
        CountersCommand::Tui => {
            if format != Format::Text {
                bail!(
                    "The TUI only supports text output, use `ww counters read --format json|jsonl`"
                );
            }
            cnt_view::tui(&mut counters, &mut source, "over WireWeaver")?;
        }
        CountersCommand::Reset { bkp } => {
            let mut done = vec![];
            for storage in [Storage::Ram, Storage::Bkp] {
                let in_use = match storage {
                    Storage::Ram => counters.ram_counters().is_some(),
                    Storage::Bkp => bkp && counters.bkp_counters().is_some(),
                };
                if !in_use {
                    continue;
                }
                if format == Format::Text {
                    println!("{}", style(format!("Resetting {storage} counters")).dim());
                }
                counters.reset(storage, &mut source)?;
                done.push(storage);
            }
            if format != Format::Text {
                output::reset(format, &done)?;
            }
        }
        CountersCommand::Info => unreachable!("answered before reading"),
    }
    Ok(())
}

fn print_info(info: &ww_counters::CountersInfoOwned, counters: &Counters) {
    let id = if info.build_id.is_empty() {
        style("none".to_string()).yellow()
    } else {
        style(elf::hex(&info.build_id))
    };
    println!("{} {id}", style("build id").dim());
    println!("{} {}", style("cnt format").dim(), info.format);
    let buffers = [(Storage::Ram, Some(info.ram)), (Storage::Bkp, info.backup)];
    for (storage, buffer) in buffers {
        let elf_counters = counters
            .blocks()
            .find(|b| b.storage() == storage)
            .map_or(0, |b| b.entries().len());
        match buffer {
            Some(b) => println!(
                "{} {} of {} words used, {elf_counters} counters",
                style(format!("{storage} buffer")).dim(),
                b.used_words,
                b.capacity_words
            ),
            None => println!(
                "{} {}",
                style(format!("{storage} buffer")).dim(),
                style("none").dim()
            ),
        }
    }
}

/// `ww_counters::Counters` trait client, calls carry a global trait path
pub(crate) struct CountersClient {
    cmd: Commander,
}

/// The calls `WwSource` makes, blocking: run from `spawn_blocking`.
pub(crate) struct Device(CountersClient);

fn call_error<E: std::fmt::Debug>(e: CallError<E>) -> anyhow::Error {
    match e {
        CallError::Device(e) => anyhow!("the device refused: {e:?}"),
        CallError::Unimplemented { resource } => anyhow!("the device doesn't implement {resource}"),
        CallError::Client(e) => e.into(),
    }
}

impl CountersDevice for Device {
    fn read(&mut self, buffer: CounterBuffer, offset: u16) -> Result<Vec<u32>> {
        let page = self
            .0
            .read(buffer, offset)
            .blocking_call()
            .map_err(call_error)?;
        Ok(page.iter().map(|w| w.0).collect())
    }

    fn reset(&mut self, buffer: CounterBuffer) -> Result<()> {
        self.0.reset(buffer).blocking_call().map_err(call_error)
    }
}

mod counters_client {
    use super::*;
    wire_weaver::ww_codegen!(
        ww_counters :: Counters for CountersClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
    );
}
