use anyhow::Result;
use uart::{DeviceFilter, OnError, UartBridge};

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _guard = runtime.enter();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut device = UartBridge::connect_blocking(filter, OnError::ExitImmediately)?;

    let mut uart0_rx = device.uart(0).rx()?;
    let chunk = uart0_rx.recv_blocking()?;
    println!("{:02?}", chunk.bytes);

    device.disconnect_and_exit_blocking()?;
    Ok(())
}
