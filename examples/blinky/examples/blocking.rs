use blinky::Blinky;
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _guard = runtime.enter();

    let device = Blinky::new().connect_blocking()?;

    println!("Turning LED on");
    device.led_on().blocking_call()?;

    std::thread::sleep(Duration::from_secs(1));

    println!("Turning LED off");
    device.led_off().blocking_call()?;

    device.disconnect_blocking()?;

    Ok(())
}
