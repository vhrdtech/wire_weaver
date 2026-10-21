//! A program that hosts the hub when started as one, and otherwise acts as a client wanting the hub:
//! - `hub_host ensure`: finds the user's hub or starts one, prints the hub's pid
//! - `hub_host stop`: asks the running hub to exit

use wire_weaver_client::hub::{HubClient, ensure_running};

fn main() -> Result<(), String> {
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
    let hub = HubClient::config(|c| c.hub_api_at(socket.display().to_string()))
        .connect()
        .await
        .map_err(|e| format!("{e:#}"))?;
    match arg.as_str() {
        "ensure" => {
            let info = hub.info().call().await.map_err(|e| format!("{e:?}"))?;
            println!("{}", info.pid);
        }
        "stop" => hub.shutdown().call().await.map_err(|e| format!("{e:?}"))?,
        other => return Err(format!("unknown command {other:?}")),
    }
    Ok(())
}
