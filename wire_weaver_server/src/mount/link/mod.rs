//! The host side of a link to a mounted backend (SRV-78): what a client's event loop does for one connection, without
//! a runtime of its own.
//!
//! A [BackendLink] opens a byte stream through a [Connect], asks for its target with the `ww_byte_stream` preamble,
//! sets the link up as a dynamic host (`ww_link` hello, `DeviceInfo`, `LinkSetup`, `LinkReady`), then carries
//! `ww_client_server` messages both ways, pings while idle and notices a backend that went quiet. The protocol is a
//! state machine fed by [BackendLink::ready] (IO) and [BackendLink::next_event] (messages and time); every await of
//! `ready` is cancel-safe, since a server's `Backend::wait` that drives it is dropped whenever a request comes.

use std::collections::VecDeque;
use std::fmt;
use std::future::pending;
use std::io;
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use futures_util::future::{Either, select};
use futures_util::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use futures_util::pin_mut;
use wire_weaver::DisconnectReason;
use wire_weaver::ww_version::{FullVersion, Version, WireVersion};
use ww_byte_stream::{Answer, AnswerError, MessageReader, MessageWriter, Preamble, Status, Target};
use ww_device::Instant;
use ww_link::hello::{Generations, Hello, Mismatch, Role};
use ww_link::{DeviceInfoOwned, Kind, LinkSetup, Message, PEER_TIMEOUT_MS, PING_INTERVAL_MS};

use super::connect::Connect;

/// Link setup, from opening the stream to `LinkReady`, must be done within this time.
const SETUP_TIMEOUT: Duration = Duration::from_secs(5);
/// A ping goes out when nothing else was sent for this long.
const PING_INTERVAL: Duration = Duration::from_millis(PING_INTERVAL_MS);
/// The backend is gone when nothing came from it for this long.
const PEER_TIMEOUT: Duration = Duration::from_millis(PEER_TIMEOUT_MS);
/// Longest preamble answer: its head and a hub version of at most 255 bytes.
const ANSWER_LEN_MAX: usize = 7 + 255;
/// Control messages (`LinkSetup`, `Disconnect`) are encoded here.
const CONTROL_SCRATCH_LEN: usize = 256;

/// This build's generations, sent in the hello and the preamble (EVO-23).
const GENERATIONS: Generations = Generations::this_build(ww_client_server::GENERATION);

/// What the host side says about itself in link setup.
#[derive(Clone, Debug)]
pub(crate) struct LinkParams {
    /// What the preamble asks for.
    pub target: Target,
    /// The longest message this side takes: the front connection's limit, so whatever the backend answers fits it.
    pub message_len_max: usize,
    /// The `ww_client_server` wire version the backend answers for: the front client's, so the error kinds it
    /// sends are ones that client reads. `None` reads none of those added since `w2.0`.
    pub model_wire: Option<WireVersion>,
}

/// Why a link went down, or never came up.
#[derive(Debug)]
pub enum LinkDown {
    /// The stream could not be opened.
    Connect(io::Error),
    /// The stream failed or closed.
    Io(io::Error),
    /// The other side answered the preamble with something that isn't a WireWeaver server.
    Answer(AnswerError),
    /// The other side refused the preamble's target.
    Refused(Status),
    /// The other side speaks another generation of a layer (EVO-23).
    Generations(Mismatch),
    /// Link setup took longer than it may.
    SetupTimeout,
    /// Nothing came from the backend for too long.
    PeerTimeout,
    /// The backend ended the link.
    Disconnect(DisconnectReason),
    /// This side closed it.
    Closed,
}

impl fmt::Display for LinkDown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkDown::Connect(e) => write!(f, "can't connect: {e}"),
            LinkDown::Io(e) => write!(f, "stream failed: {e}"),
            LinkDown::Answer(e) => write!(f, "bad preamble answer: {e}"),
            LinkDown::Refused(status) => write!(f, "refused: {status}"),
            LinkDown::Generations(m) => write!(
                f,
                "the backend speaks {} generation {}, this side {}",
                m.layer.name(),
                m.theirs,
                m.ours
            ),
            LinkDown::SetupTimeout => write!(f, "link setup timed out"),
            LinkDown::PeerTimeout => write!(f, "the backend went quiet"),
            LinkDown::Disconnect(reason) => write!(f, "the backend disconnected: {reason:?}"),
            LinkDown::Closed => write!(f, "closed"),
        }
    }
}

impl std::error::Error for LinkDown {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LinkDown::Connect(e) | LinkDown::Io(e) => Some(e),
            LinkDown::Answer(e) => Some(e),
            _ => None,
        }
    }
}

/// What a link has to say, from [BackendLink::next_event].
#[derive(Debug)]
pub(crate) enum LinkEvent<'a> {
    /// Link setup finished: the backend's facts are in [BackendLink::info].
    Up,
    /// A `ww_client_server` message of the backend: an event.
    Data(&'a [u8]),
    /// The link is over; nothing more comes.
    Down(LinkDown),
}

/// Where link setup is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Phase {
    /// Opening the stream and the preamble.
    Opening,
    /// The hello went out: the backend's hello and `DeviceInfo` are awaited.
    Hello,
    /// `LinkSetup` went out: `LinkReady` is awaited.
    Setup,
    Up,
    /// Down, told or not yet.
    Down,
}

/// The stream, by what is done with it.
enum Io<C: Connect> {
    /// In a mutex only so that a link is `Sync` whenever its parts are `Send`: a server's connection holds its
    /// backend across awaits by `&`. Never locked, reached through `get_mut`.
    Connecting(Mutex<Pin<Box<C::Opening>>>),
    /// The preamble goes out from `out[sent..]`, its answer comes into `got`.
    Preamble {
        stream: C::Stream,
        out: Vec<u8>,
        sent: usize,
        got: Vec<u8>,
    },
    Framed {
        reader: MessageReader<ReadHalf<C::Stream>>,
        writer: MessageWriter<WriteHalf<C::Stream>>,
        /// Messages were pushed since the last flush.
        dirty: bool,
    },
    Closed,
}

/// One host-side link to a backend, see the [module](self) docs.
pub(crate) struct BackendLink<C: Connect> {
    io: Io<C>,
    phase: Phase,
    params: LinkParams,
    token: u64,
    info: Option<DeviceInfoOwned>,
    /// Why the link went down, until told.
    down: Option<LinkDown>,
    /// `next_event` returned the reader's current message: consumed on the next call.
    held: bool,
    setup_deadline: Instant,
    last_rx: Instant,
    last_tx: Instant,
    /// Data pushed before the link came up, sent on `LinkReady`.
    early: VecDeque<Vec<u8>>,
}

impl<C: Connect> BackendLink<C> {
    /// Starts opening a link through `connector` at `now`; [ready](Self::ready) drives it.
    pub fn open(connector: &C, params: LinkParams, now: Instant) -> Self {
        BackendLink {
            io: Io::Connecting(Mutex::new(Box::pin(connector.connect()))),
            phase: Phase::Opening,
            params,
            token: new_setup_token(),
            info: None,
            down: None,
            held: false,
            setup_deadline: now + SETUP_TIMEOUT,
            last_rx: now,
            last_tx: now,
            early: VecDeque::new(),
        }
    }

    /// The link is down: nothing more comes.
    pub fn is_down(&self) -> bool {
        self.phase == Phase::Down
    }

    /// The backend's facts, once the link is up.
    pub fn info(&self) -> Option<&DeviceInfoOwned> {
        self.info.as_ref()
    }

    /// Completes when the link made progress: the stream opened, the preamble was answered, a message came, or the
    /// link failed. Writes what was pushed on the way. Cancel-safe; never completes while there is nothing to do.
    pub async fn ready(&mut self) {
        if self.held {
            self.consume();
        }
        let result = match &mut self.io {
            Io::Connecting(opening) => match opening
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
                .await
            {
                Ok(stream) => {
                    self.io = Io::Preamble {
                        stream,
                        out: preamble(&self.params.target),
                        sent: 0,
                        got: Vec::new(),
                    };
                    Ok(())
                }
                Err(e) => Err(LinkDown::Connect(e)),
            },
            Io::Preamble { .. } => self.preamble_step().await,
            Io::Framed {
                reader,
                writer,
                dirty,
            } => framed_step(reader, writer, dirty).await,
            Io::Closed => pending().await,
        };
        if let Err(down) = result {
            self.fail(down);
        }
    }

    /// One step of the preamble: write some of it, or read some of its answer. Taking the answer sends the hello.
    async fn preamble_step(&mut self) -> Result<(), LinkDown> {
        let Io::Preamble {
            stream,
            out,
            sent,
            got,
        } = &mut self.io
        else {
            unreachable!("called in the preamble only");
        };
        if *sent < out.len() {
            let n = stream.write(&out[*sent..]).await.map_err(LinkDown::Io)?;
            if n == 0 {
                return Err(LinkDown::Io(io::ErrorKind::WriteZero.into()));
            }
            *sent += n;
            if *sent == out.len() {
                stream.flush().await.map_err(LinkDown::Io)?;
            }
            return Ok(());
        }
        let mut buf = [0u8; ANSWER_LEN_MAX];
        let want = ANSWER_LEN_MAX - got.len();
        let n = stream
            .read(&mut buf[..want])
            .await
            .map_err(LinkDown::Io)?;
        if n == 0 {
            return Err(LinkDown::Io(io::ErrorKind::UnexpectedEof.into()));
        }
        got.extend_from_slice(&buf[..n]);
        let Some((answer, len)) = Answer::parse(got).map_err(LinkDown::Answer)? else {
            return Ok(());
        };
        if answer.status != Status::Ok {
            return Err(LinkDown::Refused(answer.status));
        }
        if len != got.len() {
            // the server says nothing before the host's hello
            return Err(LinkDown::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "bytes after the preamble answer",
            )));
        }
        let Io::Preamble { stream, .. } = std::mem::replace(&mut self.io, Io::Closed) else {
            unreachable!("matched above");
        };
        let (r, w) = stream.split();
        let mut writer = MessageWriter::new(w);
        let hello = Message::Hello(Hello {
            role: Role::Host,
            generations: GENERATIONS,
            token: self.token,
        });
        push(&mut writer, &hello);
        self.io = Io::Framed {
            reader: MessageReader::new(r, self.params.message_len_max),
            writer,
            dirty: true,
        };
        self.phase = Phase::Hello;
        Ok(())
    }

    /// The next thing the link has to say at `now`: link setup done, a message of the backend, the link down.
    /// Handles link messages and timers on the way. A [LinkEvent::Data] borrows the message until the next call.
    pub fn next_event(&mut self, now: Instant) -> Option<LinkEvent<'_>> {
        if self.held {
            self.consume();
        }
        self.handle_time(now);
        if let Some(down) = self.down.take() {
            return Some(LinkEvent::Down(down));
        }
        loop {
            let Io::Framed { reader, .. } = &self.io else {
                return None;
            };
            let (kind, payload) = reader.message()?;
            self.last_rx = now;
            match Message::decode(kind, payload) {
                Ok(Message::Data { channel: 0, .. }) if self.phase == Phase::Up => {
                    self.held = true;
                    let Io::Framed { reader, .. } = &self.io else {
                        unreachable!("matched above");
                    };
                    let (_, bytes) = reader.message().expect("the message is still there");
                    return Some(LinkEvent::Data(bytes));
                }
                Ok(Message::LinkReady { token })
                    if self.phase == Phase::Setup && (token == self.token || token == 0) =>
                {
                    self.consume();
                    self.phase = Phase::Up;
                    self.send_early(now);
                    return Some(LinkEvent::Up);
                }
                Ok(message) => {
                    let then = control(message, self.phase, self.token, &mut self.info);
                    self.consume();
                    match then {
                        Then::Nothing => {}
                        Then::Setup => {
                            let setup = self.link_setup();
                            self.push(&Message::LinkSetup(setup), now);
                            self.phase = Phase::Setup;
                        }
                        Then::Down(down) => {
                            self.fail(down);
                            return self.down.take().map(LinkEvent::Down);
                        }
                    }
                }
                // a kind of a newer backend, or a malformed message: dropped, as a device drops them
                Err(_) => self.consume(),
            }
        }
    }

    /// What this side says in link setup: a dynamic host, with the front's limit and model.
    fn link_setup(&self) -> LinkSetup<'static> {
        LinkSetup {
            host_user_version: FullVersion::new("", Version::new(0, 0, 0)),
            host_max_message_len: u32::try_from(self.params.message_len_max).unwrap_or(u32::MAX),
            token: self.token,
            client_meta: None,
            host_user_wire: None,
            host_model_wire: self.params.model_wire,
        }
    }

    /// Sends `bytes`, a `ww_client_server` request; one pushed before the link is up goes out once it is. False
    /// when the link is down. Written by the next [ready](Self::ready) or [flush](Self::flush).
    pub fn send(&mut self, bytes: &[u8], now: Instant) -> bool {
        match self.phase {
            Phase::Up => {
                self.push(
                    &Message::Data {
                        channel: 0,
                        bytes,
                    },
                    now,
                );
                true
            }
            Phase::Down => false,
            Phase::Opening | Phase::Hello | Phase::Setup => {
                self.early.push_back(bytes.to_vec());
                true
            }
        }
    }

    /// Requests waiting for the link to come up.
    pub fn waiting(&self) -> usize {
        self.early.len()
    }

    /// Writes what was pushed. Cancel-safe.
    pub async fn flush(&mut self) {
        if let Io::Framed { writer, dirty, .. } = &mut self.io
            && *dirty
        {
            match writer.flush().await {
                Ok(()) => *dirty = false,
                Err(e) => self.fail(LinkDown::Io(e)),
            }
        }
    }

    /// Ends the link: tells the backend when it is up, and drops the stream. [next_event] says
    /// [LinkDown::Closed] once.
    pub async fn close(&mut self) {
        if self.phase == Phase::Up
            && let Io::Framed { writer, .. } = &mut self.io
        {
            push(
                writer,
                &Message::Disconnect(DisconnectReason::RequestByUser),
            );
            // a backend that doesn't read any more learns it from the stream's end instead
            let _ = writer.flush().await;
        }
        self.fail(LinkDown::Closed);
    }

    /// When [next_event] wants to be called next without IO: a ping due, a timeout.
    pub fn next_wake(&self) -> Option<Instant> {
        match self.phase {
            Phase::Opening | Phase::Hello | Phase::Setup => Some(self.setup_deadline),
            Phase::Up => Some(
                (self.last_tx + PING_INTERVAL).min(self.last_rx + PEER_TIMEOUT),
            ),
            Phase::Down => None,
        }
    }

    fn handle_time(&mut self, now: Instant) {
        match self.phase {
            Phase::Opening | Phase::Hello | Phase::Setup if now >= self.setup_deadline => {
                self.fail(LinkDown::SetupTimeout);
            }
            Phase::Up if now >= self.last_rx + PEER_TIMEOUT => self.fail(LinkDown::PeerTimeout),
            Phase::Up if now >= self.last_tx + PING_INTERVAL => self.push(&Message::Ping, now),
            _ => {}
        }
    }

    fn send_early(&mut self, now: Instant) {
        while let Some(bytes) = self.early.pop_front() {
            self.push(
                &Message::Data {
                    channel: 0,
                    bytes: &bytes,
                },
                now,
            );
        }
    }

    fn push(&mut self, message: &Message<'_>, now: Instant) {
        if let Io::Framed { writer, dirty, .. } = &mut self.io {
            push(writer, message);
            *dirty = true;
            self.last_tx = now;
        }
    }

    fn consume(&mut self) {
        if let Io::Framed { reader, .. } = &mut self.io {
            reader.consume();
        }
        self.held = false;
    }

    /// The link is down for `why`, told by the next [next_event] unless it was down already.
    fn fail(&mut self, why: LinkDown) {
        if self.phase != Phase::Down {
            self.phase = Phase::Down;
            self.down = Some(why);
        }
        self.io = Io::Closed;
        self.held = false;
        self.early.clear();
    }
}

/// What a link message other than data and `LinkReady` asks for.
enum Then {
    Nothing,
    /// `DeviceInfo` came: send `LinkSetup`.
    Setup,
    Down(LinkDown),
}

/// A link message other than data and `LinkReady`, in `phase` of the link with `token`; `DeviceInfo` is kept in
/// `info`.
fn control(
    message: Message<'_>,
    phase: Phase,
    token: u64,
    info: &mut Option<DeviceInfoOwned>,
) -> Then {
    match message {
        // a hello of an earlier session on the same stream never comes, but would be harmless
        Message::Hello(hello)
            if phase == Phase::Hello && hello.role == Role::Device && hello.token == token =>
        {
            match GENERATIONS.mismatch(&hello.generations) {
                Some(mismatch) => Then::Down(LinkDown::Generations(mismatch)),
                None => Then::Nothing,
            }
        }
        Message::DeviceInfo(device) if phase == Phase::Hello && device.token == token => {
            *info = Some(device.make_owned());
            Then::Setup
        }
        Message::Disconnect(reason) => Then::Down(LinkDown::Disconnect(reason)),
        // pings, and what doesn't fit the phase: only the time counts
        _ => Then::Nothing,
    }
}

/// One step of a framed stream: write what was pushed, and wait for a message.
async fn framed_step<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut MessageReader<R>,
    writer: &mut MessageWriter<W>,
    dirty: &mut bool,
) -> Result<(), LinkDown> {
    if *dirty {
        let message = reader.wait_message();
        let flush = writer.flush();
        pin_mut!(message, flush);
        match select(message, flush).await {
            Either::Left((read, _)) => return read.map_err(LinkDown::Io),
            Either::Right((written, _)) => {
                written.map_err(LinkDown::Io)?;
                *dirty = false;
            }
        }
    }
    reader.wait_message().await.map_err(LinkDown::Io)
}

/// Queues `message` on `writer`.
fn push<W: AsyncWrite + Unpin>(writer: &mut MessageWriter<W>, message: &Message<'_>) {
    let mut scratch = [0u8; CONTROL_SCRATCH_LEN];
    let (kind, payload) = message
        .encode(&mut scratch)
        .expect("link control messages fit the scratch buffer");
    debug_assert!(
        kind != Kind::Data0 as u8 || !payload.is_empty(),
        "an empty data message is dropped by the backend"
    );
    writer.push(kind, payload);
}

/// The preamble asking for `target`, with this build's versions and generations.
fn preamble(target: &Target) -> Vec<u8> {
    let v = |v: wire_weaver::ww_version::CompactVersion| {
        ww_byte_stream::Version::new(v.major.0, v.minor.0, v.patch.0)
    };
    Preamble::new(target.clone())
        .with_versions(
            v(ww_link::LINK_VERSION),
            v(ww_client_server::COMPACT_VERSION),
        )
        .with_generations(ww_byte_stream::Generations {
            framer: GENERATIONS.framer,
            link: GENERATIONS.link,
            shrink_wrap: GENERATIONS.shrink_wrap,
            api_model: GENERATIONS.api_model,
        })
        .encode()
}

/// A random, non-zero setup token for one link: it only has to differ from an earlier session's.
fn new_setup_token() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::hash::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.finish().max(1)
}

