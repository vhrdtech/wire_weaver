use all_gpio::{AllGpio, DeviceFilter, OnError};
use anyhow::Result;
use futures::future::join_all;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = AllGpio::connect(filter, OnError::ExitImmediately).await?;
    let port_count = device.port_count().call().await?;
    let now = Instant::now();

    // send multiple call requests in parallel without waiting for each individual one to finish
    // multiple requests will be automatically assembled into one USB packet, thus greatly increasing throughput
    let port_names = join_all((0..port_count).map(|port| device.port(port).name().call())).await;
    let port_names: Result<Vec<String>, _> = port_names.into_iter().collect();
    let port_names = port_names?;

    for port in 0..port_count {
        let modes = (0..=15)
            .map(|pin| device.port(port).pin(pin).mode().call())
            .collect::<Vec<_>>();
        let modes = join_all(modes).await;
        let port_name = &port_names[port as usize];
        for (pin, mode) in modes.into_iter().enumerate() {
            let mode = mode?;
            println!("{port_name}{}: {:?}", pin, mode);
        }
    }

    // takes 55ms compared to mode example on USB Full Speed
    // 38ms on USB High Speed
    println!("took {} ms", now.elapsed().as_millis());
    device.disconnect_and_exit().await?;
    Ok(())
}
