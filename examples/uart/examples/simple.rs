use anyhow::Result;
use std::time::Duration;
use uart::{DeviceFilter, OnError, UartBridge};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = UartBridge::connect(filter, OnError::ExitImmediately).await?;

    let mut uart0_tx = device.uart(0).tx()?;
    uart0_tx.send_bytes(b"abc000def")?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    device.disconnect_and_exit().await?;
    Ok(())
}
