//! A listener on a local socket (TR-16): programs of the same user on this PC, which connect with
//! `ClientConfig::ipc_path`. A Unix domain socket on Linux and macOS, a named pipe on Windows; the socket comes from
//! [ww_local_socket::listen] or, under systemd socket activation, [ww_local_socket::inherited].
//!
//! Every connection starts with the hub preamble: any device key names the API served here, the hub's own API is
//! refused. Preambles are read side by side, so a client that sends none holds up nobody.

use std::future::{Future, poll_fn};
use std::io;
use std::pin::{Pin, pin};
use std::task::Poll;
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use wire_weaver::Peer;
use wire_weaver_client::local_socket::device::{
    DeviceRx, DeviceTx, PREAMBLE_TIMEOUT, answer_preamble, connection_medium,
};

use crate::listener::{Accepted, Listener};

/// Connections whose preamble is read at once. Past it the listener stops accepting until one is answered.
pub const HANDSHAKES_MAX: usize = 16;
/// How long the listener waits before it accepts again after the system ran out of descriptors or memory.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

type Handshake = Pin<Box<dyn Future<Output = Option<Accepted<DeviceTx, DeviceRx>>> + Send>>;

/// Clients on a local socket.
///
/// ```ignore
/// let socket = match ww_local_socket::inherited(None) {
///     Ok(socket) => socket,
///     Err(ActivationError::NotActivated) => ww_local_socket::listen(&path, false)?,
///     Err(e) => return Err(e.into()),
/// };
/// let listener = LocalSocketListener::new(socket, "thermo 0.3.0", 4096);
/// ```
pub struct LocalSocketListener {
    socket: ww_local_socket::Listener,
    version: String,
    message_len_max: usize,
    handshakes: FuturesUnordered<Handshake>,
}

impl LocalSocketListener {
    /// Accept clients on `socket`. `version` goes into the preamble answer (a refused client shows it),
    /// `message_len_max` is the longest message the server takes.
    pub fn new(socket: ww_local_socket::Listener, version: &str, message_len_max: usize) -> Self {
        LocalSocketListener {
            socket,
            version: version.to_owned(),
            message_len_max,
            handshakes: FuturesUnordered::new(),
        }
    }
}

/// What [LocalSocketListener::accept] waits for.
enum Step {
    Answered(Option<Box<Accepted<DeviceTx, DeviceRx>>>),
    Connected(io::Result<(ww_local_socket::Stream, ww_local_socket::Peer)>),
}

impl Listener for LocalSocketListener {
    type Tx = DeviceTx;
    type Rx = DeviceRx;
    type Error = io::Error;

    /// Cancel-safe: connections whose preamble is being read stay in the listener.
    ///
    /// # Errors
    ///
    /// Accepting failed for good. Running out of descriptors or memory is waited out instead.
    async fn accept(&mut self) -> io::Result<Option<Accepted<DeviceTx, DeviceRx>>> {
        loop {
            let LocalSocketListener {
                socket, handshakes, ..
            } = self;
            let accepting = handshakes.len() < HANDSHAKES_MAX;
            let mut connected = pin!(socket.accept());
            let step = poll_fn(|cx| {
                if let Poll::Ready(Some(answered)) = handshakes.poll_next_unpin(cx) {
                    return Poll::Ready(Step::Answered(answered.map(Box::new)));
                }
                if accepting && let Poll::Ready(r) = connected.as_mut().poll(cx) {
                    return Poll::Ready(Step::Connected(r));
                }
                Poll::Pending
            })
            .await;
            match step {
                Step::Answered(Some(accepted)) => return Ok(Some(*accepted)),
                Step::Answered(None) => {}
                Step::Connected(Ok((stream, peer))) => {
                    let version = self.version.clone();
                    let len_max = self.message_len_max;
                    self.handshakes
                        .push(Box::pin(handshake(stream, peer, version, len_max)));
                }
                Step::Connected(Err(e)) if is_transient(&e) => {
                    tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                }
                Step::Connected(Err(e)) => return Err(e),
            }
        }
    }
}

/// Answer the preamble of one client: its connection when it is served.
async fn handshake(
    stream: ww_local_socket::Stream,
    peer: ww_local_socket::Peer,
    version: String,
    message_len_max: usize,
) -> Option<Accepted<DeviceTx, DeviceRx>> {
    let answered = tokio::time::timeout(PREAMBLE_TIMEOUT, answer_preamble(stream, &version)).await;
    // no preamble in time, a bad one, one for something else, or the client left
    let Ok(Ok(Some((rx, tx)))) = answered else {
        return None;
    };
    // the server sees the client leave from its link going down
    let (tx, rx, _ended) = connection_medium(rx, tx, message_len_max);
    Some(Accepted {
        tx,
        rx,
        peer: peer_of(peer),
    })
}

/// The peer as handlers see it.
#[cfg(unix)]
fn peer_of(peer: ww_local_socket::Peer) -> Peer {
    use wire_weaver::{Gid, Pid, Uid};
    Peer::Unix {
        uid: Uid(peer.uid),
        gid: Gid(peer.gid),
        pid: peer.pid.map(Pid),
    }
}

/// The peer as handlers see it: the pipe's client was checked to run as this user, which [Peer] has no word for
/// yet (SRV-46).
#[cfg(windows)]
fn peer_of(_peer: ww_local_socket::Peer) -> Peer {
    Peer::Unknown
}

/// The system ran out of something a later accept may find again.
fn is_transient(e: &io::Error) -> bool {
    #[cfg(unix)]
    if let Some(code) = e.raw_os_error()
        && [libc::EMFILE, libc::ENFILE, libc::ENOBUFS, libc::ENOMEM].contains(&code)
    {
        return true;
    }
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionAborted | io::ErrorKind::Interrupted | io::ErrorKind::OutOfMemory
    )
}
