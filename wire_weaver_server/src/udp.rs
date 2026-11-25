//! A listener on a UDP socket (SRV-69): a connection per host, by its address, on a `ww_device::udp` peer table.
//!
//! Hosts are told apart by their address only, which proves nothing (SRV-46 builds policy on top). A host gets a
//! connection when it starts link setup and keeps it until its link goes down: it leaves, or goes quiet for the
//! link's peer timeout. Up to `PEERS` hosts are served at once; with every slot taken, a new host ends the connection
//! heard from least recently and is served from its next hello, as `ww_device::udp` describes.

use std::future::poll_fn;
use std::net::SocketAddr;
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};

use futures_util::future::{Either, select};
use futures_util::task::AtomicWaker;
use wire_weaver::Peer;
use ww_device::udp::{DatagramSocket, LockedUdpPeers, Readiness, StdUdpSocket, UdpSink, UdpSource};
use ww_device::ww_link::UDP_MAX_DATAGRAM_LEN;
use ww_device::{OwnedFramedRx, OwnedFramedTx};

use crate::listener::{Accept, Accepted, Listener};

/// The peer table a [UdpListener] shares with its connections.
type Peers<S, const PEERS: usize> = Arc<LockedUdpPeers<S, PEERS>>;

/// Sends a connection's messages to its host, in datagrams.
pub type UdpTx<S, const PEERS: usize> = OwnedFramedTx<UdpSink<Peers<S, PEERS>>>;
/// Receives a connection's messages from its host's datagrams.
pub type UdpRx<S, const PEERS: usize> = OwnedFramedRx<UdpSource<Peers<S, PEERS>>>;

/// What a connection's peer is, from the address of its host.
pub trait PeerAddr {
    /// The peer of the host at this address.
    fn peer(&self) -> Peer;
}

impl PeerAddr for SocketAddr {
    fn peer(&self) -> Peer {
        Peer::Addr(*self)
    }
}

/// A host of a simulated UDP device is in the same process.
#[cfg(feature = "in_process")]
impl PeerAddr for wire_weaver_client::in_process::UdpAddr {
    fn peer(&self) -> Peer {
        Peer::Local
    }
}

/// Connections of up to `PEERS` hosts on one datagram socket, see the [module docs](self).
///
/// ```ignore
/// let listener = UdpListener::<_, 8>::bind("0.0.0.0:9000".parse()?, 4096)?; // on tokio, feature `tokio`
/// server.serve(link, listener, make, shutdown).await?;
/// ```
///
/// On another event loop, give [UdpListener::new] a [StdUdpSocket] with that loop's readiness (`ww_calloop::bind_udp`
/// on calloop).
///
/// Its connections are `Send` when the socket is `Sync`, so [Server::accepting](crate::Server::accepting) can spawn
/// them on a multi-threaded runtime.
pub struct UdpListener<S: DatagramSocket, const PEERS: usize> {
    peers: Peers<S, PEERS>,
    /// What the listener receives into while no connection does
    buf: Box<[u8; UDP_MAX_DATAGRAM_LEN]>,
    message_len_max: usize,
    stop: Arc<Stop>,
}

/// Whether a [UdpListener] was stopped, and who to wake when it is.
#[derive(Default)]
struct Stop {
    stopped: AtomicBool,
    waker: AtomicWaker,
}

impl Stop {
    fn poll_stopped(&self, cx: &mut Context<'_>) -> Poll<()> {
        self.waker.register(cx.waker());
        if self.stopped.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// Stops one [UdpListener]: see [UdpListener::stop_handle].
pub struct UdpStop<S: DatagramSocket, const PEERS: usize> {
    /// Weak, so that a handle kept after the server ended holds no socket open.
    peers: Weak<LockedUdpPeers<S, PEERS>>,
    stop: Arc<Stop>,
}

impl<S: DatagramSocket, const PEERS: usize> Clone for UdpStop<S, PEERS> {
    fn clone(&self) -> Self {
        UdpStop {
            peers: self.peers.clone(),
            stop: self.stop.clone(),
        }
    }
}

impl<S: DatagramSocket, const PEERS: usize> UdpStop<S, PEERS> {
    /// Accept no more hosts: [accept](Listener::accept) gives `None` from now on, so the server ends once its
    /// connections ended, and a new host ends none of them. The socket closes with the last connection.
    pub fn stop(&self) {
        self.stop.stopped.store(true, Ordering::Release);
        if let Some(peers) = self.peers.upgrade() {
            peers.stop_accepting();
        }
        self.stop.waker.wake();
    }
}

impl<S: DatagramSocket, const PEERS: usize> UdpListener<S, PEERS> {
    /// Listen on `socket`; `message_len_max` is the longest message the server takes, longer ones than a datagram
    /// are split across several.
    ///
    /// # Panics
    ///
    /// At compile time: `PEERS` is 0.
    pub fn new(socket: S, message_len_max: usize) -> Self {
        UdpListener {
            peers: Arc::new(LockedUdpPeers::new(socket)),
            buf: Box::new([0u8; UDP_MAX_DATAGRAM_LEN]),
            message_len_max,
            stop: Arc::default(),
        }
    }

    /// A handle that stops this listener alone, for a program that moves to another address while it runs: the
    /// connections the listener has go on, on the same socket. The handle is `Send` and `Sync` when the socket is.
    pub fn stop_handle(&self) -> UdpStop<S, PEERS> {
        UdpStop {
            peers: Arc::downgrade(&self.peers),
            stop: self.stop.clone(),
        }
    }

    /// The socket listened on.
    pub fn socket(&self) -> &S {
        self.peers.socket()
    }
}

impl<R: Readiness, const PEERS: usize> UdpListener<StdUdpSocket<R>, PEERS> {
    /// The address listened on, e.g. to learn the port after binding port 0.
    ///
    /// # Errors
    ///
    /// The system can't tell.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket().socket().local_addr()
    }
}

#[cfg(feature = "tokio")]
impl<const PEERS: usize> UdpListener<StdUdpSocket<TokioReadiness>, PEERS> {
    /// Listen on a UDP socket bound to `addr`, on tokio's event loop; `message_len_max` as in [Self::new].
    ///
    /// # Errors
    ///
    /// Binding the socket failed, or tokio can't watch it (no runtime is running).
    pub fn bind(addr: SocketAddr, message_len_max: usize) -> std::io::Result<Self> {
        let socket = std::net::UdpSocket::bind(addr)?;
        let readiness = TokioReadiness::new(&socket)?;
        Ok(Self::new(
            StdUdpSocket::new(socket, readiness)?,
            message_len_max,
        ))
    }
}

/// When a [StdUdpSocket] is readable, from tokio watching a clone of it, which is never read itself.
#[cfg(feature = "tokio")]
pub struct TokioReadiness(tokio::net::UdpSocket);

#[cfg(feature = "tokio")]
impl TokioReadiness {
    /// Watches a clone of `socket`, for a [StdUdpSocket] on it. Makes the socket nonblocking.
    ///
    /// # Errors
    ///
    /// The socket can't be cloned, or tokio can't watch it (no runtime is running).
    pub fn new(socket: &std::net::UdpSocket) -> std::io::Result<Self> {
        let watched = socket.try_clone()?;
        watched.set_nonblocking(true)?;
        Ok(Self(tokio::net::UdpSocket::from_std(watched)?))
    }
}

#[cfg(feature = "tokio")]
impl Readiness for TokioReadiness {
    async fn readable(&self) -> std::io::Result<()> {
        self.0.readable().await?;
        // the clone is read, not this one: forget the readiness before that read, so that the next wait is for a
        // datagram that comes after it
        let cleared = self.0.try_io(tokio::io::Interest::READABLE, || {
            Err::<(), _>(std::io::ErrorKind::WouldBlock.into())
        });
        debug_assert!(cleared.is_err(), "the closure only clears readiness");
        Ok(())
    }
}

impl<S, const PEERS: usize> Listener for UdpListener<S, PEERS>
where
    S: DatagramSocket<Addr: PeerAddr>,
{
    type Tx = UdpTx<S, PEERS>;
    type Rx = UdpRx<S, PEERS>;
    type Error = S::RecvError;

    /// Cancel-safe, as `UdpPeers::accept`. `None` once [stopped](UdpStop::stop).
    async fn accept(&mut self) -> Accept<Self::Tx, Self::Rx, Self::Error> {
        let stopped = pin!(poll_fn(|cx| self.stop.poll_stopped(cx)));
        let accepted = pin!(self.peers.accept(&mut self.buf));
        let (slot, from) = match select(stopped, accepted).await {
            Either::Left(((), _)) => {
                // again: an accept that started beside the stop listened once more
                self.peers.stop_accepting();
                return Ok(None);
            }
            Either::Right((accepted, _)) => accepted?,
        };
        // the source first: it holds the slot for the connection, and adopts the host when the connection starts
        let rx = OwnedFramedRx::owned(
            UdpSource::new(self.peers.clone(), slot),
            self.message_len_max,
        );
        let tx = OwnedFramedTx::owned(UdpSink::new(self.peers.clone(), slot), UDP_MAX_DATAGRAM_LEN);
        Ok(Some(Accepted {
            tx,
            rx,
            peer: from.peer(),
        }))
    }
}
