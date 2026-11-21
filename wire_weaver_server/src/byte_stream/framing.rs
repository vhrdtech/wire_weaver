//! How a connection on a byte stream opens and carries messages: the axis a [StreamListener](super::StreamListener)
//! is generic over.

use std::future::Future;
use std::io;

use futures_util::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use ww_byte_stream::medium::{StreamRx, StreamTx, medium};
use ww_byte_stream::{Answer, Preamble, PreambleError, Status, Target};
use ww_device::{MessageRx, MessageTx};

/// Opens connections on streams of type `S` and frames their messages.
///
/// A connection opens in two steps. [read](Self::read) takes what the client sends first and says what it asks
/// for; the listener then decides, and [answer](Self::answer) tells the client and, when it is served, gives the
/// connection's message halves.
pub trait Framing<S> {
    /// Sends messages to the client.
    type Tx: MessageTx;
    /// Receives messages from the client.
    type Rx: MessageRx;
    /// The stream between the two steps.
    type Opening;

    /// What the client asks for. `None` for a client that is not served at all: one that answered itself (a
    /// preamble version or target this side doesn't know), sent garbage or left.
    fn read(&self, stream: S) -> impl Future<Output = Option<(Self::Opening, Target)>>;

    /// Tell the client `status`. The connection's halves when it is [Status::Ok] and the answer went out.
    fn answer(
        &self,
        opening: Self::Opening,
        status: Status,
    ) -> impl Future<Output = Option<(Self::Tx, Self::Rx)>>;
}

/// The local socket's framing on any stream: the `ww_byte_stream` preamble and answer, then `| len: u32 LE | kind:
/// u8 | payload |` messages. What `ClientConfig::ipc_path`, `tcp` and `command` clients speak.
#[derive(Clone, Debug)]
pub struct PreambleFraming {
    version: String,
    message_len_max: usize,
}

impl PreambleFraming {
    /// `version` goes into every answer (a refused client shows it), `message_len_max` is the longest message the
    /// server takes.
    pub fn new(version: &str, message_len_max: usize) -> Self {
        PreambleFraming {
            version: version.to_owned(),
            message_len_max,
        }
    }

    async fn send<S: AsyncWrite + Unpin>(&self, w: &mut S, status: Status) -> io::Result<()> {
        w.write_all(&Answer::new(status, &self.version).encode())
            .await?;
        w.flush().await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Framing<S> for PreambleFraming {
    type Tx = StreamTx<WriteHalf<S>>;
    type Rx = StreamRx<ReadHalf<S>>;
    type Opening = S;

    async fn read(&self, mut stream: S) -> Option<(S, Target)> {
        let refused = match Preamble::read(&mut stream).await {
            Ok(preamble) => return Some((stream, preamble.target)),
            Err(PreambleError::UnsupportedVersion(_)) => Status::PreambleVersion,
            Err(PreambleError::UnknownTarget(_)) => Status::UnknownTarget,
            // not a WireWeaver client, or one that left: nothing to tell it
            Err(_) => return None,
        };
        // the client is refused either way, a failed answer changes nothing
        let _ = self.send(&mut stream, refused).await;
        None
    }

    async fn answer(&self, mut stream: S, status: Status) -> Option<(Self::Tx, Self::Rx)> {
        self.send(&mut stream, status).await.ok()?;
        (status == Status::Ok).then(|| medium(stream, self.message_len_max))
    }
}
