use blinky_evolved::Blinky;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let mut device = Blinky::new().connect().await?;

    println!("Device info: {:?}", device.info());

    let r = device.led_toggle().call().await;
    let err = r.unwrap_err();
    println!("led_toggle() failed: {err}");

    device.disconnect().await?;

    Ok(())
}
