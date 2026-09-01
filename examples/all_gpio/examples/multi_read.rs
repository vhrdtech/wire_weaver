use all_gpio::AllGpio;
use std::time::Instant;
use wire_weaver_client::MultiRead;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let device = AllGpio::new().connect().await?;

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
    device.disconnect().asynch().await?;
    Ok(())
}
