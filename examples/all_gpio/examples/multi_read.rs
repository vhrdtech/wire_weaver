use all_gpio::{AllGpio, DeviceFilter};
use anyhow::Result;
use std::time::Instant;
use wire_weaver_client_common::multi_read::MultiRead;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = AllGpio::connect(filter, Default::default()).await?;
    let ports = device.port_valid_indices().read().await?;
    let now = Instant::now();

    (
        device.port(0).pin(0).read_speed(),
        device.port(0).pin(1).read_speed(),
    )
        .multi_read();

    // takes 940ms on USB Full Speed, see mode_parallel example for a vast improvement over this
    // 361ms on USB High Speed
    println!("took {} ms", now.elapsed().as_millis());
    device.disconnect().await?;
    Ok(())
}
