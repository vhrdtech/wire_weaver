use std::time::Duration;
use driver::{OnError, MyDeviceDriver, DeviceFilter, LedState};
use anyhow::Result;

#[tokio::main]
async fn main()-> Result<()> {
    tracing_subscriber::fmt::init();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut driver = MyDeviceDriver::connect(filter, OnError::ExitImmediately).await?;

    println!("Turning LED on");
    driver.set_led_state(LedState::On).call().await?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    println!("Turning LED off");
    driver.set_led_state(LedState::Off).call().await?;

    driver.disconnect_and_exit().await?;

    Ok(())
}
