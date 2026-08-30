use std::time::Duration;
use uart::UartBridge;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let mut device = UartBridge::new().connect().await?;

    let mut uart0_tx = device.uart(0).tx().await?;
    uart0_tx.send_bytes(b"abc000def").await?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    device.disconnect().await?;
    Ok(())
}
