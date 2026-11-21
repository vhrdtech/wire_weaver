//! TCP clients accepted on the loop, as a byte-stream source.

use std::cell::{Cell, RefCell};
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::pin::{Pin, pin};
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker, ready};

use calloop::LoopHandle;
use calloop::io::Async;
use calloop::timer::TimeoutFuture;
use futures_util::stream::Stream;
use wire_weaver::Peer;
use wire_weaver_server::byte_stream::{ACCEPT_RETRY_DELAY, is_transient};

/// The connections of a TCP listener the loop watches, as a source for
/// [listen](wire_weaver_server::byte_stream::listen); each client is known by its address. Running out of
/// descriptors or memory is waited out, not an error; a client whose stream fails its setup is dropped, the others go
/// on. [stop_handle](Self::stop_handle) closes the socket while the program runs.
pub struct TcpIncoming<'l, D> {
    handle: LoopHandle<'l, D>,
    socket: Rc<Socket<'l>>,
    addr: SocketAddr,
    retry: Option<TimeoutFuture>,
}

/// The listening socket, shared with the stop handles.
struct Socket<'l> {
    /// `None` once stopped.
    listener: RefCell<Option<Async<'l, TcpListener>>>,
    /// The task that polls the source, woken when it is stopped.
    waker: Cell<Option<Waker>>,
}

/// Stops one [TcpIncoming]: see [TcpIncoming::stop_handle].
#[derive(Clone)]
pub struct TcpStop<'l>(Weak<Socket<'l>>);

impl TcpStop<'_> {
    /// Close the listening socket now, so its address is free to bind again. The source ends: the server takes no
    /// new clients and ends once the connections it has ended; they go on until then. Call it on the loop's thread
    /// outside the source's own poll, as any callback of the loop is.
    ///
    /// # Panics
    ///
    /// Called from inside the source's own poll.
    pub fn stop(&self) {
        let Some(socket) = self.0.upgrade() else {
            return;
        };
        let listener = socket
            .listener
            .try_borrow_mut()
            .expect("a stop from outside the source's poll")
            .take();
        drop(listener);
        if let Some(waker) = socket.waker.take() {
            waker.wake();
        }
    }
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
            socket: Rc::new(Socket {
                listener: RefCell::new(Some(handle.adapt_io(listener)?)),
                waker: Cell::new(None),
            }),
            addr,
            retry: None,
        })
    }

    /// The address it listens on, with the port the system picked for port 0.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// A handle that closes this source's socket alone, for a program that moves to another address while it
    /// runs: the connections it accepted go on. A handle kept after the source is gone does nothing.
    pub fn stop_handle(&self) -> TcpStop<'l> {
        TcpStop(Rc::downgrade(&self.socket))
    }
}

impl<'l, D> Stream for TcpIncoming<'l, D> {
    type Item = io::Result<(Async<'l, TcpStream>, Peer)>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.socket.waker.set(Some(cx.waker().clone()));
        let mut listener = this.socket.listener.borrow_mut();
        let Some(listener) = listener.as_mut() else {
            return Poll::Ready(None);
        };
        loop {
            if let Some(retry) = this.retry.as_mut() {
                ready!(Pin::new(retry).poll(cx));
                this.retry = None;
            }
            match listener.get_mut().accept() {
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
                    ready!(pin!(listener.readable()).poll(cx));
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
