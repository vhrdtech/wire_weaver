use std::time::Duration;

use blinky::Blinky;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let device = Blinky::new().connect().await?;

    // For methods with unit return type _forget option is available - no response will be sent from device
    device.led_on().call_forget().await?;
    device.led_off().call_forget().await?;
    device.led_on().call_forget().await?;
    device.led_off().call_forget().await?;
    device.led_on().call_forget().await?;

    // since we do not await for any responses, wait a bit to allow requests to be sent
    tokio::time::sleep(Duration::from_millis(10)).await;

    device.disconnect().await?;

    Ok(())
}
