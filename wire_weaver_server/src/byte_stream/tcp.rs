//! Clients on TCP (`ClientConfig::tcp`), each known by its address only.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::stream::Stream;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use wire_weaver::Peer;

/// How long a source waits before it accepts again after the system ran out of descriptors or memory.
pub(crate) const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

/// The connections of a TCP listener, as a source for [listen](super::listen). Running out of descriptors or memory
/// is waited out, not an error.
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
                    // messages are small and flushed on purpose: Nagle would only delay them
                    stream.set_nodelay(true)?;
                    return Poll::Ready(Some(Ok((stream.compat(), Peer::Addr(addr)))));
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

/// The system ran out of something a later accept may find again.
pub(crate) fn is_transient(e: &io::Error) -> bool {
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
