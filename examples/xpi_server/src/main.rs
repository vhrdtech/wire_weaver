use anyhow::{Result};
use xpi::client_server_owned::NodeId;
use xpi_client_server::server::Server;
use std::time::Duration;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let mut node = Server::new(NodeId(0)).await;

    // let addr = "tcp://127.0.0.1:7777";
    // let addr = RemoteNodeAddr::parse(addr)
    //     .context(format!("unable to parse socket address: '{}'", addr))?;
    node.listen(xpi::client_server_owned::Protocol::Ws { ip_addr: "127.0.0.1".parse().unwrap(), port: 7777 }).await?;

    tokio::time::sleep(Duration::from_secs(60)).await;
    Ok(())
}
