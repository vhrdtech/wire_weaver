use all_gpio::{AllGpio, DeviceFilter, OnError};
use anyhow::Result;
use futures::future::join_all;
use std::time::Instant;
use ww_gpio::Mode;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = AllGpio::connect(filter, OnError::ExitImmediately).await?;
    let port_count = device.port_count().call().await?;
    let now = Instant::now();

    // send multiple call requests in parallel without waiting for each individual one to finish
    let port_names = join_all((0..port_count).map(|port| device.port(port).name().call())).await;
    let port_names: Result<Vec<String>, _> = port_names.into_iter().collect();
    let port_names = port_names?;

    let modes = (0..16 * port_count)
        .map(|i| device.port(i / 16).pin(i % 16).mode().call())
        .collect::<Vec<_>>();
    let modes = join_all(modes).await;
    let modes: Result<Vec<Mode>, _> = modes.into_iter().collect();
    let modes = modes?;

    for port in 0..port_count {
        let port_name = &port_names[port as usize];
        for pin in 0..16 {
            let mode = modes[(port * 16 + pin) as usize];
            println!("{port_name}{}: {:?}", pin, mode);
        }
    }

    // 17ms on USB High Speed
    // 125μs accumulation time, also 17ms with 300μs and 1ms windows (but much fewer packets)
    println!("took {} ms", now.elapsed().as_millis());
    device.disconnect_and_exit().await?;
    Ok(())
}
