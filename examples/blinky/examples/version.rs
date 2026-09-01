use anyhow::Result;
use blinky::Blinky;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let device = Blinky::new().connect().await?;

    println!("{:?}", device.info());

    device.disconnect().await?;

    Ok(())
}
