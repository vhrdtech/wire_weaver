use anyhow::Result;
use blinky::{Blinky, DeviceFilter};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut driver = Blinky::connect(filter, Default::default()).await?;

    // For methods with unit return type _forget option is available - no response will be sent from device
    driver.led_on().call_forget().await?;
    driver.led_off().call_forget().await?;
    driver.led_on().call_forget().await?;
    driver.led_off().call_forget().await?;
    driver.led_on().call_forget().await?;

    // since we do not await for any responses, wait a bit to allow requests to be sent
    tokio::time::sleep(Duration::from_millis(10)).await;

    driver.disconnect().await?;

    Ok(())
}
