use uart::UartBridge;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _guard = runtime.enter();

    let mut device = UartBridge::new().connect_blocking()?;

    let mut uart0_rx = device.uart(0).rx_blocking()?;
    let chunk = uart0_rx.recv_blocking()?;
    println!("{:02?}", chunk.bytes);

    device.disconnect_blocking()?;
    Ok(())
}
