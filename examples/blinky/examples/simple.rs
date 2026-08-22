use anyhow::Result;
use blinky::{Blinky, DeviceFilter};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = Blinky::connect(filter, Default::default()).await?;

    println!("Turning LED on");
    device.led_on().call().await?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    println!("Turning LED off");
    device.led_off().call().await?;

    device.disconnect_and_exit().await?;

    Ok(())
}
