//! A program that hosts the ww hub when started as one (`ww_hub::main_if_hub`), and otherwise acts as a client of the
//! user's hub:
//! - `hub_dev ensure`: finds the user's hub or starts one (replacing an older one, SRV-30), prints its version
//! - `hub_dev stop`: asks the running hub to exit

use wire_weaver_client::hub::{HubClient, ensure_running};

fn main() -> Result<(), String> {
    // a hub's stderr goes to hub.log in the runtime dir
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
    // first, before any runtime: the hub runs its own
    ww_hub::main_if_hub();
    tokio::runtime::Runtime::new()
        .map_err(|e| e.to_string())?
        .block_on(client(std::env::args().nth(1).unwrap_or_default()))
}

async fn client(arg: String) -> Result<(), String> {
    let socket = ensure_running(arg == "ensure")
        .await
        .map_err(|e| e.to_string())?;
    let hub = HubClient::config(|c| {
        c.hub_api_at(socket.display().to_string())
            .hub_reconnect(None)
    })
    .connect()
    .await
    .map_err(|e| format!("{e:#}"))?;
    match arg.as_str() {
        "ensure" => {
            let info = hub.info().call().await.map_err(|e| format!("{e:?}"))?;
            println!("{} {}", info.hub_version, info.pid);
        }
        "stop" => hub.shutdown().call().await.map_err(|e| format!("{e:?}"))?,
        other => return Err(format!("unknown command {other:?}")),
    }
    Ok(())
}
