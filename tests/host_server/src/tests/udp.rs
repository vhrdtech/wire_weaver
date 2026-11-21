//! The UDP listener (SRV-69): a connection per host of one datagram socket. Simulated UDP (`in_process::udp_device`)
//! on tokio's paused clock where time matters, a real socket on the loopback for the rest.

use wire_weaver_client::in_process::{UdpAddr, UdpPort, udp_device};
use wire_weaver_server::udp::UdpListener;
use ww_device::udp::DatagramSocket;

use super::*;

/// Simulated UDP whose device stops hearing the hosts it is told to mute: to them the device is gone, as when a
/// host goes quiet (its program frozen, its network down).
#[derive(Clone)]
struct Muting {
    port: Arc<UdpPort>,
    hosts: Arc<Mutex<Hosts>>,
}

#[derive(Default)]
struct Hosts {
    /// Every host heard so far, in order.
    heard: Vec<UdpAddr>,
    muted: Vec<UdpAddr>,
}

impl Muting {
    fn new(path: &str) -> Self {
        Muting {
            port: Arc::new(udp_device(path)),
            hosts: Arc::default(),
        }
    }

    /// The hosts, also after a test thread panicked holding them.
    fn hosts(&self) -> std::sync::MutexGuard<'_, Hosts> {
        self.hosts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Mute every host heard so far.
    fn mute_heard(&self) {
        let mut hosts = self.hosts();
        let heard = hosts.heard.clone();
        hosts.muted.extend(heard);
    }
}

impl DatagramSocket for Muting {
    type Addr = UdpAddr;
    type RecvError = <UdpPort as DatagramSocket>::RecvError;
    type SendError = <UdpPort as DatagramSocket>::SendError;

    async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, UdpAddr), Self::RecvError> {
        loop {
            let (len, from) = self.port.recv_from(buf).await?;
            let mut hosts = self.hosts();
            if !hosts.heard.contains(&from) {
                hosts.heard.push(from);
            }
            if !hosts.muted.contains(&from) {
                return Ok((len, from));
            }
        }
    }

    async fn send_to(&self, datagram: &[u8], addr: UdpAddr) -> Result<(), Self::SendError> {
        self.port.send_to(datagram, addr).await
    }
}

/// A bench on simulated UDP at `path` with `PEERS` slots, the connections as tasks of their own.
fn simulated<const PEERS: usize>(path: &str, socket: Muting) -> Bench {
    start_bench!(
        UdpListener::<_, PEERS>::new(socket, MESSAGE_LEN_MAX),
        Reach::InProcess(path.to_owned()),
        small(),
        true,
    )
}

/// Until `count` connections run.
async fn live(bench: &Bench, count: u32) {
    while bench.shared.live.load(Ordering::Acquire) != count {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Many hosts on one real socket, each its own connection with its address, served on all the runtime's threads.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_hosts_on_one_port() {
    let listener =
        UdpListener::<_, 16>::bind("127.0.0.1:0".parse().unwrap(), MESSAGE_LEN_MAX).unwrap();
    let addr = listener.local_addr().unwrap();
    let bench = start_bench!(listener, Reach::Udp(addr.to_string()), small(), true);
    let clients: Vec<_> = (0..10u32)
        .map(|n| {
            let connect = bench.connect();
            tokio::spawn(async move {
                let client = connect.await;
                for round in 0..20 {
                    assert_eq!(
                        client.echo(n * 100 + round).call().await.unwrap(),
                        n * 100 + round
                    );
                }
                // longer than a datagram: split across several both ways
                assert_eq!(client.big(3000).call().await.unwrap(), vec![7; 3000]);
                client.whoami().call().await.unwrap()
            })
        })
        .collect();
    let mut ids = vec![];
    for client in clients {
        ids.push(soon("a client", client).await.unwrap());
    }
    ids.sort_unstable();
    assert_eq!(ids, (0..10).collect::<Vec<_>>(), "a connection per host");
    for connection in bench.shared.connections.lock().unwrap().iter() {
        let Peer::Addr(peer) = connection.peer else {
            panic!("a UDP peer is an address: {:?}", connection.peer);
        };
        assert!(peer.ip().is_loopback());
        assert_ne!(peer.port(), addr.port());
    }
}

/// A host that goes quiet is dropped after the link's peer timeout, which frees its slot: a new host gets it
/// without taking one from a host still served.
#[tokio::test(start_paused = true)]
async fn a_host_that_goes_quiet_frees_its_slot() {
    let path = "host_server/udp_quiet";
    let socket = Muting::new(path);
    let bench = simulated::<2>(path, socket.clone());
    let quiet = bench.connect().await;
    socket.mute_heard();
    let served = bench.connect().await;
    assert_eq!(served.whoami().call().await.unwrap(), 1);
    live(&bench, 2).await;

    tokio::time::sleep(Duration::from_millis(
        2 * ww_device::ww_link::PEER_TIMEOUT_MS,
    ))
    .await;
    soon("the quiet host's connection ends", live(&bench, 1)).await;
    let next = bench.connect().await;
    assert_eq!(next.whoami().call().await.unwrap(), 2);
    assert_eq!(served.whoami().call().await.unwrap(), 1);
    live(&bench, 2).await;
    drop(quiet);
}

/// With every slot taken, a new host ends the connection heard from least recently and is served from its next
/// hello; the other connection goes on.
#[tokio::test(start_paused = true)]
async fn a_full_table_gives_the_quietest_slot_to_a_new_host() {
    let path = "host_server/udp_full";
    let bench = simulated::<2>(path, Muting::new(path));
    let first = bench.connect().await;
    let second = bench.connect().await;
    assert_eq!(first.whoami().call().await.unwrap(), 0);
    assert_eq!(second.whoami().call().await.unwrap(), 1);

    let third = soon("the third host", bench.connect()).await;
    assert_eq!(third.whoami().call().await.unwrap(), 2);
    assert_eq!(second.whoami().call().await.unwrap(), 1);
    assert!(first.whoami().call().await.is_err(), "the first was ended");
    live(&bench, 2).await;
}

/// Shutdown ends every connection and then the server; the slots are freed with them.
#[tokio::test(start_paused = true)]
async fn shutdown_ends_every_connection() {
    let path = "host_server/udp_shutdown";
    let mut bench = simulated::<4>(path, Muting::new(path));
    let clients = [
        bench.connect().await,
        bench.connect().await,
        bench.connect().await,
    ];
    for (n, client) in clients.iter().enumerate() {
        assert_eq!(client.whoami().call().await.unwrap(), n as u32);
    }
    bench.shut_down();
    soon("the server ends", &mut bench.served).await.unwrap();
    live(&bench, 0).await;
    for client in &clients {
        assert!(client.echo(1).call().await.is_err());
    }
}

/// Connections on all the runtime's threads take turns receiving from the one socket: under load every request is
/// answered. Each round trip passes the receiving role between threads, so a role ended twice (once by its holder,
/// once more after another thread took it) would let two receive at once and lose a handed-over datagram.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connections_on_many_threads_share_the_socket() {
    let path = "host_server/udp_threads";
    let bench = simulated::<16>(path, Muting::new(path));
    let clients: Vec<_> = (0..10u32)
        .map(|n| {
            let connect = bench.connect();
            tokio::spawn(async move {
                let client = connect.await;
                for round in 0..100 {
                    assert_eq!(
                        client.echo(n * 1000 + round).call().await.unwrap(),
                        n * 1000 + round
                    );
                }
            })
        })
        .collect();
    for client in clients {
        soon("a client", client).await.unwrap();
    }
}

/// A stopped listener (SRV-75) accepts no new host and ends no connection for one, even with every slot taken; the
/// server ends once its last connection ended.
#[tokio::test(start_paused = true)]
async fn a_stopped_listener_keeps_its_connections_and_takes_no_new_host() {
    let path = "host_server/udp_stop";
    let listener = UdpListener::<_, 1>::new(Muting::new(path), MESSAGE_LEN_MAX);
    let stop = listener.stop_handle();
    let mut bench = start_bench!(listener, Reach::InProcess(path.to_owned()), small(), true);
    let first = bench.connect().await;
    assert_eq!(first.whoami().call().await.unwrap(), 0);
    stop.stop();

    let refused = tokio::time::timeout(Duration::from_secs(5), bench.connect()).await;
    assert!(refused.is_err(), "a new host is not served");
    assert_eq!(first.whoami().call().await.unwrap(), 0);
    drop(first);
    soon("the server ends", &mut bench.served).await.unwrap();
    assert_eq!(bench.shared.connections.lock().unwrap().len(), 1);
}
