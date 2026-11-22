//! Clients on TCP (`ClientConfig::tcp`), each known by its address only.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::stream::Stream;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use wire_weaver::Peer;

use super::{ACCEPT_RETRY_DELAY, is_transient};

/// The connections of a TCP listener, as a source for [listen](super::listen). Running out of descriptors or memory
/// is waited out, not an error; a client whose stream fails its setup is dropped.
pub struct TcpIncoming {
    listener: TcpListener,
    retry: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl TcpIncoming {
    /// Listen on `addr`.
    ///
    /// # Errors
    ///
    /// Binding failed.
    pub async fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
        Ok(Self::new(TcpListener::bind(addr).await?))
    }

    /// Accept on `listener`.
    pub fn new(listener: TcpListener) -> Self {
        TcpIncoming {
            listener,
            retry: None,
        }
    }

    /// The address it listens on, with the port the system picked for port 0.
    ///
    /// # Errors
    ///
    /// The system doesn't tell.
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }
}

impl Stream for TcpIncoming {
    type Item = io::Result<(Compat<TcpStream>, Peer)>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(retry) = self.retry.as_mut() {
                std::task::ready!(retry.as_mut().poll(cx));
                self.retry = None;
            }
            match self.listener.poll_accept(cx) {
                Poll::Ready(Ok((stream, addr))) => {
                    // messages are small and flushed on purpose: Nagle would only delay them. A stream that can't
                    // take it is about that one client: it is dropped, the others go on
                    if stream.set_nodelay(true).is_ok() {
                        return Poll::Ready(Some(Ok((stream.compat(), Peer::Addr(addr)))));
                    }
                }
                Poll::Ready(Err(e)) if is_transient(&e) => {
                    self.retry = Some(Box::pin(tokio::time::sleep(ACCEPT_RETRY_DELAY)));
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
