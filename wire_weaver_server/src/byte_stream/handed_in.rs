//! Streams the program accepted itself (an iroh bi-stream, a socket from its own accept loop), handed to a listener
//! one by one with the peer the program vouches for.

use std::convert::Infallible;
use std::fmt;
use std::future::poll_fn;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_channel::mpsc;
use futures_util::stream::{Stream, StreamExt};
use wire_weaver::Peer;

/// A source of streams handed in through its [StreamSender]s, holding up to `queued_max` of them not yet taken:
/// [listen](super::listen) on it. It ends once every sender is dropped.
pub fn handed_in<S>(queued_max: usize) -> (StreamSender<S>, HandedIn<S>) {
    let (tx, rx) = mpsc::channel(queued_max);
    (StreamSender(tx), HandedIn(rx))
}

/// Hands streams to a listener.
pub struct StreamSender<S>(mpsc::Sender<(S, Peer)>);

impl<S> Clone for StreamSender<S> {
    fn clone(&self) -> Self {
        StreamSender(self.0.clone())
    }
}

/// The listener is gone: the stream comes back.
pub struct Gone<S>(pub S);

impl<S> fmt::Debug for Gone<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the listener is gone")
    }
}

impl<S> StreamSender<S> {
    /// Serve `stream`, whose other side is `peer`: the transport's word for it, never what the client says about
    /// itself. Waits while the listener holds as many streams as it takes.
    ///
    /// # Errors
    ///
    /// The listener was dropped.
    pub async fn serve_stream(&mut self, stream: S, peer: Peer) -> Result<(), Gone<S>> {
        if poll_fn(|cx| self.0.poll_ready(cx)).await.is_err() {
            return Err(Gone(stream));
        }
        self.0
            .try_send((stream, peer))
            .map_err(|e| Gone(e.into_inner().0))
    }
}

/// The streams handed in, as a listener's source.
pub struct HandedIn<S>(mpsc::Receiver<(S, Peer)>);

impl<S> Stream for HandedIn<S> {
    type Item = Result<(S, Peer), Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_next_unpin(cx).map(|s| s.map(Ok))
    }
}
