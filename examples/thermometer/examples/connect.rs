//! Reads one sample from the thermometer over the transport you name: the same call, a different medium.
//!
//! `cargo run -p thermometer --example thermometer_connect -- websocket` against `thermometer_sim`, or `-- udp`
//! against `thermometer_sim --udp 9000`. `usb` reaches a board flashed with the thermometer firmware.

use anyhow::Result;
use clap::{Parser, ValueEnum};
use thermometer::Thermometer;

#[derive(Parser)]
struct Args {
    #[arg(value_enum)]
    transport: Transport,
    /// Where to connect: a WebSocket URL or a UDP address; defaults to the simulated device on this PC
    #[arg(long)]
    address: Option<String>,
    /// A USB device by serial number, when several are plugged in
    #[arg(long)]
    serial: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Transport {
    Usb,
    Websocket,
    Udp,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let device = match args.transport {
        Transport::Usb => usb(args.serial).await?,
        Transport::Websocket => websocket(args.address).await?,
        Transport::Udp => udp(args.address).await?,
    };
    let sample = device.read_once().call().await?;
    println!("input {}: {}", sample.input.get(), sample.temp);
    device.disconnect().asynch().await?;
    Ok(())
}

async fn usb(serial: Option<String>) -> Result<Thermometer> {
    // --8<-- [start:usb]
    // any WireWeaver device on USB, or the one with this serial number
    let device = Thermometer::config(|c| match &serial {
        None => c.usb(),
        Some(serial) => c.usb().serial_eq(serial.clone()),
    })
    .connect()
    .await?;
    // --8<-- [end:usb]
    Ok(device)
}

async fn websocket(address: Option<String>) -> Result<Thermometer> {
    // --8<-- [start:websocket]
    let url = address.unwrap_or_else(|| "ws://127.0.0.1:8080/ww".to_owned());
    let device = Thermometer::config(|c| c.websocket_url(url.clone()).no_ww_usb())
        .connect()
        .await?;
    // --8<-- [end:websocket]
    Ok(device)
}

async fn udp(address: Option<String>) -> Result<Thermometer> {
    // --8<-- [start:udp]
    let addr = address.unwrap_or_else(|| "127.0.0.1:9000".to_owned());
    let device = Thermometer::config(|c| c.udp_addr(addr.clone()).no_ww_usb())
        .connect()
        .await?;
    // --8<-- [end:udp]
    Ok(device)
}
