//! Blink over RTT through a debug probe, against `ww_rtt` firmware from `examples_mcu` (e.g., `just run nucleo ww_rtt`
//! in `examples_mcu/nucleo_g0b1re`). Stop `probe-rs run` first, a probe can only be opened once.
//!
//! `cargo run -p blinky --features rtt --example blinky_rtt -- --chip STM32G0B1RETx`
//!
//! Pass the firmware's ELF with `--elf` to skip scanning RAM for the RTT control block, e.g.,
//! `--elf examples_mcu/nucleo_g0b1re/target/thumbv6m-none-eabi/debug/nucleo-g0b1re-ww-rtt`.

use anyhow::Result;
use blinky::Blinky;
use clap::Parser;
use std::time::Duration;

#[derive(Parser)]
struct Args {
    /// probe-rs chip name
    #[arg(long, default_value = "STM32G0B1RETx")]
    chip: String,
    /// Select a probe by serial number (substring), if several are connected
    #[arg(long)]
    probe_serial: Option<String>,
    /// SWD / JTAG clock
    #[arg(long)]
    speed_hz: Option<u32>,
    /// Firmware ELF file, to take the RTT control block address from instead of scanning RAM
    #[arg(long)]
    elf: Option<std::path::PathBuf>,
    /// RTT control block address (hex, e.g., 0x20000000), instead of scanning RAM
    #[arg(long, value_parser = parse_hex)]
    control_block_at: Option<u64>,
    #[arg(long, default_value_t = 3)]
    blinks: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    // default config (device's USB VID:PID) is kept, only filters after rtt(..) select the probe
    let device = Blinky::config(|c| {
        let mut c = c.rtt(args.chip.clone(), args.speed_hz);
        if let Some(elf) = &args.elf {
            c = c.rtt_elf(elf);
        }
        if let Some(address) = args.control_block_at {
            c = c.rtt_control_block_at(address);
        }
        match &args.probe_serial {
            Some(serial) => c.serial_contains(serial.clone()),
            None => c,
        }
    })
    .connect()
    .await?;
    println!("Connected: {:?}", device.info());

    for _ in 0..args.blinks {
        println!("Turning LED on");
        device.led_on().call().await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        println!("Turning LED off");
        device.led_off().call().await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    device.disconnect().asynch().await?;
    Ok(())
}

fn parse_hex(s: &str) -> Result<u64, std::num::ParseIntError> {
    u64::from_str_radix(s.trim_start_matches("0x"), 16)
}
