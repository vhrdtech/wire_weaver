//! A program that hosts the ww hub when started as one (`ww_hub::main_if_hub`), and otherwise acts as a client of the
//! user's hub:
//! - `hub_dev ensure`: finds the user's hub or starts one (replacing an older one, SRV-30), prints its version
//! - `hub_dev stop`: asks the running hub to exit
//! - `hub_dev hold <socket> <kind>`: holds the device served on `<socket>` directly (`ipc_exclusive`), as a program
//!   of client kind `<kind>` (`Cli`, `Gui`, ...), calling it every 10 ms; prints `holding`, then what happens to the
//!   connection: `disconnected <reason>`, `reconnected <gap in µs> <streams resumed>`, `ok after reconnect` (CLT-43)

use wire_weaver_client::hub::{HubClient, ensure_running};

fn main() -> Result<(), String> {
    // a hub's stderr goes to hub.log in the runtime dir
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
    // first, before any runtime: the hub runs its own
    ww_hub::main_if_hub();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    match args.as_slice() {
        [hold, socket, kind] if hold == "hold" => rt.block_on(hold_device(socket, kind)),
        _ => rt.block_on(client(args.first().cloned().unwrap_or_default())),
    }
}

async fn hold_device(socket: &str, kind: &str) -> Result<(), String> {
    use std::io::Write;
    use wire_weaver_client::{ClientKind, ClientMeta, ConnectionEvent};
    let kind = match kind {
        "Cli" => ClientKind::Cli,
        "Gui" => ClientKind::Gui,
        "Service" => ClientKind::Service,
        "Notebook" => ClientKind::Notebook,
        other => return Err(format!("unknown kind {other}")),
    };
    let mut client = hub_reconnect::client::Client::config(|c| {
        c.ipc_path(socket.to_string())
            .ipc_exclusive()
            .client_meta(ClientMeta::new().kind(kind).program("polite-holder"))
    })
    .connect()
    .await
    .map_err(|e| format!("{e:#}"))?;
    let mut events = client.cmd.connection_events();
    // a counted stream, resumed through the hub after a handover
    let _ticks = client.ticks().call().await.map_err(|e| format!("{e:?}"))?;
    let say = |line: String| {
        println!("{line}");
        let _ = std::io::stdout().flush();
    };
    say("holding".into());
    let mut after_reconnect = false;
    loop {
        while let Ok(event) = events.try_recv() {
            match event {
                ConnectionEvent::Disconnected { reason, .. } => {
                    say(format!("disconnected {reason:?}"))
                }
                ConnectionEvent::Reconnected { gap, resumed, .. } => {
                    after_reconnect = true;
                    say(format!("reconnected {} {resumed}", gap.as_micros()));
                }
                _ => {}
            }
        }
        if client.add(1, 2).call().await.is_ok() && after_reconnect {
            after_reconnect = false;
            say("ok after reconnect".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
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
