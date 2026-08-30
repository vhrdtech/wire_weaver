use all_gpio::AllGpio;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let mut device = AllGpio::new().connect().await?;

    let available_ports = device.port_valid_indices().read().await?;
    println!("Available ports: {:?}", available_ports);
    for port_idx in available_ports.iter() {
        let name = device.port(port_idx).name().call().await?;
        let available_pins = device.port(port_idx).pin_valid_indices().read().await?;
        println!("Port {port_idx}: name: '{name}', pins: {available_pins:?}");
    }

    let capabilities = device.port(0).capabilities().call().await?;
    println!("Port 0 capabilities: {:#?}", capabilities);

    device.disconnect().await?;
    Ok(())
}
