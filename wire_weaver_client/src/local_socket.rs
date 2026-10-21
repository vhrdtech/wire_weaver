//! Local socket transport (TR-16): a device served by another process on the same PC, over a Unix domain socket
//! (Linux, macOS) or a named pipe (Windows), see [ww_local_socket]. Reached with
//! [ClientConfig::ipc_path](crate::ClientConfig::ipc_path), or through the ww hub (SRV-13, see [crate::hub]).
//!
//! The connection starts with the hub [Preamble] naming the device, then carries one
//! [ww_link] message per `| len: u32 LE | kind | payload |`; a reliable stream, so [ww_framer] is not used. Link
//! setup, version checks, pings and timeouts are the same as over USB or WebSocket. Messages are held back until
//! a flush, so those accumulated by the event loop go out in one write.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::bail;
use interprocess::local_socket::tokio::{RecvHalf, SendHalf};
use interprocess::local_socket::traits::tokio::Stream as _;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tracing::debug;
use ww_local_socket::{Answer, MessageReader, MessageWriter, Preamble, Status, Target};

use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::transport::{MessageRx, MessageTx, Opened, Selected, Transport};

/// The other process must answer the preamble within this time, a socket nobody serves fails fast.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// Where to connect: the socket and the device behind it.
#[derive(Clone, Debug)]
pub(crate) struct LocalTarget {
    pub path: PathBuf,
    pub target: Target,
}

pub(crate) fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let to = c.pieces.iter().rev().find_map(|p| match p {
        // the one device the process serves
        ConfigPiece::IpcPath { path } => Some(Ok(LocalTarget {
            path: PathBuf::from(path),
            target: Target::Device(String::new()),
        })),
        ConfigPiece::HubApi { path } => Some(
            match path {
                Some(path) => Ok(PathBuf::from(path)),
                None => ww_local_socket::runtime_dir()
                    .map(|d| d.join(ww_local_socket::files::HUB_SOCKET)),
            }
            .map(|path| LocalTarget {
                path,
                target: Target::Hub,
            }),
        ),
        _ => None,
    });
    let Some(to) = to.transpose()? else {
        // devices behind the hub are listed by it (CLT-39), nothing to discover here yet
        return Ok(Selected::NotFound { unmatched: vec![] });
    };
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    tokio::spawn(worker(cmd_rx));
    Ok(Selected::Device {
        info: Box::new(device_info(&to.path.display().to_string())),
        handle: Box::new(to),
    })
}

/// The event loop on a local socket, until its commander is gone.
pub(crate) async fn worker(cmd_rx: mpsc::Receiver<Command>) {
    debug!("local socket worker started");
    crate::event_loop::core::worker(cmd_rx, LocalSocketTransport).await;
    debug!("local socket worker exited");
}

fn device_info(path: &str) -> crate::DeviceInfo {
    crate::DeviceInfo {
        location: format!("local socket {path}"),
        manufacturer: String::new(),
        product: String::new(),
        serials: vec![],
        user_label: String::new(),
        api: None,
        usb: None,
    }
}

pub(crate) struct LocalSocketTransport;

impl Transport for LocalSocketTransport {
    fn link_setup_timeout(&self) -> Duration {
        crate::event_loop::transport::link_setup_timeout::LOCAL_SOCKET
    }

    type Tx = LocalTx;
    type Rx = LocalRx;

    async fn connect(
        &mut self,
        handle: DeviceHandle,
        tap: ww_tap::RecorderLink,
    ) -> Result<Opened<LocalTx, LocalRx>, String> {
        let tunnel = match handle.downcast::<LocalTarget>() {
            Ok(to) => match open(&to.path, to.target.clone()).await {
                Ok(tunnel) => tunnel,
                Err(e) => return Err(e.to_string()),
            },
            // opened already, e.g. through the hub when connect() picked the route (CLT-39)
            Err(handle) => *handle
                .downcast::<Tunnel>()
                .map_err(|_| "expected a local socket target".to_string())?,
        };
        let mut tap = tap;
        ww_tap::Tap::session(
            &mut tap,
            &crate::tap::host_session(
                ww_tap::Medium::LocalSocket,
                ww_tap::Framer::None,
                0,
                &tunnel.shown,
            ),
        );
        Ok(Opened {
            tx: LocalTx(MessageWriter::new(tunnel.tx)),
            // + kind byte, messages above what the host advertised in LinkSetup are an error anyway
            rx: LocalRx(MessageReader::new(
                tunnel.rx,
                crate::DEFAULT_MAX_MESSAGE_SIZE,
            )),
        })
    }
}

/// A connection whose preamble was answered `Ok`: ready to carry the link.
pub(crate) struct Tunnel {
    rx: RecvHalf,
    tx: SendHalf,
    /// The socket, for messages and the tap
    pub(crate) shown: String,
}

/// Why [open] failed.
#[derive(Debug)]
pub(crate) enum OpenError {
    /// Nothing answers there, or the connection broke
    Io(String),
    /// The other side answered, with a status but `Ok`
    Refused { shown: String, answer: Answer },
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

/// Connects to the socket at `path` and asks for `target` with the preamble.
pub(crate) async fn open(path: &std::path::Path, target: Target) -> Result<Tunnel, OpenError> {
    let shown = path.display().to_string();
    let io = |e: &dyn std::fmt::Display| OpenError::Io(format!("{shown}: {e}"));
    let (stream, _peer) = ww_local_socket::connect(path).await.map_err(|e| io(&e))?;
    let (mut rx, mut tx) = stream.split();
    tx.write_all(&Preamble::new(target).encode())
        .await
        .map_err(|e| io(&e))?;
    let answer = tokio::time::timeout(ANSWER_TIMEOUT, Answer::read(&mut rx))
        .await
        .map_err(|_| io(&"no answer to the preamble"))?
        .map_err(|e| io(&e))?;
    if answer.status != Status::Ok {
        return Err(OpenError::Refused { shown, answer });
    }
    Ok(Tunnel { rx, tx, shown })
}

pub(crate) struct LocalTx(MessageWriter<SendHalf>);

impl MessageTx for LocalTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        self.0.push(kind, message);
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), String> {
        self.0.flush().await.map_err(|e| e.to_string())
    }

    async fn close(mut self) {
        let _ = self.0.flush().await;
        let _ = self.0.shutdown().await;
    }
}

pub(crate) struct LocalRx(MessageReader<RecvHalf>);

impl MessageRx for LocalRx {
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        // cancel-safe: a partly received message stays in the reader
        self.0.read_message().await.map_err(|e| match e.kind() {
            std::io::ErrorKind::UnexpectedEof => {
                "local socket closed by the other side".to_string()
            }
            _ => e.to_string(),
        })
    }
}

/// Device side: a [ww_device::Server] medium for a device served by this process on a local socket
/// ([ClientConfig::ipc_path](crate::ClientConfig::ipc_path) on the client): host-emulated firmware,
/// simulators, a program publishing a virtual device. One client at a time, like the WebSocket medium; the next one
/// waits in the backlog until the current one leaves.
///
/// ```ignore
/// let listener = ww_local_socket::listen(&path, true)?;
/// let (tx, rx) = wire_weaver_client::local_socket::device_medium(listener, "sim 0.1.0", 1024);
/// let mut server = ww_device::Server::new(link_config, tx, rx, TokioClock::new(), &mut scratch, &mut events);
/// ```
pub mod device {
    use std::time::Duration;

    use interprocess::local_socket::tokio::{RecvHalf, SendHalf};
    use interprocess::local_socket::traits::tokio::Stream as _;
    use tokio::io::AsyncWriteExt;
    use tokio::sync::mpsc;
    use tracing::{debug, warn};
    use ww_local_socket::{
        Answer, Listener, MessageReader, MessageWriter, Preamble, PreambleError, Status, Target,
    };

    /// A client that connected must send its preamble within this time, or it is dropped.
    const PREAMBLE_TIMEOUT: Duration = Duration::from_secs(2);

    /// Tx and rx halves for [ww_device::Server] on `listener`. `version` goes into the preamble answer (shown to a
    /// client that is refused), `max_message_len` is the longest message accepted, the device's own limit.
    pub fn device_medium(
        listener: Listener,
        version: &str,
        max_message_len: usize,
    ) -> (DeviceTx, DeviceRx) {
        let (writer_tx, writer_rx) = mpsc::unbounded_channel();
        (
            DeviceTx {
                writer: None,
                new_writer: writer_rx,
            },
            DeviceRx {
                listener: Some((listener, version.to_string())),
                max: max_message_len,
                reader: None,
                writer_tx,
                ended: None,
            },
        )
    }

    /// Tx and rx halves for [ww_device::Server] on one connection whose preamble was already answered, e.g. by the
    /// hub, which accepts every connection itself and picks what serves it by the preamble. Once the client leaves,
    /// the medium stays down for good and [Ended] says so (the link may never have come up, so a link event alone
    /// doesn't tell): the caller ends the server then.
    pub fn connection_medium(
        rx: RecvHalf,
        tx: SendHalf,
        max_message_len: usize,
    ) -> (DeviceTx, DeviceRx, Ended) {
        let (writer_tx, writer_rx) = mpsc::unbounded_channel();
        let (ended_tx, ended_rx) = tokio::sync::watch::channel(false);
        (
            DeviceTx {
                writer: Some(MessageWriter::new(tx)),
                new_writer: writer_rx,
            },
            DeviceRx {
                listener: None,
                max: max_message_len,
                reader: Some(MessageReader::new(rx, max_message_len)),
                writer_tx,
                ended: Some(ended_tx),
            },
            Ended(ended_rx),
        )
    }

    /// The single connection of a [connection_medium] ended.
    pub struct Ended(tokio::sync::watch::Receiver<bool>);

    impl Ended {
        /// Returns once the client is gone. Cancel-safe.
        pub async fn wait(&mut self) {
            let _ = self.0.wait_for(|e| *e).await;
        }
    }

    #[derive(Debug)]
    pub enum Error {
        Io(std::io::Error),
        /// No client connected
        Closed,
    }

    pub struct DeviceTx {
        writer: Option<MessageWriter<SendHalf>>,
        /// The writer of each newly accepted client, from [DeviceRx::wait_connected]
        new_writer: mpsc::UnboundedReceiver<MessageWriter<SendHalf>>,
    }

    impl DeviceTx {
        fn take_new(&mut self) {
            while let Ok(w) = self.new_writer.try_recv() {
                self.writer = Some(w);
            }
        }
    }

    impl ww_device::MessageTx for DeviceTx {
        type Error = Error;

        async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), Error> {
            self.take_new();
            let w = self.writer.as_mut().ok_or(Error::Closed)?;
            w.push(kind, message);
            Ok(())
        }

        async fn flush(&mut self) -> Result<(), Error> {
            self.take_new();
            let Some(w) = self.writer.as_mut() else {
                return Ok(());
            };
            let r = w.flush().await;
            if r.is_err() {
                // the client is gone, the next one comes with its own writer
                self.writer = None;
            }
            r.map_err(Error::Io)
        }

        /// Drops what is held back; the connection stays (the server also calls this when the link comes up).
        fn reset(&mut self) {
            self.take_new();
            if let Some(w) = self.writer.as_mut() {
                w.clear();
            }
        }
    }

    pub struct DeviceRx {
        /// Accepts the next client, with the version for the preamble answer; `None` for a single connection
        listener: Option<(Listener, String)>,
        max: usize,
        reader: Option<MessageReader<RecvHalf>>,
        writer_tx: mpsc::UnboundedSender<MessageWriter<SendHalf>>,
        /// Single connection: told when it ends
        ended: Option<tokio::sync::watch::Sender<bool>>,
    }

    impl DeviceRx {
        /// Accepts one client: reads its preamble, answers, hands the write half to [DeviceTx].
        async fn accept_one(&mut self) -> std::io::Result<bool> {
            let Some((listener, version)) = &self.listener else {
                return Ok(false);
            };
            let (stream, peer) = listener.accept().await?;
            let (mut rx, mut tx) = stream.split();
            let preamble = tokio::time::timeout(PREAMBLE_TIMEOUT, Preamble::read(&mut rx)).await;
            let status = match preamble {
                Err(_) => {
                    debug!("local socket: client {peer:?} sent no preamble");
                    return Ok(false);
                }
                Ok(Err(PreambleError::UnsupportedVersion(v))) => {
                    debug!("local socket: preamble version {v} from {peer:?}");
                    Status::PreambleVersion
                }
                Ok(Err(e)) => {
                    debug!("local socket: bad preamble from {peer:?}: {e}");
                    return Ok(false);
                }
                // the one device served here: any key names it, there is no other
                Ok(Ok(Preamble {
                    target: Target::Device(_),
                    ..
                })) => Status::Ok,
                Ok(Ok(Preamble {
                    target: Target::Hub,
                    ..
                })) => Status::UnknownDevice,
            };
            tx.write_all(&Answer::new(status, version).encode()).await?;
            if status != Status::Ok {
                return Ok(false);
            }
            debug!("local socket: client {peer:?} connected");
            self.reader = Some(MessageReader::new(rx, self.max));
            let _ = self.writer_tx.send(MessageWriter::new(tx));
            Ok(true)
        }
    }

    impl ww_device::MessageRx for DeviceRx {
        type Error = Error;

        async fn wait_message(&mut self) -> Result<(), Error> {
            let r = self.reader.as_mut().ok_or(Error::Closed)?;
            let result = r.wait_message().await;
            if result.is_err() {
                // closed or out of step: wait_connected accepts the next client
                self.reader = None;
                if let Some(ended) = &self.ended {
                    ended.send_replace(true);
                }
            }
            result.map_err(Error::Io)
        }

        fn message(&self) -> Option<(u8, &[u8])> {
            self.reader.as_ref()?.message()
        }

        fn consume(&mut self) {
            if let Some(r) = self.reader.as_mut() {
                r.consume();
            }
        }

        async fn wait_connected(&mut self) {
            if self.reader.is_some() {
                return;
            }
            if self.listener.is_none() {
                // a single connection that ended: nothing comes after it
                return std::future::pending().await;
            }
            loop {
                match self.accept_one().await {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(e) => {
                        warn!("local socket: accept failed: {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }

        /// Nothing partial to drop on a stream that is in step, and the connection stays: the server also calls this
        /// when the link comes up. A broken connection was already dropped by `wait_message`.
        fn reset(&mut self) {
            self.consume();
        }

        fn max_message_len(&self) -> usize {
            self.max
        }
    }
}

#[cfg(test)]
mod tests {
    //! Host event loop against [ww_device::Server] on the device medium above, over a real local socket.
    use super::*;
    use crate::event_loop::device_e2e_tests::{DEV_MAX_MESSAGE, serve, talk};
    use ww_device::LinkEvent;

    struct To(LocalTarget);

    impl Transport for To {
        fn link_setup_timeout(&self) -> Duration {
            crate::event_loop::transport::link_setup_timeout::IN_PROCESS
        }

        type Tx = LocalTx;
        type Rx = LocalRx;
        async fn connect(
            &mut self,
            _: DeviceHandle,
            tap: ww_tap::RecorderLink,
        ) -> Result<Opened<LocalTx, LocalRx>, String> {
            LocalSocketTransport
                .connect(Box::new(self.0.clone()), tap)
                .await
        }
    }

    fn socket_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ww-ls-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("dev.sock")
    }

    async fn start(
        path: PathBuf,
    ) -> (
        mpsc::Sender<Command>,
        mpsc::UnboundedReceiver<LinkEvent>,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = ww_local_socket::listen(&path, true).unwrap();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let dev = tokio::spawn(async move {
            let (tx, rx) = device::device_medium(listener, "test 0.1.0", DEV_MAX_MESSAGE);
            serve(tx, rx, events_tx).await
        });
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let host = tokio::spawn(crate::event_loop::core::worker(
            cmd_rx,
            To(LocalTarget {
                path,
                target: Target::Device(String::new()),
            }),
        ));
        (cmd_tx, events_rx, dev, host)
    }

    #[tokio::test]
    async fn host_and_device_talk_over_a_local_socket() {
        let path = socket_path("talk");
        talk(start(path.clone()).await).await;
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn nothing_listening_fails_connect() {
        let path = socket_path("none");
        let r = LocalSocketTransport
            .connect(
                Box::new(LocalTarget {
                    path: path.clone(),
                    target: Target::Device(String::new()),
                }),
                crate::tap::default_recorder().link(),
            )
            .await;
        let Err(e) = r else {
            panic!("connected to nothing")
        };
        assert!(e.contains("dev.sock"), "{e}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn refusal_names_status_and_version() {
        let path = socket_path("refuse");
        let listener = ww_local_socket::listen(&path, true).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut rx, mut tx) = stream.split();
            Preamble::read(&mut rx).await.unwrap();
            tx.write_all(&Answer::new(Status::Versions, "hub 9.0.0").encode())
                .await
                .unwrap();
        });
        let r = LocalSocketTransport
            .connect(
                Box::new(LocalTarget {
                    path: path.clone(),
                    target: Target::Device("usb:c0de:cafe:1".into()),
                }),
                crate::tap::default_recorder().link(),
            )
            .await;
        let Err(e) = r else { panic!("refused") };
        assert!(
            e.contains("versions are not served") && e.contains("hub 9.0.0"),
            "{e}"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
