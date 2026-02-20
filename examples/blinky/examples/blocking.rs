use std::time::Duration;
use driver::{OnError, MyDeviceDriver, DeviceFilter, LedState};
use anyhow::Result;

fn main()-> Result<()> {
    tracing_subscriber::fmt::init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _guard = runtime.enter();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut driver = MyDeviceDriver::connect_blocking(filter, OnError::ExitImmediately)?;

    println!("Turning LED on");
    driver.set_led_state(LedState::On).blocking_call()?;

    std::thread::sleep(Duration::from_secs(1));

    println!("Turning LED off");
    driver.set_led_state(LedState::Off).blocking_call()?;

    driver.disconnect_and_exit_blocking()?;

    Ok(())
}
