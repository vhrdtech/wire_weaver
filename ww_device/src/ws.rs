//! WebSocket as a medium: a server accepts one client at a time on a TCP-like [Socket], completes the HTTP
//! upgrade and then carries each `ww_link` message as one binary WebSocket message, `| kind: u8 | payload ... |`,
//! the same format the `ws` transport of `wire_weaver_client` speaks.
//!
//! WebSocket has its own framing over a reliable stream, so `ww_framer` is not used. Frame headers and the
//! handshake come from `edge-ws` and `edge-http` (their sans-IO parts only), the IO stays here so that every
//! partially received or written state lives in [WsRx] / [WsTx], which keeps [WsRx::wait_message] and
//! [WsRx::wait_connected] cancel-safe.
//!
//! Supported: unfragmented binary messages, Ping and Pong are ignored (the host doesn't send them, `ww_link` pings
//! keep the link alive), Close, Text and fragmented messages drop the connection. Any request path is accepted,
//! a request that is not a WebSocket upgrade gets `400 Bad Request`.
//!
//! Tx and Rx share the socket through a [WsConnection] (an async mutex, they are never used at the same time by
//! [Server]). With the `embassy-net` feature, [EmbassyNetSocket] implements [Socket] on an `embassy-net` TCP socket,
//! [ws_server] puts everything together.

use core::future::Future;

use edge_ws::{FrameHeader, FrameType};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;

use crate::fmt::{debug, warn};
use crate::server::Clock;
use crate::transport::{MessageRx, MessageTx};
use crate::{LinkConfig, Server};

/// TCP-like byte stream that a server accepts clients on, one at a time.
pub trait Socket {
    type Error;

    /// Drop the current client, if any, and wait for the next one. Must be cancel-safe (e.g., by dropping a
    /// client that was half accepted).
    fn accept(&mut self) -> impl Future<Output = ()>;

    /// Read at least one byte into `buf`, `Ok(0)` means the client closed the connection. Must be cancel-safe.
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<usize, Self::Error>>;

    /// Write some of the bytes, returns how many. Should fail eventually if the client does not read, so that
    /// the device notices a host that is gone without disconnecting (e.g., a TCP timeout).
    fn write(&mut self, bytes: &[u8]) -> impl Future<Output = Result<usize, Self::Error>>;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum WsError<E> {
    Socket(E),
    /// Client closed the connection, or there is no open connection
    Closed,
    /// Malformed frame, or a frame type that is not supported (text, fragmented)
    Protocol,
    /// Message does not fit into the buffer
    TooLong,
}

/// Socket shared by [WsTx] and [WsRx].
pub struct WsConnection<S> {
    inner: Mutex<NoopRawMutex, Inner<S>>,
}

struct Inner<S> {
    socket: S,
    /// Handshake done and no error since, Tx only writes while true
    open: bool,
}

impl<S: Socket> WsConnection<S> {
    pub const fn new(socket: S) -> Self {
        WsConnection {
            inner: Mutex::new(Inner {
                socket,
                open: false,
            }),
        }
    }
}

/// Server side frame header and the kind byte at most, see [WsBuffers].
pub const WS_OVERHEAD: usize = FrameHeader::MAX_LEN + 1;

/// [MessageTx] over a [WsConnection]: one binary message per `ww_link` message, held back in `buf` until it is full
/// or flushed, so that small messages share TCP segments.
pub struct WsTx<'a, S> {
    conn: &'a WsConnection<S>,
    buf: &'a mut [u8],
    len: usize,
}

impl<'a, S: Socket> WsTx<'a, S> {
    /// `buf` must fit the longest message plus [WS_OVERHEAD].
    pub fn new(conn: &'a WsConnection<S>, buf: &'a mut [u8]) -> Self {
        WsTx { conn, buf, len: 0 }
    }
}

impl<S: Socket> MessageTx for WsTx<'_, S> {
    type Error = WsError<S::Error>;

    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), Self::Error> {
        let header = FrameHeader {
            frame_type: FrameType::Binary(false),
            payload_len: 1 + message.len() as u64,
            mask_key: None,
        };
        let total = header.serialized_len() + 1 + message.len();
        if total > self.buf.len() {
            return Err(WsError::TooLong);
        }
        if self.len + total > self.buf.len() {
            self.flush().await?;
        }
        let frame = &mut self.buf[self.len..self.len + total];
        let header_len = header
            .serialize(frame)
            .map_err(|_| WsError::<S::Error>::TooLong)?;
        frame[header_len] = kind;
        frame[header_len + 1..].copy_from_slice(message);
        self.len += total;
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        if self.len == 0 {
            return Ok(());
        }
        let len = core::mem::take(&mut self.len);
        let mut inner = self.conn.inner.lock().await;
        if !inner.open {
            return Err(WsError::Closed);
        }
        let mut written = 0;
        while written < len {
            match inner.socket.write(&self.buf[written..len]).await {
                Ok(0) => {
                    inner.open = false;
                    return Err(WsError::Closed);
                }
                Ok(n) => written += n,
                Err(e) => {
                    inner.open = false;
                    return Err(WsError::Socket(e));
                }
            }
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.len = 0;
    }
}

#[derive(Copy, Clone)]
enum Phase {
    /// Waiting for a client
    Accept,
    /// Receiving the HTTP upgrade request
    Request,
    /// Writing the 101 response
    Response { accept_key: [u8; ACCEPT_KEY_LEN] },
    /// Writing the 400 response, the client is dropped afterward
    Reject,
    /// Handshake done, frames are exchanged
    Open,
}

/// Length of `Sec-WebSocket-Accept`: base64 of a SHA-1
const ACCEPT_KEY_LEN: usize = 28;
const RESPONSE_HEAD: &[u8] = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ";
const RESPONSE_TAIL: &[u8] = b"\r\n\r\n";
const RESPONSE_LEN: usize = RESPONSE_HEAD.len() + ACCEPT_KEY_LEN + RESPONSE_TAIL.len();
const REJECT: &[u8] = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// [MessageRx] over a [WsConnection]: accepts clients in [wait_connected](MessageRx::wait_connected) and receives
/// binary messages into `buf`, without an extra copy.
pub struct WsRx<'a, S> {
    conn: &'a WsConnection<S>,
    buf: &'a mut [u8],
    /// First byte not consumed yet
    start: usize,
    /// End of received bytes
    end: usize,
    phase: Phase,
    /// How much of the handshake response is written, in case a write is cancelled
    written: usize,
    /// Unmasked payload (kind and message) of a received frame, and where the frame ends
    message: Option<(usize, usize)>,
}

impl<'a, S: Socket> WsRx<'a, S> {
    /// `buf` must fit the longest message plus [WS_OVERHEAD], and the client's HTTP upgrade request (a few hundred
    /// bytes). `buf.len() - WS_OVERHEAD` is reported to the host as the maximum message length.
    pub fn new(conn: &'a WsConnection<S>, buf: &'a mut [u8]) -> Self {
        debug_assert!(buf.len() > WS_OVERHEAD);
        WsRx {
            conn,
            buf,
            start: 0,
            end: 0,
            phase: Phase::Accept,
            written: 0,
            message: None,
        }
    }

    /// Read more bytes after `end`, moving what's not consumed to the front first if less than `need` bytes fit.
    async fn read_more(&mut self, need: usize) -> Result<(), WsError<S::Error>> {
        if need > self.buf.len() {
            return Err(WsError::TooLong);
        }
        if self.start + need > self.buf.len() || self.end == self.buf.len() {
            self.buf.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        let mut inner = self.conn.inner.lock().await;
        match inner.socket.read(&mut self.buf[self.end..]).await {
            Ok(0) => Err(WsError::Closed),
            Ok(n) => {
                self.end += n;
                Ok(())
            }
            Err(e) => Err(WsError::Socket(e)),
        }
    }

    /// Write the rest of `bytes`, starting from `self.written`, which is kept up to date in case the future is dropped.
    async fn write_response(&mut self, bytes: &[u8]) -> Result<(), ()> {
        let conn = self.conn;
        let mut inner = conn.inner.lock().await;
        while self.written < bytes.len() {
            match inner.socket.write(&bytes[self.written..]).await {
                Ok(n) if n > 0 => self.written += n,
                _ => return Err(()),
            }
        }
        Ok(())
    }
}

impl<S: Socket> MessageRx for WsRx<'_, S> {
    type Error = WsError<S::Error>;

    async fn wait_message(&mut self) -> Result<(), Self::Error> {
        loop {
            if self.message.is_some() {
                return Ok(());
            }
            if !matches!(self.phase, Phase::Open) {
                return Err(WsError::Closed);
            }
            let need = match FrameHeader::deserialize(&self.buf[self.start..self.end]) {
                Ok((header, header_len)) => {
                    let total = usize::try_from(header.payload_len)
                        .ok()
                        .and_then(|len| len.checked_add(header_len))
                        .ok_or(WsError::TooLong)?;
                    if self.end - self.start >= total {
                        let payload = self.start + header_len..self.start + total;
                        let frame_end = self.start + total;
                        match header.frame_type {
                            FrameType::Binary(false) if !payload.is_empty() => {
                                header.mask(&mut self.buf[payload.clone()], 0);
                                self.message = Some((payload.start, payload.end));
                                self.start = frame_end;
                                return Ok(());
                            }
                            FrameType::Ping | FrameType::Pong => {
                                self.start = frame_end;
                                continue;
                            }
                            FrameType::Close => return Err(WsError::Closed),
                            _ => {
                                warn!("ws: unsupported frame");
                                return Err(WsError::Protocol);
                            }
                        }
                    }
                    total
                }
                Err(edge_ws::Error::Incomplete(more)) => self.end - self.start + more,
                Err(_) => return Err(WsError::Protocol),
            };
            self.read_more(need).await?;
        }
    }

    fn message(&self) -> Option<(u8, &[u8])> {
        let (start, end) = self.message?;
        Some((self.buf[start], &self.buf[start + 1..end]))
    }

    fn consume(&mut self) {
        self.message = None;
    }

    async fn wait_connected(&mut self) {
        loop {
            match self.phase {
                Phase::Open => {
                    // the link went down, drop this client and wait for a new one
                    self.phase = Phase::Accept;
                }
                Phase::Accept => {
                    let mut inner = self.conn.inner.lock().await;
                    inner.open = false;
                    inner.socket.accept().await;
                    drop(inner);
                    self.start = 0;
                    self.end = 0;
                    self.message = None;
                    self.phase = Phase::Request;
                }
                Phase::Request => match parse_request(&self.buf[..self.end]) {
                    Ok(Some((len, accept_key))) => {
                        // anything after the request is already the first frame
                        self.start = len;
                        self.phase = Phase::Response { accept_key };
                        self.written = 0;
                    }
                    Ok(None) => {
                        let need = self.end + 1;
                        if need > self.buf.len() {
                            warn!("ws: upgrade request does not fit into the rx buffer");
                            self.phase = Phase::Reject;
                            self.written = 0;
                        } else if self.read_more(need).await.is_err() {
                            self.phase = Phase::Accept;
                        }
                    }
                    Err(()) => {
                        self.phase = Phase::Reject;
                        self.written = 0;
                    }
                },
                Phase::Response { accept_key } => {
                    let mut response = [0u8; RESPONSE_LEN];
                    let (head, rest) = response.split_at_mut(RESPONSE_HEAD.len());
                    head.copy_from_slice(RESPONSE_HEAD);
                    rest[..ACCEPT_KEY_LEN].copy_from_slice(&accept_key);
                    rest[ACCEPT_KEY_LEN..].copy_from_slice(RESPONSE_TAIL);
                    if self.write_response(&response).await.is_err() {
                        self.phase = Phase::Accept;
                        continue;
                    }
                    self.conn.inner.lock().await.open = true;
                    self.phase = Phase::Open;
                    debug!("ws: client connected");
                    return;
                }
                Phase::Reject => {
                    _ = self.write_response(REJECT).await;
                    self.phase = Phase::Accept;
                }
            }
        }
    }

    fn reset(&mut self) {
        // called right after wait_connected(): bytes after the handshake belong to the new client, keep them
        self.message = None;
    }

    fn max_message_len(&self) -> usize {
        self.buf.len() - WS_OVERHEAD
    }
}

/// Parse an HTTP upgrade request, returns its length and the `Sec-WebSocket-Accept` value,
/// `Ok(None)` if incomplete, `Err` if it's not a valid WebSocket upgrade request.
fn parse_request(buf: &[u8]) -> Result<Option<(usize, [u8; ACCEPT_KEY_LEN])>, ()> {
    let mut headers = [httparse::EMPTY_HEADER; 24];
    let mut request = httparse::Request::new(&mut headers);
    let len = match request.parse(buf) {
        Ok(httparse::Status::Complete(len)) => len,
        Ok(httparse::Status::Partial) => return Ok(None),
        Err(_) => {
            warn!("ws: malformed http request");
            return Err(());
        }
    };
    let headers = || {
        request
            .headers
            .iter()
            .filter_map(|h| Some((h.name, core::str::from_utf8(h.value).ok()?)))
    };
    let method = request.method.and_then(edge_http::Method::new).ok_or(())?;
    if !edge_http::ws::is_upgrade_request(method, headers()) {
        warn!("ws: not an upgrade request");
        return Err(());
    }
    let mut buf = [0u8; edge_http::ws::MAX_BASE64_KEY_RESPONSE_LEN];
    let response =
        edge_http::ws::upgrade_response_headers(headers(), None, &mut buf).map_err(|_e| {
            warn!("ws: bad upgrade request");
        })?;
    let accept = response
        .iter()
        .find(|(name, _)| *name == "Sec-WebSocket-Accept")
        .map(|(_, value)| value.as_bytes())
        .ok_or(())?;
    let accept_key = accept.try_into().map_err(|_| ())?;
    Ok(Some((len, accept_key)))
}

/// [WsTx] / [WsRx] server, see [Server] on how to use it.
pub type WsServer<'a, S, C, M = ()> = Server<'a, WsTx<'a, S>, WsRx<'a, S>, C, M>;

/// Buffers used by [WsServer], `MAX_MESSAGE_LEN` is the longest message the device accepts and the longest reply
/// it can serialize, reported to the host exactly as is. The client's HTTP upgrade request must fit into
/// `MAX_MESSAGE_LEN + WS_OVERHEAD` too, a few hundred bytes are enough.
///
/// Takes `4 * MAX_MESSAGE_LEN + 2 * WS_OVERHEAD` bytes, in addition to the socket's own buffers.
#[repr(C)]
pub struct WsBuffers<const MAX_MESSAGE_LEN: usize> {
    /// Frames are received and unmasked here
    rx: [u8; MAX_MESSAGE_LEN],
    rx_overhead: [u8; WS_OVERHEAD],
    /// Frames are accumulated here before being written to the socket
    tx: [u8; MAX_MESSAGE_LEN],
    tx_overhead: [u8; WS_OVERHEAD],
    /// Used to serialize replies and link messages
    scratch: [u8; MAX_MESSAGE_LEN],
    /// Used to serialize events sent from handlers and through `server.sink()`
    event_scratch: [u8; MAX_MESSAGE_LEN],
}

impl<const MAX_MESSAGE_LEN: usize> WsBuffers<MAX_MESSAGE_LEN> {
    pub const fn new() -> Self {
        WsBuffers {
            rx: [0u8; MAX_MESSAGE_LEN],
            rx_overhead: [0u8; WS_OVERHEAD],
            tx: [0u8; MAX_MESSAGE_LEN],
            tx_overhead: [0u8; WS_OVERHEAD],
            scratch: [0u8; MAX_MESSAGE_LEN],
            event_scratch: [0u8; MAX_MESSAGE_LEN],
        }
    }

    /// Contiguous `MAX_MESSAGE_LEN + WS_OVERHEAD` tx and rx buffers, and the scratch buffers.
    #[allow(clippy::type_complexity)]
    fn split(&mut self) -> (&mut [u8], &mut [u8], &mut [u8], &mut [u8]) {
        const {
            assert!(
                size_of::<Self>() == 4 * MAX_MESSAGE_LEN + 2 * WS_OVERHEAD,
                "WsBuffers must have no padding"
            )
        };
        let (tx, rx) = (self.tx.as_mut_ptr(), self.rx.as_mut_ptr());
        // SAFETY: repr(C) struct of u8 arrays: alignment 1 and no padding (checked above), so `rx`
        // is followed by `rx_overhead` and `tx` by `tx_overhead`, both are contiguous, initialized
        // bytes borrowed mutably through self; the ranges do not overlap with each other or the scratch buffers.
        unsafe {
            (
                core::slice::from_raw_parts_mut(tx, MAX_MESSAGE_LEN + WS_OVERHEAD),
                core::slice::from_raw_parts_mut(rx, MAX_MESSAGE_LEN + WS_OVERHEAD),
                &mut self.scratch,
                &mut self.event_scratch,
            )
        }
    }
}

impl<const MAX_MESSAGE_LEN: usize> Default for WsBuffers<MAX_MESSAGE_LEN> {
    fn default() -> Self {
        Self::new()
    }
}

/// WebSocket server on a [Socket], e.g.:
/// ```ignore
/// let socket = EmbassyNetSocket::new(TcpSocket::new(stack, rx_buf, tx_buf), 8080);
/// let conn = WS_CONNECTION.init(WsConnection::new(socket));
/// let mut server = ws_server(link_config, conn, EmbassyClock, WS_BUFFERS.init(WsBuffers::new()));
/// server.run(&mut state).await;
/// ```
pub fn ws_server<'a, const MAX_MESSAGE_LEN: usize, S: Socket, C: Clock>(
    link_config: LinkConfig<'a>,
    conn: &'a WsConnection<S>,
    clock: C,
    buffers: &'a mut WsBuffers<MAX_MESSAGE_LEN>,
) -> WsServer<'a, S, C> {
    let (tx_buf, rx_buf, scratch, event_scratch) = buffers.split();
    Server::new(
        link_config,
        WsTx::new(conn, tx_buf),
        WsRx::new(conn, rx_buf),
        clock,
        scratch,
        event_scratch,
    )
}

#[cfg(feature = "embassy-net")]
pub use embassy::EmbassyNetSocket;

#[cfg(feature = "embassy-net")]
mod embassy {
    use embassy_net::tcp::{Error, State, TcpSocket};

    use super::Socket;
    use crate::fmt::warn;

    /// [Socket] on an `embassy-net` TCP socket, listening on `port`.
    ///
    /// Set a timeout on the socket (`set_timeout`, with `set_keep_alive` shorter than it) before handing it in,
    /// so that writes to a host that is gone fail. Nagle's algorithm is disabled on every accepted connection,
    /// for the same reason the host disables it, see the WebSocket transport docs.
    pub struct EmbassyNetSocket<'a> {
        socket: TcpSocket<'a>,
        port: u16,
    }

    impl<'a> EmbassyNetSocket<'a> {
        pub fn new(socket: TcpSocket<'a>, port: u16) -> Self {
            EmbassyNetSocket { socket, port }
        }

        pub fn socket_mut(&mut self) -> &mut TcpSocket<'a> {
            &mut self.socket
        }
    }

    impl Socket for EmbassyNetSocket<'_> {
        type Error = Error;

        async fn accept(&mut self) {
            loop {
                if self.socket.state() != State::Closed {
                    // previous client, or listening since an accept that was cancelled
                    self.socket.abort();
                }
                match self.socket.accept(self.port).await {
                    Ok(()) => {
                        self.socket.set_nagle_enabled(false);
                        return;
                    }
                    Err(_e) => {
                        warn!("ws: accept failed: {:?}", _e);
                        embassy_time::Timer::after_millis(100).await;
                    }
                }
            }
        }

        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
            self.socket.read(buf).await
        }

        async fn write(&mut self, bytes: &[u8]) -> Result<usize, Error> {
            self.socket.write(bytes).await
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use core::cell::RefCell;
    use core::task::Poll;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::vec::Vec;

    use embassy_futures::{block_on, poll_once};

    use super::*;

    /// Chunks to be read and bytes written, shared with the test. Reads with nothing queued stay pending.
    #[derive(Default)]
    struct Script {
        input: VecDeque<Vec<u8>>,
        output: Vec<u8>,
        accepts: usize,
    }

    struct MockSocket(Rc<RefCell<Script>>);

    impl Socket for MockSocket {
        type Error = ();

        async fn accept(&mut self) {
            self.0.borrow_mut().accepts += 1;
        }

        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
            let mut chunk =
                core::future::poll_fn(|_| match self.0.borrow_mut().input.pop_front() {
                    Some(chunk) => Poll::Ready(chunk),
                    None => Poll::Pending,
                })
                .await;
            let n = chunk.len().min(buf.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            if n < chunk.len() {
                self.0.borrow_mut().input.push_front(chunk.split_off(n));
            }
            Ok(n)
        }

        async fn write(&mut self, bytes: &[u8]) -> Result<usize, ()> {
            // a few bytes at a time, as a full socket buffer would
            let n = bytes.len().min(7);
            self.0.borrow_mut().output.extend_from_slice(&bytes[..n]);
            Ok(n)
        }
    }

    /// Example from RFC 6455
    const REQUEST: &[u8] =
        b"GET /ww HTTP/1.1\r\nHost: device\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
        Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";
    const ACCEPT: &[u8] = b"Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n";

    /// Client to server frame, masked as clients must
    fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mask = [0x12, 0x34, 0x56, 0x78];
        let mut f = std::vec![0x80 | opcode, 0x80 | payload.len() as u8];
        f.extend_from_slice(&mask);
        f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        f
    }

    fn bytes(chunks: &[&[u8]]) -> VecDeque<Vec<u8>> {
        chunks.iter().map(|c| c.to_vec()).collect()
    }

    fn one_by_one(b: &[u8]) -> VecDeque<Vec<u8>> {
        b.iter().map(|b| std::vec![*b]).collect()
    }

    #[test]
    fn handshake_and_frames_byte_by_byte() {
        let script = Rc::new(RefCell::new(Script::default()));
        let mut input = one_by_one(REQUEST);
        input.extend(one_by_one(&frame(0x9, b"ping")));
        input.extend(one_by_one(&frame(0x2, &[3, 1, 2, 3])));
        script.borrow_mut().input = input;
        let conn = WsConnection::new(MockSocket(script.clone()));
        let (mut rx_buf, mut tx_buf) = ([0u8; 256], [0u8; 64]);
        let mut rx = WsRx::new(&conn, &mut rx_buf);
        let mut tx = WsTx::new(&conn, &mut tx_buf);

        block_on(rx.wait_connected());
        let out = core::mem::take(&mut script.borrow_mut().output);
        assert!(out.starts_with(b"HTTP/1.1 101 Switching Protocols\r\n"));
        assert!(
            out.ends_with(ACCEPT),
            "{}",
            std::string::String::from_utf8_lossy(&out)
        );

        block_on(rx.wait_message()).unwrap();
        assert_eq!(rx.message(), Some((3, &[1u8, 2, 3][..])));
        rx.consume();

        block_on(tx.write_message(5, &[7; 200])).unwrap_err(); // does not fit into tx_buf
        block_on(tx.write_message(5, &[1, 2])).unwrap();
        block_on(tx.write_message(6, &[])).unwrap();
        block_on(tx.flush()).unwrap();
        assert_eq!(script.borrow().output, [0x82, 3, 5, 1, 2, 0x82, 1, 6]);
    }

    #[test]
    fn wait_message_is_cancel_safe() {
        let script = Rc::new(RefCell::new(Script::default()));
        script.borrow_mut().input = bytes(&[REQUEST]);
        let conn = WsConnection::new(MockSocket(script.clone()));
        let mut rx_buf = [0u8; 256];
        let mut rx = WsRx::new(&conn, &mut rx_buf);
        block_on(rx.wait_connected());
        let mut received = Vec::new();

        // several frames in one read, the last one split, a dropped wait in between each piece
        let (a, b) = (frame(0x2, &[1, 10]), frame(0x2, &[2, 20, 21]));
        let mut all = a.clone();
        all.extend_from_slice(&b[..3]);
        for piece in [all, b[3..].to_vec()] {
            assert!(poll_once(rx.wait_message()).is_pending());
            script.borrow_mut().input.push_back(piece);
            while let Poll::Ready(r) = poll_once(rx.wait_message()) {
                r.unwrap();
                let (kind, m) = rx.message().unwrap();
                received.push((kind, m.to_vec()));
                rx.consume();
            }
        }
        assert_eq!(received, [(1, std::vec![10]), (2, std::vec![20, 21])]);
        assert!(script.borrow().input.is_empty());
    }

    #[test]
    fn unsupported_frames_and_close_drop_the_connection() {
        for (opcode, error) in [(0x1, WsError::Protocol), (0x8, WsError::Closed)] {
            let script = Rc::new(RefCell::new(Script::default()));
            script.borrow_mut().input = bytes(&[REQUEST, &frame(opcode, b"x")]);
            let conn = WsConnection::new(MockSocket(script.clone()));
            let mut rx_buf = [0u8; 256];
            let mut rx = WsRx::new(&conn, &mut rx_buf);
            block_on(rx.wait_connected());
            assert_eq!(block_on(rx.wait_message()), Err(error));
        }
    }

    #[test]
    fn plain_http_request_is_rejected() {
        let script = Rc::new(RefCell::new(Script::default()));
        script.borrow_mut().input = bytes(&[b"GET / HTTP/1.1\r\nHost: device\r\n\r\n", REQUEST]);
        let conn = WsConnection::new(MockSocket(script.clone()));
        let mut rx_buf = [0u8; 256];
        let mut rx = WsRx::new(&conn, &mut rx_buf);
        block_on(rx.wait_connected());
        let s = script.borrow();
        assert_eq!(s.accepts, 2);
        assert!(s.output.starts_with(REJECT));
        assert!(s.output.ends_with(ACCEPT));
    }

    #[test]
    fn tx_fails_when_not_connected() {
        let script = Rc::new(RefCell::new(Script::default()));
        let conn = WsConnection::new(MockSocket(script));
        let mut tx_buf = [0u8; 64];
        let mut tx = WsTx::new(&conn, &mut tx_buf);
        block_on(tx.write_message(1, &[1])).unwrap();
        assert_eq!(block_on(tx.flush()), Err(WsError::Closed));
    }
}
