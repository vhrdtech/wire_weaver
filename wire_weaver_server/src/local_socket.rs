//! Clients on a local socket (TR-16): programs of the same user on this PC, which connect with
//! `ClientConfig::ipc_path`. A Unix domain socket on Linux and macOS, a named pipe on Windows; the socket comes from
//! [ww_local_socket::listen] or, under systemd socket activation, [ww_local_socket::inherited].
//!
//! [LocalSocketIncoming] is a source for [listen](crate::byte_stream::listen) with
//! [PreambleFraming](crate::byte_stream::PreambleFraming): any device key names the API served there, the hub's own
//! API is refused, and further APIs are named services ([StreamListener::service](
//! crate::byte_stream::StreamListener::service)).

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_util::stream::Stream;
use wire_weaver::Peer;

use crate::byte_stream::is_transient;

type Accepting = Pin<Box<dyn Future<Output = io::Result<(ww_local_socket::Stream, Peer)>> + Send>>;

/// The connections of a local socket, each with the process on the other side as its peer.
///
/// ```ignore
/// let socket = match ww_local_socket::inherited(None, PeerPolicy::SameUser) {
///     Ok(socket) => socket,
///     Err(ActivationError::NotActivated) => ww_local_socket::listen(&path, false)?,
///     Err(e) => return Err(e.into()),
/// };
/// let listener = listen(LocalSocketIncoming::new(socket), PreambleFraming::new("thermo 0.3.0", 4096), clock);
/// ```
pub struct LocalSocketIncoming {
    socket: Arc<ww_local_socket::Listener>,
    accepting: Option<Accepting>,
    retry: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl LocalSocketIncoming {
    /// Accept clients on `socket`.
    pub fn new(socket: ww_local_socket::Listener) -> Self {
        LocalSocketIncoming {
            socket: Arc::new(socket),
            accepting: None,
            retry: None,
        }
    }
}

impl Stream for LocalSocketIncoming {
    type Item = io::Result<(ww_local_socket::Stream, Peer)>;

    /// Running out of descriptors or memory is waited out, not an error.
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(retry) = self.retry.as_mut() {
                std::task::ready!(retry.as_mut().poll(cx));
                self.retry = None;
            }
            let socket = self.socket.clone();
            let accepting = self.accepting.get_or_insert_with(|| {
                Box::pin(async move {
                    let (stream, peer) = socket.accept().await?;
                    Ok((stream, peer_of(peer)))
                })
            });
            let accepted = std::task::ready!(accepting.as_mut().poll(cx));
            self.accepting = None;
            match accepted {
                Err(e) if is_transient(&e) => {
                    self.retry = Some(Box::pin(tokio::time::sleep(
                        crate::byte_stream::ACCEPT_RETRY_DELAY,
                    )));
                }
                accepted => return Poll::Ready(Some(accepted)),
            }
        }
    }
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
