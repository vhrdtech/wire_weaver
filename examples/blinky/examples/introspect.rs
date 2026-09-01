use blinky::Blinky;
use wire_weaver_client::{DynClient, WwClient};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let device = DynClient::from_config(Blinky::default_config())
        .connect()
        .await?;
    println!("{:?}", device.device_api_info());

    let introspect_bundle = device.device_introspect().unwrap();
    println!("{:#?}", introspect_bundle);

    let force_downloaded = device.cmd().introspect().download().await?;
    println!("{:#?}", force_downloaded);

    device.disconnect().asynch().await?;
    Ok(())
}
