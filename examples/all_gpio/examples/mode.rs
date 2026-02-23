use all_gpio::{AllGpio, DeviceFilter, OnError};
use anyhow::Result;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = AllGpio::connect(filter, OnError::ExitImmediately).await?;
    let port_count = device.port_count().call().await?;
    let now = Instant::now();

    for port in 0..port_count {
        let port_name = device.port(port).name().call().await?;
        for pin in 0..=15 {
            let mode = device.port(port).pin(pin).mode().call().await?;
            println!("{port_name}{}: {:?}", pin, mode);
        }
    }

    // takes 940ms on USB Full Speed, see mode_parallel example for a vast improvement over this
    // 361ms on USB High Speed
    println!("took {} ms", now.elapsed().as_millis());
    device.disconnect_and_exit().await?;
    Ok(())
}
