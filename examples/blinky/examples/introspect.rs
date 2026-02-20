use anyhow::Result;
use blinky::DeviceFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = wire_weaver_usb_host::util::connect_runtime_api(filter).await?;

    let api_bundle = device.introspect().download().await?;
    println!("{:#?}", api_bundle);

    device.disconnect().await;

    Ok(())
}
