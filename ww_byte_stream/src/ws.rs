//! WebSocket framing on a byte stream (feature `ws`): what the `ws` transport of `wire_weaver_client` speaks, the
//! same as `ww_device::ws`. The client's HTTP upgrade request names what it asks for in its path; the server answers
//! `101 Switching Protocols` or refuses with an HTTP status. Each `ww_link` message is then one binary WebSocket
//! message, `| kind: u8 | payload |`.
//!
//! Frame headers and the handshake come from `edge-ws` and `edge-http` (their sans-IO parts). Supported: unfragmented
//! binary messages; Ping and Pong are skipped (`ww_link` pings keep the link alive); Close, Text and fragmented
//! messages end the connection.

use std::io;

use edge_ws::{FrameHeader, FrameType};
use futures_util::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The longest upgrade request a server reads.
pub const REQUEST_LEN_MAX: usize = 8 * 1024;

/// The longest frame header and the kind byte: what a message takes on the wire beyond its payload.
pub const OVERHEAD: usize = FrameHeader::MAX_LEN + 1;

/// Length of `Sec-WebSocket-Accept`: base64 of a SHA-1.
const ACCEPT_KEY_LEN: usize = 28;

/// A client's WebSocket upgrade request, read and not answered yet.
#[derive(Clone, Debug)]
pub struct Upgrade {
    path: String,
    accept_key: String,
    /// What the client sent after the request: the start of its first frame.
    rest: Vec<u8>,
}

/// Why an upgrade request was not read.
#[derive(Debug)]
pub enum UpgradeError {
    /// The stream failed or ended.
    Io(io::Error),
    /// Not an HTTP request, or longer than [REQUEST_LEN_MAX]: worth no answer.
    NotHttp,
    /// An HTTP request but not a WebSocket upgrade: answered with `400 Bad Request` by [refuse].
    NotUpgrade,
}

impl From<io::Error> for UpgradeError {
    fn from(e: io::Error) -> Self {
        UpgradeError::Io(e)
    }
}

impl Upgrade {
    /// Read the upgrade request from `stream`.
    ///
    /// # Errors
    ///
    /// See [UpgradeError].
    pub async fn read<S: AsyncRead + Unpin>(stream: &mut S) -> Result<Upgrade, UpgradeError> {
        let mut buf = vec![0u8; 1024];
        let mut end = 0;
        loop {
            if let Some((len, upgrade)) = parse_request(&buf[..end])? {
                return Ok(Upgrade {
                    rest: buf[len..end].to_vec(),
                    ..upgrade
                });
            }
            if end == buf.len() {
                if buf.len() >= REQUEST_LEN_MAX {
                    return Err(UpgradeError::NotHttp);
                }
                buf.resize((buf.len() * 2).min(REQUEST_LEN_MAX), 0);
            }
            match stream.read(&mut buf[end..]).await? {
                0 => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
                n => end += n,
            }
        }
    }

    /// The path the client asked for, without its query: `/` or `/counter`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Switch `stream` to WebSocket.
    ///
    /// # Errors
    ///
    /// Writing the answer failed.
    pub async fn accept<S: AsyncWrite + Unpin>(&self, stream: &mut S) -> io::Result<()> {
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\r\n",
            self.accept_key
        );
        stream.write_all(response.as_bytes()).await?;
        stream.flush().await
    }

    /// What the client sent after its request.
    pub fn rest(&self) -> &[u8] {
        &self.rest
    }
}

/// Refuse the client with the HTTP status `code` and its `reason`: `404 Not Found`, `503 Service Unavailable`.
///
/// # Errors
///
/// Writing the answer failed.
pub async fn refuse<S: AsyncWrite + Unpin>(
    stream: &mut S,
    code: u16,
    reason: &str,
) -> io::Result<()> {
    let response =
        format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await
}

/// The request at the start of `buf` and its length; `None` while it is incomplete.
fn parse_request(buf: &[u8]) -> Result<Option<(usize, Upgrade)>, UpgradeError> {
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut request = httparse::Request::new(&mut headers);
    let len = match request.parse(buf) {
        Ok(httparse::Status::Complete(len)) => len,
        Ok(httparse::Status::Partial) => return Ok(None),
        Err(_) => return Err(UpgradeError::NotHttp),
    };
    let headers = || {
        request
            .headers
            .iter()
            .filter_map(|h| Some((h.name, std::str::from_utf8(h.value).ok()?)))
    };
    let method = request
        .method
        .and_then(edge_http::Method::new)
        .ok_or(UpgradeError::NotUpgrade)?;
    if !edge_http::ws::is_upgrade_request(method, headers()) {
        return Err(UpgradeError::NotUpgrade);
    }
    let mut key = [0u8; edge_http::ws::MAX_BASE64_KEY_RESPONSE_LEN];
    let response = edge_http::ws::upgrade_response_headers(headers(), None, &mut key)
        .map_err(|_| UpgradeError::NotUpgrade)?;
    let accept_key = response
        .iter()
        .find(|(name, _)| *name == "Sec-WebSocket-Accept")
        .map(|(_, value)| *value)
        .filter(|value| value.len() == ACCEPT_KEY_LEN)
        .ok_or(UpgradeError::NotUpgrade)?;
    let path = request.path.unwrap_or("/");
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    Ok(Some((
        len,
        Upgrade {
            path: path.to_owned(),
            accept_key: accept_key.to_owned(),
            rest: Vec::new(),
        },
    )))
}

/// Writes messages as binary WebSocket messages, held back until [flush](Self::flush).
pub struct WsWriter<W> {
    w: W,
    buf: Vec<u8>,
    /// How much of `buf` is written, kept in case a flush is dropped.
    written: usize,
}

impl<W: AsyncWrite + Unpin> WsWriter<W> {
    pub fn new(w: W) -> Self {
        WsWriter {
            w,
            buf: Vec::new(),
            written: 0,
        }
    }

    /// Hold back `kind` and `message` as one binary message.
    pub fn push(&mut self, kind: u8, message: &[u8]) {
        let header = FrameHeader {
            frame_type: FrameType::Binary(false),
            payload_len: 1 + message.len() as u64,
            mask_key: None,
        };
        let start = self.buf.len();
        self.buf.resize(start + header.serialized_len(), 0);
        // the buffer was sized for the header
        let _ = header.serialize(&mut self.buf[start..]);
        self.buf.push(kind);
        self.buf.extend_from_slice(message);
    }

    /// Write what is held back. Cancel-safe: a flush dropped halfway goes on from there.
    ///
    /// # Errors
    ///
    /// The stream failed.
    pub async fn flush(&mut self) -> io::Result<()> {
        while self.written < self.buf.len() {
            match self.w.write(&self.buf[self.written..]).await? {
                0 => return Err(io::ErrorKind::WriteZero.into()),
                n => self.written += n,
            }
        }
        self.buf.clear();
        self.written = 0;
        self.w.flush().await
    }

    /// Drop what is held back and not started; a message partly written stays, so the stream stays in step.
    pub fn clear(&mut self) {
        if self.written == 0 {
            self.buf.clear();
        }
    }

    /// Close the stream after what is held back.
    ///
    /// # Errors
    ///
    /// The stream failed.
    pub async fn close(&mut self) -> io::Result<()> {
        self.flush().await?;
        self.w.close().await
    }
}

/// Reads binary WebSocket messages of up to `message_len_max` bytes after the kind byte.
pub struct WsReader<R> {
    r: R,
    buf: Vec<u8>,
    /// The first byte not consumed.
    start: usize,
    /// The end of what was read.
    end: usize,
    /// The unmasked payload of the message received, kind byte first.
    message: Option<(usize, usize)>,
}

impl<R: AsyncRead + Unpin> WsReader<R> {
    /// `rest` is what came after the upgrade request ([Upgrade::rest]).
    pub fn new(r: R, rest: &[u8], message_len_max: usize) -> Self {
        let mut buf = vec![0u8; (message_len_max + OVERHEAD).max(rest.len())];
        buf[..rest.len()].copy_from_slice(rest);
        WsReader {
            r,
            buf,
            start: 0,
            end: rest.len(),
            message: None,
        }
    }

    /// Wait until a message is received. Cancel-safe.
    ///
    /// # Errors
    ///
    /// The stream failed or ended, the client closed the connection, sent a frame of another kind or one too long.
    pub async fn wait_message(&mut self) -> io::Result<()> {
        loop {
            if self.message.is_some() {
                return Ok(());
            }
            let need = match FrameHeader::deserialize(&self.buf[self.start..self.end]) {
                Ok((header, header_len)) => {
                    let total = usize::try_from(header.payload_len)
                        .ok()
                        .and_then(|len| len.checked_add(header_len))
                        .ok_or_else(|| invalid("a frame too long"))?;
                    if self.end - self.start >= total {
                        let payload = self.start + header_len..self.start + total;
                        self.start += total;
                        match header.frame_type {
                            FrameType::Binary(false) if !payload.is_empty() => {
                                header.mask(&mut self.buf[payload.clone()], 0);
                                self.message = Some((payload.start, payload.end));
                                return Ok(());
                            }
                            FrameType::Ping | FrameType::Pong => continue,
                            FrameType::Close => {
                                return Err(io::ErrorKind::ConnectionAborted.into());
                            }
                            _ => return Err(invalid("a frame other than one binary message")),
                        }
                    }
                    total
                }
                Err(edge_ws::Error::Incomplete(more)) => self.end - self.start + more,
                Err(_) => return Err(invalid("a malformed frame")),
            };
            self.read_more(need).await?;
        }
    }

    /// Read more after `end`, first moving what is not consumed to the front when `need` bytes don't fit.
    async fn read_more(&mut self, need: usize) -> io::Result<()> {
        if need > self.buf.len() {
            return Err(invalid("a frame longer than the longest message"));
        }
        if self.start + need > self.buf.len() || self.end == self.buf.len() {
            self.buf.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        match self.r.read(&mut self.buf[self.end..]).await? {
            0 => Err(io::ErrorKind::UnexpectedEof.into()),
            n => {
                self.end += n;
                Ok(())
            }
        }
    }

    /// The message received: its kind and payload.
    pub fn message(&self) -> Option<(u8, &[u8])> {
        let (start, end) = self.message?;
        Some((self.buf[start], &self.buf[start + 1..end]))
    }

    /// Done with the message received.
    pub fn consume(&mut self) {
        self.message = None;
    }
}

/// One WebSocket connection as a `ww_device` medium (feature `device`), after [Upgrade::accept]: what a server runs
/// a `DeviceLink` on. `rest` is [Upgrade::rest].
#[cfg(feature = "device")]
pub fn medium<S: AsyncRead + AsyncWrite>(
    stream: S,
    rest: &[u8],
    message_len_max: usize,
) -> (
    WsTx<futures_util::io::WriteHalf<S>>,
    WsRx<futures_util::io::ReadHalf<S>>,
) {
    let (r, w) = stream.split();
    (
        WsTx {
            writer: Some(WsWriter::new(w)),
        },
        WsRx {
            reader: Some(WsReader::new(r, rest, message_len_max)),
            message_len_max,
        },
    )
}

/// Sends the messages of one WebSocket connection; down for good after a write failed.
#[cfg(feature = "device")]
pub struct WsTx<W> {
    writer: Option<WsWriter<W>>,
}

/// Receives the messages of one WebSocket connection; down for good after the stream ended or the client closed it.
#[cfg(feature = "device")]
pub struct WsRx<R> {
    reader: Option<WsReader<R>>,
    message_len_max: usize,
}

/// The connection is down: its stream failed or ended before.
#[cfg(feature = "device")]
fn down() -> io::Error {
    io::ErrorKind::NotConnected.into()
}

#[cfg(feature = "device")]
impl<W: AsyncWrite + Unpin> ww_device::MessageTx for WsTx<W> {
    type Error = io::Error;

    async fn write_message(&mut self, kind: u8, message: &[u8]) -> io::Result<()> {
        self.writer.as_mut().ok_or_else(down)?.push(kind, message);
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        let w = self.writer.as_mut().ok_or_else(down)?;
        let r = w.flush().await;
        if r.is_err() {
            self.writer = None;
        }
        r
    }

    /// Drops what is held back; a message partly written stays, so the stream stays in step.
    fn reset(&mut self) {
        if let Some(w) = self.writer.as_mut() {
            w.clear();
        }
    }

    /// The frozen WebSocket hello (`ww_link::hello::ws`): one binary message, alone.
    async fn write_hello(&mut self, hello: &ww_device::ww_link::Hello) -> io::Result<()> {
        let frame = ww_device::ww_link::hello::ws::frame(hello);
        self.write_message(frame[0], &frame[1..]).await?;
        self.flush().await
    }
}

#[cfg(feature = "device")]
impl<R: AsyncRead + Unpin> ww_device::MessageRx for WsRx<R> {
    type Error = io::Error;

    async fn wait_message(&mut self) -> io::Result<()> {
        let r = self.reader.as_mut().ok_or_else(down)?;
        let result = r.wait_message().await;
        if result.is_err() {
            self.reader = None;
        }
        result
    }

    fn message(&self) -> Option<(u8, &[u8])> {
        self.reader.as_ref()?.message()
    }

    fn consume(&mut self) {
        if let Some(r) = self.reader.as_mut() {
            r.consume();
        }
    }

    /// Ready at once while the stream is up; a connection that ended never comes back.
    async fn wait_connected(&mut self) {
        if self.reader.is_none() {
            std::future::pending::<()>().await;
        }
    }

    /// Nothing partial to drop: a message partly read stays for the next wait.
    fn reset(&mut self) {
        self.consume();
    }

    fn max_message_len(&self) -> usize {
        self.message_len_max
    }
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("WebSocket: {what}"))
}

#[cfg(test)]
mod tests {
    use futures_util::io::Cursor;

    use super::*;

    const REQUEST: &[u8] = b"GET /counter?x=1 HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\n\
        Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";

    #[tokio::test]
    async fn an_upgrade_is_read_with_its_path_and_answered() {
        let mut sent = REQUEST.to_vec();
        sent.extend_from_slice(&[0x82, 0x80]);
        let upgrade = Upgrade::read(&mut Cursor::new(sent)).await.unwrap();
        assert_eq!(upgrade.path(), "/counter");
        assert_eq!(upgrade.rest(), &[0x82, 0x80]);
        let mut answer = Cursor::new(Vec::new());
        upgrade.accept(&mut answer).await.unwrap();
        // the accept key of RFC 6455's example
        let answer = String::from_utf8(answer.into_inner()).unwrap();
        assert!(answer.starts_with("HTTP/1.1 101 "));
        assert!(answer.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
    }

    #[tokio::test]
    async fn a_request_that_is_no_upgrade_is_told_apart_from_garbage() {
        let get = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec();
        let not_upgrade = Upgrade::read(&mut Cursor::new(get)).await;
        assert!(matches!(not_upgrade, Err(UpgradeError::NotUpgrade)));
        let garbage = Upgrade::read(&mut Cursor::new(b"WWHB\x02\x01".to_vec())).await;
        assert!(matches!(garbage, Err(UpgradeError::NotHttp)));
    }

    /// A masked binary frame of `payload`, as a client sends it.
    fn client_frame(frame_type: FrameType, payload: &[u8]) -> Vec<u8> {
        let header = FrameHeader {
            frame_type,
            payload_len: payload.len() as u64,
            mask_key: Some(0x1234_5678),
        };
        let mut frame = vec![0u8; header.serialized_len()];
        header.serialize(&mut frame).unwrap();
        let start = frame.len();
        frame.extend_from_slice(payload);
        header.mask(&mut frame[start..], 0);
        frame
    }

    #[tokio::test]
    async fn messages_are_read_unmasked_skipping_pings_and_end_at_close() {
        let mut sent = client_frame(FrameType::Binary(false), &[7, 1, 2]);
        sent.extend(client_frame(FrameType::Ping, b"hi"));
        sent.extend(client_frame(FrameType::Binary(false), &[8]));
        sent.extend(client_frame(FrameType::Close, &[]));
        // the first byte came with the upgrade request
        let mut r = WsReader::new(Cursor::new(sent[1..].to_vec()), &sent[..1], 16);
        r.wait_message().await.unwrap();
        assert_eq!(r.message(), Some((7, &[1u8, 2][..])));
        r.consume();
        r.wait_message().await.unwrap();
        assert_eq!(r.message(), Some((8, &[][..])));
        r.consume();
        assert!(r.wait_message().await.is_err());
    }

    #[tokio::test]
    async fn a_frame_over_the_longest_message_ends_the_connection() {
        let sent = client_frame(FrameType::Binary(false), &[0; 64]);
        let mut r = WsReader::new(Cursor::new(sent), &[], 16);
        let e = r.wait_message().await.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn written_messages_are_unmasked_binary_frames() {
        let mut w = WsWriter::new(Cursor::new(Vec::new()));
        w.push(3, &[9, 9]);
        w.push(4, &[]);
        w.flush().await.unwrap();
        assert_eq!(w.w.into_inner(), [0x82, 3, 3, 9, 9, 0x82, 1, 4]);
    }
}
