use anyhow::Result;
use std::time::Duration;
use uart::{DeviceFilter, UartBridge};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = UartBridge::connect(filter, Default::default()).await?;

    let mut uart0_tx = device.uart(0).tx().await?;
    uart0_tx.send_bytes(b"abc000def").await?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    device.disconnect().await?;
    Ok(())
}
