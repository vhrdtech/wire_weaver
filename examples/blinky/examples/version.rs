use anyhow::Result;
use blinky::{Blinky, DeviceFilter, OnError};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = Blinky::connect(filter, OnError::ExitImmediately).await?;

    println!("{:?}", device.info());

    device.disconnect_and_exit().await?;

    Ok(())
}
