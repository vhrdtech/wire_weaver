//! Byte-stream listeners (TR-19): TCP clients, streams the program hands in, two APIs on one source, and the same
//! over WebSocket framing.

use std::io;

use tokio_util::compat::TokioAsyncReadCompatExt;
use wire_weaver_client::StreamOpener;
use wire_weaver_server::byte_stream::{TcpIncoming, WsFraming, handed_in};
use ww_byte_stream::Status;

use super::*;
use crate::counter::{self, Counter, CounterClient};

/// The framing of the test servers.
fn framing() -> PreambleFraming {
    PreambleFraming::new("host server 0.1.0", MESSAGE_LEN_MAX)
}

/// A TCP source on a free port of the loopback, and its address.
async fn tcp() -> (TcpIncoming, std::net::SocketAddr) {
    let incoming = TcpIncoming::bind("127.0.0.1:0").await.unwrap();
    let addr = incoming.local_addr().unwrap();
    (incoming, addr)
}

/// Connect to the counter at `addr` as the service `service`.
async fn counter_client(
    addr: std::net::SocketAddr,
    service: &str,
) -> anyhow::Result<CounterClient> {
    tests_common::api!(counter::api_impl).seed();
    let service = service.to_owned();
    CounterClient::config(move |c| {
        c.tcp(addr.to_string())
            .service(service.clone())
            .resolver(tests_common::resolver())
            .link_setup_timeout(SOON)
    })
    .connect()
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_clients_are_served_with_their_address() {
    let (incoming, addr) = tcp().await;
    let bench = start_bench!(
        listen(incoming, framing(), TokioClock::new()),
        Reach::Tcp(addr.to_string()),
        small(),
        false,
    );
    let first = bench.connect().await;
    let second = bench.connect().await;
    assert_eq!(first.whoami().call().await.unwrap(), 0);
    assert_eq!(second.whoami().call().await.unwrap(), 1);
    assert_eq!(second.echo(7).call().await.unwrap(), 7);
    let connections = bench.shared.connections.lock().unwrap().clone();
    let Peer::Addr(peer) = connections[0].peer else {
        panic!("a TCP peer is an address: {:?}", connections[0].peer);
    };
    assert!(peer.ip().is_loopback());
    assert_ne!(peer.port(), addr.port());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_apis_share_one_listener() {
    let (incoming, addr) = tcp().await;
    let listener = listen(incoming, framing(), TokioClock::new());
    let counter_listener = listener.service("counter");
    let counter_served = tokio::spawn(async move {
        Server::new(TokioClock::new())
            .serve(
                counter::link(),
                counter_listener,
                Counter::make,
                std::future::pending(),
            )
            .await
    });
    let bench = start_bench!(listener, Reach::Tcp(addr.to_string()), small(), false);
    let host = bench.connect().await;
    let counter = soon("the counter's client", counter_client(addr, "counter"))
        .await
        .expect("the counter connects");
    assert_eq!(host.echo(3).call().await.unwrap(), 3);
    assert_eq!(
        counter.read_ticks().read().await.unwrap().value,
        counter::TICKS
    );
    // the counter's client is not a connection of the host server
    assert_eq!(bench.shared.connections.lock().unwrap().len(), 1);
    let refused = soon("a client of no service", counter_client(addr, "journal")).await;
    let refused = refused.err().expect("an unknown service is refused");
    let Some(wire_weaver_client::Error::HubRefused { status, .. }) = refused.downcast_ref() else {
        panic!("an unknown service is refused: {refused:?}");
    };
    assert_eq!(status, &Status::UnknownService.to_u8());
    assert!(!counter_served.is_finished());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streams_handed_in_are_served_until_their_sender_and_clients_are_gone() {
    let peer = Peer::Unix {
        uid: Uid(7),
        gid: Gid(8),
        pid: None,
    };
    let (sender, handed) = handed_in(1);
    let sender = Arc::new(Mutex::new(Some(sender)));
    let for_opener = sender.clone();
    let opener = StreamOpener::new("duplex", move || {
        let sender = for_opener.lock().unwrap().clone();
        async move {
            let mut sender = sender.ok_or_else(|| io::Error::other("no more streams"))?;
            let (client, server) = tokio::io::duplex(MESSAGE_LEN_MAX);
            sender
                .serve_stream(server.compat(), peer)
                .await
                .map_err(|_| io::Error::other("the listener is gone"))?;
            Ok(client.compat())
        }
    });
    let mut bench = start_bench!(
        listen(handed, framing(), TokioClock::new()),
        Reach::Stream(opener),
        small(),
        false,
    );
    let client = bench.connect().await;
    assert_eq!(client.call_uid().call().await.unwrap(), 7);
    assert_eq!(bench.shared.connections.lock().unwrap()[0].peer, peer);
    // no more streams come; the one served stays
    sender.lock().unwrap().take();
    assert_eq!(client.echo(5).call().await.unwrap(), 5);
    assert!(!bench.served.is_finished());
    drop(client);
    soon("the server ends", &mut bench.served).await.unwrap();
}

/// Connect to the counter at the WebSocket URL `url`.
async fn counter_ws_client(url: String) -> anyhow::Result<CounterClient> {
    tests_common::api!(counter::api_impl).seed();
    CounterClient::config(move |c| {
        c.websocket_url(url.clone())
            .resolver(tests_common::resolver())
            .link_setup_timeout(SOON)
    })
    .connect()
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_clients_name_the_api_in_the_path() {
    let (incoming, addr) = tcp().await;
    let listener = listen(incoming, WsFraming::new(MESSAGE_LEN_MAX), TokioClock::new());
    let counter_listener = listener.service("counter");
    tokio::spawn(async move {
        Server::new(TokioClock::new())
            .serve(
                counter::link(),
                counter_listener,
                Counter::make,
                std::future::pending(),
            )
            .await
    });
    let bench = start_bench!(
        listener,
        Reach::WebSocket(format!("ws://{addr}/")),
        small(),
        false,
    );
    let host = bench.connect().await;
    let counter = soon(
        "the counter's client",
        counter_ws_client(format!("ws://{addr}/counter")),
    )
    .await
    .expect("the counter connects");
    assert_eq!(host.echo(3).call().await.unwrap(), 3);
    assert_eq!(
        counter.read_ticks().read().await.unwrap().value,
        counter::TICKS
    );
    let Peer::Addr(peer) = bench.shared.connections.lock().unwrap()[0].peer else {
        panic!("a WebSocket client on TCP has an address");
    };
    assert!(peer.ip().is_loopback());
    let refused = soon(
        "a client of no service",
        counter_ws_client(format!("ws://{addr}/journal")),
    )
    .await;
    let refused = refused.err().expect("an unknown service is refused");
    assert!(format!("{refused:#}").contains("404"), "{refused:#}");
}
