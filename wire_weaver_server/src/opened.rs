//! Connections the program opened itself and hands to a server: a program that answers each client's preamble
//! itself and serves several things on one socket (the ww hub: its own API, or a tunnel to a device) hands the
//! connections of its API to [Server::serve](crate::Server::serve) here.

use std::convert::Infallible;
use std::fmt;
use std::future::poll_fn;

use futures_channel::mpsc;
use futures_util::stream::StreamExt;
use wire_weaver::Peer;
use ww_device::{MessageRx, MessageTx};

use crate::listener::{Accept, Accepted, Listener};

/// A listener of connections handed in through its [OpenedSender]s, holding up to `queued_max` of them not yet
/// taken by its server. It ends once every sender is dropped: the server then ends after its connections.
pub fn opened<Tx, Rx>(queued_max: usize) -> (OpenedSender<Tx, Rx>, OpenedListener<Tx, Rx>) {
    let (tx, rx) = mpsc::channel(queued_max);
    (OpenedSender(tx), OpenedListener(rx))
}

/// Hands opened connections to an [OpenedListener].
pub struct OpenedSender<Tx, Rx>(mpsc::Sender<Accepted<Tx, Rx>>);

impl<Tx, Rx> Clone for OpenedSender<Tx, Rx> {
    fn clone(&self) -> Self {
        OpenedSender(self.0.clone())
    }
}

/// The listener is gone: the connection's halves come back.
pub struct Closed<Tx, Rx>(pub Tx, pub Rx);

impl<Tx, Rx> fmt::Debug for Closed<Tx, Rx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the listener is gone")
    }
}

impl<Tx, Rx> OpenedSender<Tx, Rx> {
    /// Serve the connection of `tx` and `rx`, whose other side is `peer`: the transport's word for it, never what
    /// the client says about itself. The connection is open already (its client's opening answered), so the server
    /// starts with link setup. Waits while the listener holds as many connections as it takes.
    ///
    /// # Errors
    ///
    /// The listener was dropped.
    pub async fn serve(&mut self, tx: Tx, rx: Rx, peer: Peer) -> Result<(), Closed<Tx, Rx>> {
        if poll_fn(|cx| self.0.poll_ready(cx)).await.is_err() {
            return Err(Closed(tx, rx));
        }
        self.0.try_send(Accepted { tx, rx, peer }).map_err(|e| {
            let accepted = e.into_inner();
            Closed(accepted.tx, accepted.rx)
        })
    }
}

/// The connections handed in, as a server's listener.
pub struct OpenedListener<Tx, Rx>(mpsc::Receiver<Accepted<Tx, Rx>>);

impl<Tx: MessageTx, Rx: MessageRx> Listener for OpenedListener<Tx, Rx> {
    type Tx = Tx;
    type Rx = Rx;
    type Error = Infallible;

    /// Cancel-safe: a connection stays in the queue until it is returned.
    async fn accept(&mut self) -> Accept<Tx, Rx, Infallible> {
        Ok(self.0.next().await)
    }
}
