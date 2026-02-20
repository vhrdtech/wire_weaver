use anyhow::Result;
use blinky::{Blinky, DeviceFilter, OnError};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut driver = Blinky::connect(filter, OnError::ExitImmediately).await?;

    println!("Turning LED on");
    driver.led_on().call().await?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    println!("Turning LED off");
    driver.led_off().call().await?;

    driver.disconnect_and_exit().await?;

    Ok(())
}
