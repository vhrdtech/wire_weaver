use anyhow::Result;
use blinky_evolved::{Blinky, DeviceFilter};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = Blinky::connect(filter, Default::default()).await?;

    println!("Device info: {:?}", device.info());

    let r = device.led_toggle().call().await;
    let err = r.unwrap_err();
    println!("led_toggle() failed: {err}");

    device.disconnect().await?;

    Ok(())
}
