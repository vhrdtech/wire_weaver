use all_gpio::AllGpio;
use std::time::Instant;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let mut device = AllGpio::new().connect().await?;
    let ports = device.port_valid_indices().read().await?;
    let now = Instant::now();

    for port in ports.iter() {
        let port_name = device.port(port).name().call().await?;
        for pin in 0..=15 {
            let mode = device.port(port).pin(pin).mode().call().await?;
            println!("{port_name}{}: {:?}", pin, mode);
        }
    }

    // takes 940ms on USB Full Speed, see mode_parallel example for a vast improvement over this
    // 361ms on USB High Speed
    println!("took {} ms", now.elapsed().as_millis());
    device.disconnect().await?;
    Ok(())
}
