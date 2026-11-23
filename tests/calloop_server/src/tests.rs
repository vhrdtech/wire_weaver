use std::cell::RefCell;
use std::future::Future;
use std::net::SocketAddr;
use std::rc::Rc;
use std::thread::JoinHandle;
use std::time::Duration;

use calloop::EventLoop;
use futures_channel::oneshot;
use futures_util::FutureExt;
use futures_util::future::{Shared, join};
use host_server::counter::{self, Counter, CounterClient};
use wire_weaver::Peer;
use wire_weaver_server::byte_stream::{WsFraming, listen};
use wire_weaver_server::{Connection, Server};
use ww_calloop::{LoopClock, TcpIncoming, bind_udp};

/// How long a step may take before the test fails.
const SOON: Duration = Duration::from_secs(20);

/// Hosts the UDP listener serves at once.
const PEERS: usize = 4;

/// A calloop loop on its own thread, serving the counter over WebSocket and UDP on one port number.
struct Bench {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    /// Asks the loop to stop both listeners; it answers once they are stopped.
    stop_listeners: calloop::channel::Sender<std::sync::mpsc::SyncSender<()>>,
    /// Ends with the peers of every connection the servers made a backend for.
    thread: Option<JoinHandle<Vec<Peer>>>,
}

impl Bench {
    fn start() -> Self {
        let (addr_tx, addr_rx) = std::sync::mpsc::sync_channel(1);
        let (shutdown, stop) = oneshot::channel::<()>();
        let (stop_listeners, stops) = calloop::channel::channel();
        let thread = std::thread::spawn(move || run_loop(&addr_tx, stop, stops));
        let addr = addr_rx.recv_timeout(SOON).expect("the loop starts");
        Bench {
            addr,
            shutdown: Some(shutdown),
            stop_listeners,
            thread: Some(thread),
        }
    }

    /// Stops both listeners on the loop, and waits until they are.
    fn stop_listeners(&self) {
        let (done, stopped) = std::sync::mpsc::sync_channel(1);
        self.stop_listeners
            .send(done)
            .expect("the loop takes the stop");
        stopped
            .recv_timeout(SOON)
            .expect("the loop stops the listeners");
    }

    /// Asks the servers to shut down and waits for the loop to end; the peers the servers saw.
    fn stop(mut self) -> Vec<Peer> {
        self.shutdown
            .take()
            .expect("stopped once")
            .send(())
            .expect("the loop waits for shutdown");
        let thread = self.thread.take().expect("stopped once");
        let deadline = std::time::Instant::now() + SOON;
        while !thread.is_finished() {
            assert!(
                std::time::Instant::now() < deadline,
                "the loop ends after shutdown"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        thread.join().expect("the loop thread doesn't panic")
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        // a failed test still ends the loop
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

/// The loop: both listeners on a free port of the loopback, sent through `addr`, served until `stop`; `stops`
/// stops the listeners alone.
fn run_loop(
    addr: &std::sync::mpsc::SyncSender<SocketAddr>,
    stop: oneshot::Receiver<()>,
    stops: calloop::channel::Channel<std::sync::mpsc::SyncSender<()>>,
) -> Vec<Peer> {
    let mut event_loop: EventLoop<'static, ()> = EventLoop::try_new().expect("a loop");
    let handle = event_loop.handle();
    let (executor, scheduler) = calloop::futures::executor().expect("an executor");
    handle
        .insert_source(executor, |(), _, ()| ())
        .expect("the loop takes the executor");

    let clock = LoopClock::new(handle.clone());
    let tcp = TcpIncoming::bind(&handle, "127.0.0.1:0").expect("a TCP port");
    let tcp_addr = tcp.local_addr();
    let tcp_stop = tcp.stop_handle();
    let ws = listen(tcp, WsFraming::new(counter::MESSAGE_LEN_MAX), clock.clone());
    let udp =
        bind_udp::<_, PEERS>(&handle, tcp_addr, counter::MESSAGE_LEN_MAX).expect("the UDP port");
    let udp_stop = udp.stop_handle();
    handle
        .insert_source(stops, move |event, _, ()| {
            if let calloop::channel::Event::Msg(done) = event {
                tcp_stop.stop();
                udp_stop.stop();
                done.send(()).expect("the test waits for the stop");
            }
        })
        .expect("the loop takes the stop channel");

    // backends and what they share need not be `Send`: everything runs on this thread
    let peers = Rc::new(RefCell::new(Vec::new()));
    let make = {
        let peers = peers.clone();
        move |connection: &Connection| {
            peers.borrow_mut().push(connection.peer);
            Counter::make(connection)
        }
    };
    let shutdown: Shared<_> = stop.map(|_| ()).shared();
    let signal = event_loop.get_signal();
    scheduler
        .schedule(async move {
            let server = Server::new(clock);
            let (ws, udp) = join(
                server.serve(counter::link(), ws, &make, shutdown.clone()),
                server.serve(counter::link(), udp, &make, shutdown),
            )
            .await;
            ws.expect("the WebSocket server ends without an error");
            udp.expect("the UDP server ends without an error");
            signal.stop();
        })
        .expect("the executor takes the server");

    addr.send(tcp_addr).expect("the test waits for the address");
    event_loop
        .run(None, &mut (), |()| ())
        .expect("the loop runs");
    peers.take()
}

async fn soon<T>(what: &str, f: impl Future<Output = T>) -> T {
    tokio::time::timeout(SOON, f)
        .await
        .unwrap_or_else(|_| panic!("{what} took over {SOON:?}"))
}

async fn ws_client(addr: SocketAddr) -> CounterClient {
    tests_common::api!(counter::api_impl).seed();
    let connect = CounterClient::config(move |c| {
        c.websocket_url(format!("ws://{addr}/"))
            .resolver(tests_common::resolver())
            .link_setup_timeout(SOON)
    })
    .connect();
    soon("the WebSocket client", connect)
        .await
        .expect("connects")
}

async fn udp_client(addr: SocketAddr) -> CounterClient {
    tests_common::api!(counter::api_impl).seed();
    let connect = CounterClient::config(move |c| {
        c.udp_addr(addr.to_string())
            .resolver(tests_common::resolver())
            .link_setup_timeout(SOON)
    })
    .connect();
    soon("the UDP client", connect).await.expect("connects")
}

/// A client of each kind reads the counter, each connection gets the address of its client as its peer, and
/// shutdown ends the servers and the loop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_and_udp_clients_are_served_on_the_loop() {
    let bench = Bench::start();
    let ws = ws_client(bench.addr).await;
    let udp = udp_client(bench.addr).await;
    for client in [&ws, &udp] {
        let ticks = soon("a read", client.read_ticks().read()).await.unwrap();
        assert_eq!(ticks.value, counter::TICKS);
    }

    let peers = tokio::task::spawn_blocking(move || bench.stop())
        .await
        .unwrap();
    assert_eq!(peers.len(), 2, "a connection per client: {peers:?}");
    for peer in peers {
        let Peer::Addr(addr) = peer else {
            panic!("a peer is its address: {peer:?}");
        };
        assert!(addr.ip().is_loopback());
    }
}

/// Several UDP hosts at once, each on a connection of its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_hosts_get_a_connection_each() {
    let bench = Bench::start();
    let mut clients = Vec::new();
    for _ in 0..PEERS {
        clients.push(udp_client(bench.addr).await);
    }
    for client in &clients {
        let ticks = soon("a read", client.read_ticks().read()).await.unwrap();
        assert_eq!(ticks.value, counter::TICKS);
    }
    let peers = tokio::task::spawn_blocking(move || bench.stop())
        .await
        .unwrap();
    assert_eq!(peers.len(), PEERS);
}

/// Each listener stops alone (SRV-75): the TCP port is free at once and refuses new clients, a WebSocket and a UDP
/// client connected before go on, and shutdown still ends the servers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopped_listeners_free_the_port_and_keep_their_connections() {
    let bench = Bench::start();
    let ws = ws_client(bench.addr).await;
    let udp = udp_client(bench.addr).await;
    let addr = bench.addr;
    let bench = tokio::task::spawn_blocking(move || {
        bench.stop_listeners();
        bench
    })
    .await
    .unwrap();

    let refused = std::net::TcpStream::connect(addr).map(drop);
    assert_eq!(
        refused.map_err(|e| e.kind()),
        Err(std::io::ErrorKind::ConnectionRefused)
    );
    let rebound = std::net::TcpListener::bind(addr).expect("the TCP port is free");
    for client in [&ws, &udp] {
        let ticks = soon("a read", client.read_ticks().read()).await.unwrap();
        assert_eq!(ticks.value, counter::TICKS);
    }
    drop(rebound);
    let peers = tokio::task::spawn_blocking(move || bench.stop())
        .await
        .unwrap();
    assert_eq!(peers.len(), 2);
}

/// A mount table on the loop (SRV-78) reaches a backend on a Unix socket through `UnixConnector`: the counter,
/// served by tokio in another thread, comes up in its slot with its API learned.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mounts_on_the_loop_reach_a_unix_socket() {
    use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, UNib32};
    use wire_weaver::ww_api::mount::MountPresenceOwned;
    use wire_weaver_server::byte_stream::PreambleFraming;
    use wire_weaver_server::local_socket::LocalSocketIncoming;
    use wire_weaver_server::mount::{MountOptions, Mounts};
    use ww_calloop::UnixConnector;
    use ww_client_server::ResourceIndex;

    let dir = std::env::temp_dir().join(format!("ww-calloop-mount-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("counter.sock");
    let socket = ww_local_socket::listen(&path, true).expect("the counter listens");
    let (stop, stopped) = oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        let listener = listen(
            LocalSocketIncoming::new(socket),
            PreambleFraming::new("counter 0.1.0", counter::MESSAGE_LEN_MAX),
            wire_weaver_client::in_process::TokioClock::new(),
        );
        Server::new(wire_weaver_client::in_process::TokioClock::new())
            .serve(counter::link(), listener, Counter::make, stopped.map(|_| ()))
            .await
            .expect("the counter serves");
    });

    let for_loop = path.clone();
    let presence = tokio::task::spawn_blocking(move || {
        let mut event_loop: EventLoop<'static, ()> = EventLoop::try_new().expect("a loop");
        let handle = event_loop.handle();
        let (executor, scheduler) = calloop::futures::executor().expect("an executor");
        handle
            .insert_source(executor, |(), _, ()| ())
            .expect("the loop takes the executor");
        let own = ww_self::compressed::bundle_bytes(&counter::api_impl::WW_SELF_BYTES).unwrap();
        let own = ww_self::ApiBundleOwned::from_ww_bytes_owned(&own).unwrap();
        let mounts = Mounts::new(std::sync::Arc::new(own));
        let slot = [ResourceIndex::new(UNib32(0)), ResourceIndex::new(UNib32(1))];
        mounts.mount(
            &slot,
            UnixConnector::new(&handle, for_loop),
            MountOptions::default(),
        );
        let table = mounts.clone();
        let clock = LoopClock::new(handle.clone());
        scheduler
            .schedule(async move { table.run(clock).await })
            .expect("the loop runs the table");
        let deadline = std::time::Instant::now() + SOON;
        loop {
            event_loop
                .dispatch(Some(Duration::from_millis(10)), &mut ())
                .expect("the loop runs");
            let presence = mounts.presence(&slot);
            if matches!(presence, MountPresenceOwned::Up { .. })
                || std::time::Instant::now() > deadline
            {
                return presence;
            }
        }
    })
    .await
    .expect("the loop thread doesn't panic");
    let MountPresenceOwned::Up { api, .. } = presence else {
        panic!("the counter is not up: {presence:?}");
    };
    assert_eq!(api.crate_id, "keep_trying_api");
    let _ = stop.send(());
    served.await.unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}
