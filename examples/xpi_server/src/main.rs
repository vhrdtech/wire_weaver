use anyhow::{Context, Result};
use std::time::Duration;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use xpi::owned::NodeId;
use xpi_node::node::addressing::RemoteNodeAddr;
use xpi_node::node::async_std::VhNode;

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let mut node = VhNode::new_server(NodeId(0)).await;

    let addr = "tcp://127.0.0.1:7777";
    let addr = RemoteNodeAddr::parse(addr)
        .context(format!("unable to parse socket address: '{}'", addr))?;
    node.listen(addr).await?;

    tokio::time::sleep(Duration::from_secs(60)).await;
    Ok(())
}
