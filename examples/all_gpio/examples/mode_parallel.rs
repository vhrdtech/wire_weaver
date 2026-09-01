use all_gpio::AllGpio;
use futures::future::join_all;
use std::time::Instant;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let device = AllGpio::new().connect().await?;
    let ports = device.port_valid_indices().read().await?;
    let now = Instant::now();

    // send multiple call requests in parallel without waiting for each individual one to finish
    // multiple requests will be automatically assembled into one USB packet, thus greatly increasing throughput
    let port_names = join_all(ports.iter().map(|port| device.port(port).name().call())).await;
    let port_names: Result<Vec<String>, _> = port_names.into_iter().collect();
    let port_names = port_names?;

    for port in ports.iter() {
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
    device.disconnect().await?;
    Ok(())
}
