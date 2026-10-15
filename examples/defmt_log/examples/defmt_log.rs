//! Shows the defmt log of a device running `examples_mcu/rp2`'s `rp2_usb_log`, over USB, decoded with its ELF.
//!
//! Logs from before this program connected arrive first (the device keeps them in its store), then live ones;
//! messages the device dropped are shown as one line with their exact number, where they were dropped.
//!
//! ```sh
//! cargo run -p defmt_log --example defmt_log -- --elf examples_mcu/rp2/target/thumbv8m.main-none-eabihf/release/rp2_usb_log
//! cargo run -p defmt_log --example defmt_log -- --elf ... --flood 500   # overload the logger on purpose
//! ```

use anyhow::{Context, Result, bail};
use clap::Parser;
use defmt_decoder::{DecodeError, Table};
use defmt_log::DefmtLog;
use object::Object;
use std::path::PathBuf;
use wire_weaver::shrink_wrap::UVlq64;
use wire_weaver_client::{StreamCursor, StreamEvent};

#[derive(Parser)]
#[command(
    version,
    about = "defmt log of an rp2_usb_log device over USB, decoded with its ELF"
)]
struct Args {
    /// The firmware's ELF, with the defmt table
    #[arg(long)]
    elf: PathBuf,
    /// Use the ELF even when its build id differs from the device's
    #[arg(long)]
    force: bool,
    /// Make the device log this many frames at once after connecting
    #[arg(long)]
    flood: Option<u32>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let elf =
        std::fs::read(&args.elf).with_context(|| format!("reading {}", args.elf.display()))?;
    let elf_build_id = object::File::parse(&*elf)?
        .build_id()?
        .unwrap_or_default()
        .to_vec();
    let table = Table::parse(&elf)?.context("the ELF has no defmt table")?;

    let mut device = DefmtLog::new().connect().await?;
    let info = device.log().read_DEFMT().read().await?;
    let build_id = info.build_id.into_inner();
    println!(
        "device: defmt {:?}, build id {}",
        info.encoding,
        hex(&build_id)
    );
    if build_id.is_empty() {
        println!("warning: the device reports no build id, can't check that the ELF matches");
    } else if build_id != elf_build_id {
        let msg = format!(
            "the ELF's build id is {}, not the device's",
            hex(&elf_build_id)
        );
        if !args.force {
            bail!("{msg} (--force to use it anyway)");
        }
        println!("warning: {msg}");
    }

    // resume the session the device started at boot, from its start: logs from before we connected come too
    let status = device.log().frames_status().read().await?;
    let session = status.session.context("the device has no log session")?;
    let cursor = StreamCursor {
        session,
        pos: UVlq64(0),
    };
    let mut frames = device.log().frames_resume(cursor).call().await?;
    if let Some(count) = args.flood {
        device.flood(count).call().await?;
    }

    let mut decoder = table.new_stream_decoder();
    loop {
        let event = match frames.recv_event().await {
            Ok(event) => event,
            Err(e) => {
                println!("stream ended: {e}");
                return Ok(());
            }
        };
        match event {
            StreamEvent::Item { value, units, .. } => {
                decoder.received(&value.bytes.into_inner());
                let mut decoded = 0;
                loop {
                    match decoder.decode() {
                        Ok(frame) => {
                            println!("{}", frame.display(true));
                            decoded += 1;
                        }
                        Err(DecodeError::UnexpectedEof) => break,
                        Err(DecodeError::Malformed) => {
                            println!("error: malformed frame (wrong ELF?)");
                            break;
                        }
                    }
                }
                if decoded != units {
                    println!("warning: {decoded} frames decoded, the device sent {units}");
                }
            }
            StreamEvent::Gap { units, cause, .. } => {
                println!("--- {units} log messages lost ({cause:?}) ---");
            }
            other => tracing::debug!("{other:?}"),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
