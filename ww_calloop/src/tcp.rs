//! TCP clients accepted on the loop, as a byte-stream source.

use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::pin::{Pin, pin};
use std::task::{Context, Poll, ready};

use calloop::LoopHandle;
use calloop::io::Async;
use calloop::timer::TimeoutFuture;
use futures_util::stream::Stream;
use wire_weaver::Peer;
use wire_weaver_server::byte_stream::{ACCEPT_RETRY_DELAY, is_transient};

/// The connections of a TCP listener the loop watches, as a source for
/// [listen](wire_weaver_server::byte_stream::listen); each client is known by its address. Running out of
/// descriptors or memory is waited out, not an error; a client whose stream fails its setup is dropped, the others go
/// on.
pub struct TcpIncoming<'l, D> {
    handle: LoopHandle<'l, D>,
    listener: Async<'l, TcpListener>,
    addr: SocketAddr,
    retry: Option<TimeoutFuture>,
}

impl<'l, D> TcpIncoming<'l, D> {
    /// Listen on `addr`, on the loop of `handle`.
    ///
    /// # Errors
    ///
    /// Binding failed, or the loop can't watch the socket.
    pub fn bind(handle: &LoopHandle<'l, D>, addr: impl ToSocketAddrs) -> io::Result<Self> {
        Self::new(handle, TcpListener::bind(addr)?)
    }

    /// Accept on `listener` (made nonblocking here), on the loop of `handle`.
    ///
    /// # Errors
    ///
    /// The system doesn't tell the listener's address, or the loop can't watch it.
    pub fn new(handle: &LoopHandle<'l, D>, listener: TcpListener) -> io::Result<Self> {
        let addr = listener.local_addr()?;
        Ok(TcpIncoming {
            handle: handle.clone(),
            listener: handle.adapt_io(listener)?,
            addr,
            retry: None,
        })
    }

    /// The address it listens on, with the port the system picked for port 0.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
}

impl<'l, D> Stream for TcpIncoming<'l, D> {
    type Item = io::Result<(Async<'l, TcpStream>, Peer)>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(retry) = this.retry.as_mut() {
                ready!(Pin::new(retry).poll(cx));
                this.retry = None;
            }
            match this.listener.get_mut().accept() {
                Ok((stream, addr)) => {
                    // messages are small and flushed on purpose: Nagle would only delay them. A stream that can't
                    // take it, or that the loop can't watch, is about that one client: it is dropped
                    if stream.set_nodelay(true).is_ok()
                        && let Ok(stream) = this.handle.adapt_io(stream)
                    {
                        return Poll::Ready(Some(Ok((stream, Peer::Addr(addr)))));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    ready!(pin!(this.listener.readable()).poll(cx));
                }
                Err(e) if is_transient(&e) => {
                    this.retry = Some(TimeoutFuture::from_duration(
                        &this.handle,
                        ACCEPT_RETRY_DELAY,
                    ));
                }
                Err(e) => return Poll::Ready(Some(Err(e))),
            }
        }
    }
}
