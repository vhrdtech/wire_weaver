use anyhow::Result;
use blinky::Blinky;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let device = Blinky::new().connect().await?;

    println!("Turning LED on");
    device.led_on().call().await?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    println!("Turning LED off");
    device.led_off().call().await?;

    device.disconnect().await?;

    Ok(())
}
