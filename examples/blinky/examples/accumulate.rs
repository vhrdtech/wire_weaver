use std::time::Duration;
use driver::{OnError, MyDeviceDriver, DeviceFilter, LedState};
use anyhow::Result;

#[tokio::main]
async fn main()-> Result<()> {
    tracing_subscriber::fmt::init();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut driver = MyDeviceDriver::connect(filter, OnError::ExitImmediately).await?;

    // For methods with unit return type _forget option is available - no response will be sent from device
    driver.set_led_state(LedState::On).call_forget()?;
    driver.set_led_state(LedState::Off).call_forget()?;
    driver.set_led_state(LedState::On).call_forget()?;
    driver.set_led_state(LedState::Off).call_forget()?;
    driver.set_led_state(LedState::On).call_forget()?;

    // since we do not await for any responses, wait a bit to allow requests to be sent
    tokio::time::sleep(Duration::from_millis(10)).await;

    driver.disconnect_and_exit().await?;

    Ok(())
}
