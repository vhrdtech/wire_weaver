//! The blinky API served over WebSocket from this PC and announced on the LAN over DNS-SD as `_wireweaver._tcp`.
//! The LED is a log line. Every 5 s the interface's addresses are read again and the record follows a change;
//! Ctrl-C withdraws it.
//!
//! `cargo run -p blinky --example blinky_ws_device -- --id blinky-demo`, then from another PC on the
//! LAN: `avahi-browse -r -t _wireweaver._tcp`, and `blinky_ws --url ws://<address>:8080/ww` blinks it.

use std::num::NonZeroU16;
use std::time::Duration;

use anyhow::Result;
use blinky_device::Blinky;
use clap::{Parser, ValueEnum};
use futures_util::Stream;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use wire_weaver_client::in_process::TokioClock;
use ww_device::ws::{IoSocket, WsBuffers, WsConnection, ws_server};
use ww_dns_sd::{
    Advertiser, InstanceId, InstanceName, ResponderChoice, ServiceRecord, Txt, interface_addresses,
    start_responder,
};

#[derive(Parser)]
struct Args {
    /// TCP port of the WebSocket server
    #[arg(long, default_value_t = NonZeroU16::new(8080).expect("not zero"))]
    port: NonZeroU16,
    /// Instance name shown when browsing
    #[arg(long, default_value = "blinky demo")]
    name: String,
    /// Stable instance id, a-z, 0-9 and -
    #[arg(long)]
    id: String,
    /// Interface whose addresses are announced; every interface but loopback when left out
    #[arg(long)]
    interface: Option<String>,
    /// Responder that announces the record
    #[arg(long, value_enum, default_value_t = Responder::Auto)]
    responder: Responder,
}

#[derive(Copy, Clone, ValueEnum)]
enum Responder {
    Auto,
    InProcess,
    Avahi,
}

const MAX_MESSAGE_LEN: usize = 1024;
const PATH: &str = "/ww";
const ADDRESS_CHECK_INTERVAL: Duration = Duration::from_secs(5);

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    // the medium is not Send (NoopRawMutex, as on embassy): everything runs on one LocalSet
    rt.block_on(tokio::task::LocalSet::new().run_until(run(args)))
}

async fn run(args: Args) -> Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", args.port.get())).await?;

    let txt = Txt::new(
        PATH,
        &blinky_api::BLINKY_API_FULL_GID,
        Some(blinky_api::BLINKY_API_WIRE_VERSION),
        InstanceId::new(args.id)?,
    )?;
    let record = ServiceRecord::new(InstanceName::new(args.name)?, args.port, txt)
        .with_addresses(interface_addresses(args.interface.as_deref())?)?;
    let responder = start_responder(match args.responder {
        Responder::Auto => ResponderChoice::Auto,
        Responder::InProcess => ResponderChoice::InProcess,
        Responder::Avahi => ResponderChoice::Avahi,
    })?;
    let mut advertiser = Advertiser::new(responder, record)?;
    println!(
        "serving ws://0.0.0.0:{}{PATH}, announced at {:?}",
        args.port,
        advertiser.record().addresses()
    );

    let conn = WsConnection::new(IoSocket::new(incoming(listener)));
    let mut buffers = Box::new(WsBuffers::<MAX_MESSAGE_LEN>::new());
    let mut server = ws_server(
        Blinky::link_config(),
        &conn,
        TokioClock::new(),
        &mut buffers,
    );
    let mut state = Blinky::new(Box::leak(Box::new(PrintedLed)));
    let mut check = tokio::time::interval(ADDRESS_CHECK_INTERVAL);
    loop {
        tokio::select! {
            ready = server.wait() => {
                if let Some(event) = server.handle(ready, &mut state).await {
                    println!("{event:?}");
                }
            }
            _ = check.tick() => {
                if advertiser.set_addresses(interface_addresses(args.interface.as_deref())?)? {
                    println!("addresses changed, announced at {:?}", advertiser.record().addresses());
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    advertiser.withdraw()?;
    println!("record withdrawn");
    Ok(())
}

/// The clients of `listener`, one at a time as [IoSocket] takes them, with Nagle off.
fn incoming(listener: TcpListener) -> impl Stream<Item = Compat<TcpStream>> + Unpin {
    Box::pin(futures_util::stream::unfold(
        listener,
        |listener| async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        if let Err(e) = stream.set_nodelay(true) {
                            println!("{peer}: TCP_NODELAY: {e}");
                        }
                        println!("client {peer}");
                        return Some((stream.compat(), listener));
                    }
                    Err(e) => println!("accept: {e}"),
                }
            }
        },
    ))
}

/// The LED of this device: a log line.
struct PrintedLed;

impl blinky::Led for PrintedLed {
    fn set(&mut self, on: bool) {
        println!("LED {}", if on { "on" } else { "off" });
    }
}
