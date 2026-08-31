use blinky::Blinky;
use wire_weaver_client::{DynClient, WwClient};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let device = DynClient::from_config(Blinky::default_config())
        .connect()
        .await?;
    println!("{:?}", device.device_api_info());

    // can use .download to force download, .get will try local cache
    let api_bundle = device.introspect().get().await?;
    println!("{:#?}", api_bundle);

    device.disconnect().await?;
    Ok(())
}
