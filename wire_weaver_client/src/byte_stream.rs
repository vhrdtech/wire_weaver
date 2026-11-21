//! WireWeaver on a byte stream (TR-19): a local socket, TCP, the pipes of a spawned command, or a stream the program
//! opens itself ([StreamOpener]). The connection starts with the `ww_byte_stream` preamble naming what it is for (a
//! device, the hub, a service), then carries one [ww_link] message per `| len: u32 LE | kind | payload |`; a reliable
//! stream, so [ww_framer] is not used. Messages are held back until a flush, so those accumulated by the event loop
//! go out in one write.

use std::fmt;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use futures_util::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::debug;
use ww_byte_stream::{Answer, MessageReader, MessageWriter, Preamble, Status, Target, Version};

use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::transport::{
    MessageRx, MessageTx, Opened, Selected, SharedClaim, Transport, handover_requested,
};

/// The other side must answer the preamble within this time: a socket nobody serves fails fast.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// The read half of a connection.
pub(crate) type BoxRead = Box<dyn AsyncRead + Send + Unpin>;
/// The write half of a connection.
pub(crate) type BoxWrite = Box<dyn AsyncWrite + Send + Unpin>;

/// A byte stream as [StreamOpener] takes it.
pub trait ByteStream: AsyncRead + AsyncWrite + Send + Unpin + 'static {}

impl<S: AsyncRead + AsyncWrite + Send + Unpin + 'static> ByteStream for S {}

type Open =
    dyn Fn() -> Pin<Box<dyn Future<Output = io::Result<Box<dyn ByteStream>>> + Send>> + Send + Sync;

/// Opens a byte stream to a server, for [ClientConfig::stream](crate::ClientConfig::stream): a stream the program
/// makes itself (an iroh bi-stream, a socket of its own, an in-memory pipe in a test). Called for every connection,
/// so a client that reconnects opens a new stream. Configs compare equal when they hold the same opener.
#[derive(Clone)]
pub struct StreamOpener {
    open: Arc<Open>,
    shown: String,
}

impl StreamOpener {
    /// An opener shown as `shown` in errors and device info. `open` makes a futures-io stream; a tokio one is
    /// wrapped with `tokio_util::compat`.
    pub fn new<F, Fut, S>(shown: &str, open: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = io::Result<S>> + Send + 'static,
        S: ByteStream,
    {
        StreamOpener {
            open: Arc::new(move || {
                let opening = open();
                Box::pin(async move { Ok(Box::new(opening.await?) as Box<dyn ByteStream>) })
            }),
            shown: shown.to_owned(),
        }
    }
}

impl fmt::Debug for StreamOpener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StreamOpener({})", self.shown)
    }
}

impl PartialEq for StreamOpener {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.open, &other.open)
    }
}

impl Eq for StreamOpener {}

/// How to reach the server.
#[derive(Clone, Debug)]
pub(crate) enum Reach {
    /// A local socket at this path
    #[cfg_attr(
        not(feature = "local_socket"),
        allow(dead_code, reason = "only made with the `local_socket` feature")
    )]
    LocalSocket(PathBuf),
    /// TCP to `host:port`
    Tcp(String),
    /// The stdin and stdout of a command started for the connection
    Command { program: String, args: Vec<String> },
    /// A stream the program opens
    Opener(StreamOpener),
}

impl fmt::Display for Reach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reach::LocalSocket(path) => write!(f, "{}", path.display()),
            Reach::Tcp(addr) => write!(f, "tcp {addr}"),
            Reach::Command { program, args } => {
                write!(f, "command {program}")?;
                args.iter().try_for_each(|a| write!(f, " {a}"))
            }
            Reach::Opener(opener) => f.write_str(&opener.shown),
        }
    }
}

impl Reach {
    /// The medium the tap records.
    fn tap_medium(&self) -> ww_tap::Medium {
        match self {
            Reach::LocalSocket(_) => ww_tap::Medium::LocalSocket,
            _ => ww_tap::Medium::ByteStream,
        }
    }

    /// A new stream to the server, as two halves.
    async fn connect(&self) -> io::Result<(BoxRead, BoxWrite)> {
        match self {
            #[cfg(feature = "local_socket")]
            Reach::LocalSocket(path) => {
                let (stream, _peer) = ww_local_socket::connect(path).await?;
                Ok(boxed(stream))
            }
            #[cfg(not(feature = "local_socket"))]
            Reach::LocalSocket(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "wire_weaver_client is built without the `local_socket` feature",
            )),
            Reach::Tcp(addr) => {
                let stream = tokio::net::TcpStream::connect(addr).await?;
                // messages are small and flushed on purpose: Nagle would only delay them
                stream.set_nodelay(true)?;
                let (r, w) = stream.into_split();
                Ok((Box::new(r.compat()), Box::new(w.compat_write())))
            }
            Reach::Command { program, args } => spawn(program, args),
            Reach::Opener(opener) => Ok(boxed((opener.open)().await?)),
        }
    }
}

/// The halves of `stream`, boxed.
fn boxed(stream: impl ByteStream) -> (BoxRead, BoxWrite) {
    let (r, w) = stream.split();
    (Box::new(r), Box::new(w))
}

/// Starts `program` with `args`, its stdin and stdout the stream; its stderr is this program's. The command is
/// killed once the write half is dropped.
fn spawn(program: &str, args: &[String]) -> io::Result<(BoxRead, BoxWrite)> {
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err(io::Error::other("the command's pipes were not made"));
    };
    let stdin = ChildStdin {
        stdin: stdin.compat_write(),
        _child: child,
    };
    Ok((Box::new(stdout.compat()), Box::new(stdin)))
}

/// A command's stdin, which holds the command: it ends with the connection.
struct ChildStdin {
    stdin: tokio_util::compat::Compat<tokio::process::ChildStdin>,
    _child: tokio::process::Child,
}

impl AsyncWrite for ChildStdin {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        Pin::new(&mut self.stdin).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.stdin).poll_flush(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.stdin).poll_close(cx)
    }
}

/// Where to connect and what to ask for there.
#[derive(Clone, Debug)]
pub(crate) struct StreamTarget {
    pub reach: Reach,
    pub target: Target,
}

impl Tunnel {
    /// Its message halves, sharing `claim`, taking messages up to `max_message_len`.
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    pub(crate) fn into_halves(
        self,
        claim: SharedClaim,
        max_message_len: usize,
    ) -> (ByteTx, ByteRx) {
        halves((self.rx, self.tx), claim, max_message_len)
    }
}

/// The message halves of an opened connection, sharing `claim`, taking messages up to `max_message_len`.
fn halves(
    (rx, tx): (BoxRead, BoxWrite),
    claim: SharedClaim,
    max_message_len: usize,
) -> (ByteTx, ByteRx) {
    (
        ByteTx(MessageWriter::new(tx), claim.clone()),
        // + kind byte, messages above what the host advertised in LinkSetup are an error anyway
        ByteRx(MessageReader::new(rx, max_message_len), claim),
    )
}

/// A connection whose preamble was answered `Ok`: ready to carry the link.
pub(crate) struct Tunnel {
    rx: BoxRead,
    tx: BoxWrite,
    /// Where it goes, for messages and the tap
    pub(crate) shown: String,
    /// The medium the tap records
    tap_medium: ww_tap::Medium,
    /// What the hub (or the serving process) says its version is
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    pub(crate) hub_version: String,
    /// Held for a device that takes one host at a time ([ClientConfig::ipc_exclusive](
    /// crate::ClientConfig::ipc_exclusive)), for as long as the link lives
    pub(crate) claim: Option<crate::holders::Claim>,
}

/// Connects to `reach` and asks for `target` with the preamble, which carries this program's versions and
/// generations; a server that only reads the first preamble layout is asked again with it.
pub(crate) async fn open(reach: &Reach, target: Target) -> Result<Tunnel, OpenError> {
    open_preamble(reach, Preamble::new(target)).await
}

/// [open] with `preamble`, to which this program's versions and generations are added.
pub(crate) async fn open_preamble(reach: &Reach, preamble: Preamble) -> Result<Tunnel, OpenError> {
    let (link, api_model) = own_versions();
    let preamble = preamble
        .with_versions(link, api_model)
        .with_generations(own_generations());
    match open_with(reach, &preamble).await {
        Err(OpenError::Refused { answer, .. }) if answer.status == Status::PreambleVersion => {
            debug!(
                "{reach}: preamble version 1 for a hub {}",
                answer.hub_version
            );
            open_with(reach, &preamble.v1()).await
        }
        r => r,
    }
}

async fn open_with(reach: &Reach, preamble: &Preamble) -> Result<Tunnel, OpenError> {
    let shown = reach.to_string();
    let io = |e: &dyn fmt::Display| OpenError::Io(format!("{shown}: {e}"));
    let (mut rx, mut tx) = reach.connect().await.map_err(|e| io(&e))?;
    tx.write_all(&preamble.encode()).await.map_err(|e| io(&e))?;
    tx.flush().await.map_err(|e| io(&e))?;
    let answer = tokio::time::timeout(ANSWER_TIMEOUT, Answer::read(&mut rx))
        .await
        .map_err(|_| io(&"no answer to the preamble"))?
        .map_err(|e| io(&e))?;
    if answer.status != Status::Ok {
        return Err(OpenError::Refused { shown, answer });
    }
    Ok(Tunnel {
        rx,
        tx,
        shown,
        tap_medium: reach.tap_medium(),
        hub_version: answer.hub_version,
        claim: None,
    })
}

/// TCP, a command's pipes or a stream the program opens ([ClientConfig::tcp](crate::ClientConfig::tcp),
/// [command](crate::ClientConfig::command), [stream](crate::ClientConfig::stream)): opens the connection now, so
/// that a refusal (an unknown service, a busy server) is an [Error::HubRefused](crate::Error::HubRefused).
pub(crate) async fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let Some(reach) = c.pieces.iter().rev().find_map(|p| match p {
        ConfigPiece::TcpAddr { addr } => Some(Reach::Tcp(addr.clone())),
        ConfigPiece::Command { program, args } => Some(Reach::Command {
            program: program.clone(),
            args: args.clone(),
        }),
        ConfigPiece::Stream { opener } => Some(Reach::Opener(opener.clone())),
        _ => None,
    }) else {
        return Ok(Selected::NotFound { unmatched: vec![] });
    };
    let tunnel = open(&reach, service_or(c, Target::Device(String::new())))
        .await
        .map_err(OpenError::into_error)?;
    let location = tunnel.shown.clone();
    started(
        cmd_rx,
        location,
        tunnel,
        false,
        crate::event_loop::transport::link_setup_timeout::BYTE_STREAM,
    )
}

/// What to ask for: the service the config names, else `otherwise`.
pub(crate) fn service_or(c: &ValidatedConfig, otherwise: Target) -> Target {
    c.pieces
        .iter()
        .rev()
        .find_map(|p| match p {
            ConfigPiece::Service { name } => Some(Target::Service(name.clone())),
            _ => None,
        })
        .unwrap_or(otherwise)
}

/// Starts the event loop for an opened tunnel, its link set up within `link_setup_timeout`; the device is shown at
/// `location`.
pub(crate) fn started(
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
    location: String,
    tunnel: Tunnel,
    via_hub: bool,
    link_setup_timeout: Duration,
) -> Result<Selected, anyhow::Error> {
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    tokio::spawn(worker(cmd_rx, link_setup_timeout));
    let info = crate::DeviceInfo {
        location,
        manufacturer: String::new(),
        product: String::new(),
        serials: vec![],
        user_label: String::new(),
        api: None,
        usb: None,
    };
    Ok(Selected::Device {
        info: Box::new(info),
        handle: Box::new(tunnel),
        via_hub,
    })
}

/// The event loop on a byte stream, until its commander is gone.
pub(crate) async fn worker(cmd_rx: mpsc::Receiver<Command>, link_setup_timeout: Duration) {
    debug!("byte stream worker started");
    crate::event_loop::core::worker(cmd_rx, ByteStreamTransport { link_setup_timeout }).await;
    debug!("byte stream worker exited");
}

/// Opens [StreamTarget]s, or goes on with a [Tunnel] opened already.
pub(crate) struct ByteStreamTransport {
    pub(crate) link_setup_timeout: Duration,
}

impl Transport for ByteStreamTransport {
    fn link_setup_timeout(&self) -> Duration {
        self.link_setup_timeout
    }

    type Tx = ByteTx;
    type Rx = ByteRx;

    async fn connect(
        &mut self,
        handle: DeviceHandle,
        tap: crate::tap::ClientTap,
        max_message_len: usize,
    ) -> Result<Opened<ByteTx, ByteRx>, String> {
        let tunnel = match handle.downcast::<StreamTarget>() {
            Ok(to) => open(&to.reach, to.target.clone())
                .await
                .map_err(|e| e.to_string())?,
            // opened already: by connect(), or through the hub when connect() picked the route (CLT-39)
            Err(handle) => *handle
                .downcast::<Tunnel>()
                .map_err(|_| "expected a byte stream target".to_string())?,
        };
        let claim = tunnel.claim.map(Arc::new);
        let mut tap = tap;
        ww_tap::Tap::session(
            &mut tap,
            &crate::tap::host_session(tunnel.tap_medium, ww_tap::Framer::None, 0, &tunnel.shown),
        );
        let (tx, rx) = halves((tunnel.rx, tunnel.tx), claim, max_message_len);
        Ok(Opened { tx, rx })
    }
}

/// Why [open] failed.
#[derive(Debug)]
pub(crate) enum OpenError {
    /// Nothing answers there, or the connection broke
    Io(String),
    /// The other side answered, with a status but `Ok`
    Refused { shown: String, answer: Answer },
}

impl OpenError {
    /// As the client's error: a refusal is [Error::HubRefused](crate::Error::HubRefused).
    pub(crate) fn into_error(self) -> anyhow::Error {
        match self {
            OpenError::Io(e) => anyhow::anyhow!(e),
            OpenError::Refused { shown, answer } => refusal(shown, answer).into(),
        }
    }
}

/// What a refusal means for this program: for versions the hub doesn't serve (SRV-30), both sides' and the fix.
pub(crate) fn refusal(target: String, answer: Answer) -> crate::Error {
    let why = match answer.status {
        Status::Versions => versions_not_served(&answer.hub_version),
        status => status.to_string(),
    };
    crate::Error::HubRefused {
        target,
        status: answer.status.to_u8(),
        why,
        hub_version: answer.hub_version,
    }
}

/// The hub `hub_version` doesn't serve this program's versions: which side is older, and what to do.
fn versions_not_served(hub_version: &str) -> String {
    let (link, api_model) = own_versions();
    let ours = env!("CARGO_PKG_VERSION");
    let fix = match (
        semver::Version::parse(hub_version),
        semver::Version::parse(ours),
    ) {
        (Ok(hub), Ok(ours)) if hub < ours => format!(
            "the hub is older than this program (WireWeaver {ours}): stop it (`ww hub stop`) and let a newer one \
             start (install `ww` {ours} or newer: cargo install wire_weaver_cli), or connect directly with \
             WW_HUB=off while it is stopped"
        ),
        _ => format!(
            "rebuild this program with WireWeaver {hub_version} or newer (it is built with {ours}), or stop the \
             hub (`ww hub stop`) and connect directly with WW_HUB=off while it is stopped"
        ),
    };
    format!(
        "the ww hub {hub_version} doesn't serve this program's ww_link {link} and ww_client_server {api_model}: {fix}"
    )
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Io(e) => f.write_str(e),
            OpenError::Refused { shown, answer } => write!(
                f,
                "{shown}: {} (served by version {})",
                answer.status, answer.hub_version
            ),
        }
    }
}

/// This program's `ww_link` and `ww_client_server` versions, for the preamble (SRV-30).
fn own_versions() -> (Version, Version) {
    let v = |v: ww_version::CompactVersion| Version::new(v.major.0, v.minor.0, v.patch.0);
    (
        v(ww_link::LINK_VERSION),
        v(ww_client_server::COMPACT_VERSION),
    )
}

/// This program's generations, for the preamble (EVO-23): the same bytes as in its link hello.
fn own_generations() -> ww_byte_stream::Generations {
    let g = crate::event_loop::core::GENERATIONS;
    ww_byte_stream::Generations {
        framer: g.framer,
        link: g.link,
        shrink_wrap: g.shrink_wrap,
        api_model: g.api_model,
    }
}

/// The claim is shared by both halves: released once both closed the stream.
pub(crate) struct ByteTx(MessageWriter<BoxWrite>, SharedClaim);

impl MessageTx for ByteTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        self.0.push(kind, message);
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), String> {
        self.0.flush().await.map_err(|e| e.to_string())
    }

    async fn handover_requested(&mut self) {
        handover_requested(&self.1).await
    }

    async fn close(mut self) {
        let _ = self.0.flush().await;
        let _ = self.0.close().await;
    }
}

pub(crate) struct ByteRx(
    MessageReader<BoxRead>,
    // held, never read: the claim goes with the last half
    #[allow(
        dead_code,
        reason = "held, never read: the claim goes with the last half"
    )]
    SharedClaim,
);

impl MessageRx for ByteRx {
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        // cancel-safe: a partly received message stays in the reader
        self.0.read_message().await.map_err(|e| match e.kind() {
            std::io::ErrorKind::UnexpectedEof => "closed by the other side".to_string(),
            _ => e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    //! The host event loop against [ww_device::Server] on a byte-stream medium (`ww_byte_stream::medium`), over TCP
    //! and over a pipe pair like a command's stdin and stdout.
    use tokio_util::compat::TokioAsyncReadCompatExt;
    use ww_byte_stream::PreambleError;
    use ww_device::LinkEvent;

    use super::*;
    use crate::event_loop::device_e2e_tests::{DEV_MAX_MESSAGE, serve, talk};

    /// Connects every time to `self.0`, whatever handle the test's connect command carries.
    struct To(StreamTarget);

    impl Transport for To {
        fn link_setup_timeout(&self) -> Duration {
            crate::event_loop::transport::link_setup_timeout::IN_PROCESS
        }

        type Tx = ByteTx;
        type Rx = ByteRx;

        async fn connect(
            &mut self,
            _: DeviceHandle,
            tap: crate::tap::ClientTap,
            max_message_len: usize,
        ) -> Result<Opened<ByteTx, ByteRx>, String> {
            ByteStreamTransport {
                link_setup_timeout: self.link_setup_timeout(),
            }
            .connect(Box::new(self.0.clone()), tap, max_message_len)
            .await
        }
    }

    /// A device on `stream`: answers the preamble as a server serving one device does, then runs the e2e device.
    async fn device(
        mut stream: impl ByteStream,
        events_tx: tokio::sync::mpsc::UnboundedSender<LinkEvent>,
    ) {
        let status = match Preamble::read(&mut stream).await {
            Ok(Preamble {
                target: Target::Device(_),
                ..
            }) => Status::Ok,
            Ok(_) => Status::UnknownService,
            Err(PreambleError::Io(_)) => return,
            Err(e) => panic!("bad preamble: {e}"),
        };
        stream
            .write_all(&Answer::new(status, "test 0.1.0").encode())
            .await
            .unwrap();
        if status != Status::Ok {
            return;
        }
        let (tx, rx) = ww_byte_stream::medium::medium(stream, DEV_MAX_MESSAGE);
        serve(tx, rx, events_tx).await
    }

    type Started = (
        mpsc::Sender<Command>,
        mpsc::UnboundedReceiver<LinkEvent>,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    );

    fn host(reach: Reach) -> (mpsc::Sender<Command>, tokio::task::JoinHandle<()>) {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let to = To(StreamTarget {
            reach,
            target: Target::Device(String::new()),
        });
        (
            cmd_tx,
            tokio::spawn(crate::event_loop::core::worker(cmd_rx, to)),
        )
    }

    async fn on_tcp() -> Started {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let dev = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            device(stream.compat(), events_tx).await
        });
        let (cmd_tx, host) = host(Reach::Tcp(addr.to_string()));
        (cmd_tx, events_rx, dev, host)
    }

    /// Two pipes, as a spawned command's stdin and stdout, opened by a [StreamOpener].
    fn on_pipes() -> Started {
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (dev_side, host_side) = tokio::io::duplex(4096);
        let dev = tokio::spawn(device(dev_side.compat(), events_tx));
        let host_side = std::sync::Mutex::new(Some(host_side));
        let opener = StreamOpener::new("pipes", move || {
            let stream = host_side.lock().unwrap().take();
            async move {
                stream
                    .map(TokioAsyncReadCompatExt::compat)
                    .ok_or_else(|| io::Error::other("the pipes were taken"))
            }
        });
        let (cmd_tx, host) = host(Reach::Opener(opener));
        (cmd_tx, events_rx, dev, host)
    }

    #[tokio::test]
    async fn host_and_device_talk_over_tcp() {
        talk(on_tcp().await).await;
    }

    #[tokio::test]
    async fn host_and_device_talk_over_a_pipe_pair() {
        talk(on_pipes()).await;
    }

    /// A service the server doesn't serve is a refusal naming it, not a timeout.
    #[tokio::test]
    async fn an_unknown_service_is_refused() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            device(stream.compat(), mpsc::unbounded_channel().0).await
        });
        let Err(OpenError::Refused { shown, answer }) = open(
            &Reach::Tcp(addr.to_string()),
            Target::Service("journal".into()),
        )
        .await
        else {
            panic!("refused");
        };
        assert_eq!(answer.status, Status::UnknownService);
        assert_eq!(shown, format!("tcp {addr}"));
    }

    /// Configs with the same opener compare equal, with another one not.
    #[test]
    fn openers_compare_by_identity() {
        let open = || async { Ok(futures_util::io::Cursor::new(Vec::<u8>::new())) };
        let a = StreamOpener::new("a", open);
        let b = StreamOpener::new("a", open);
        assert_eq!(a, a.clone());
        assert_ne!(a, b);
        assert_eq!(format!("{a:?}"), "StreamOpener(a)");
    }
}
