//! UDP [Transport]: one [ww_framer] frame per datagram, with the UDP framer configuration from [ww_link].
//!
//! Datagrams keep their boundaries, so framing is the same as over USB: small messages accumulated by the event
//! loop share a datagram, big ones are split across several. Datagrams sent by the host are at most
//! [ww_link::UDP_MAX_DATAGRAM_LEN], any size is accepted from the device.
//!
//! UDP is unreliable and nothing is retransmitted: a lost request times out, a lost stream event is gone, a split
//! message with a lost or reordered piece is dropped by the framer. Link setup, version checks, pings and timeouts
//! are the same as over USB or RTT, see [ww_link].

use std::net::SocketAddr;

use anyhow::bail;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, trace};

use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::transport::{MessageRx, MessageTx, Opened, Selected, Transport};

type TxFramer = ww_framer::TxOwned<ww_link::UdpHead, ww_link::UdpChecksum, ww_link::UdpTail>;
type RxFramer = ww_framer::FramedRxOwned<ww_link::UdpHead, ww_link::UdpChecksum, ww_link::UdpTail>;

/// Largest UDP payload, datagrams are received whole whatever their size
const MAX_RX_DATAGRAM_LEN: usize = 65_535;

pub(crate) fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let Some(addr) = c.pieces.iter().rev().find_map(|p| match p {
        ConfigPiece::UdpAddr { addr } => Some(addr.clone()),
        _ => None,
    }) else {
        // no discovery yet, nothing to list
        return Ok(Selected::NotFound { unmatched: vec![] });
    };
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    tokio::spawn(async move {
        debug!("udp worker started");
        crate::event_loop::core::worker(cmd_rx, UdpTransport).await;
        debug!("udp worker exited");
    });
    Ok(Selected::Device {
        info: Box::new(device_info(&addr)),
        handle: Box::new(addr),
    })
}

fn device_info(addr: &str) -> crate::DeviceInfo {
    crate::DeviceInfo {
        location: format!("udp {addr}"),
        manufacturer: String::new(),
        product: String::new(),
        serials: vec![],
        user_label: String::new(),
        api: None,
        usb: None,
    }
}

struct UdpTransport;

impl Transport for UdpTransport {
    type Tx = UdpTx;
    type Rx = UdpRx;

    async fn connect(&mut self, handle: DeviceHandle) -> Result<Opened<UdpTx, UdpRx>, String> {
        let addr = handle
            .downcast::<String>()
            .map_err(|_| "expected UDP address".to_string())?;
        let socket = connect(&addr).await.map_err(|e| format!("{addr}: {e}"))?;
        let socket = std::sync::Arc::new(socket);
        Ok(Opened {
            tx: UdpTx {
                framer: TxFramer::new(ww_link::UDP_MAX_DATAGRAM_LEN),
                socket: socket.clone(),
            },
            rx: UdpRx {
                // must hold one maximum message plus one more frame, see FramedRx docs
                framer: RxFramer::new(crate::DEFAULT_MAX_MESSAGE_SIZE + MAX_RX_DATAGRAM_LEN),
                datagram: vec![0; MAX_RX_DATAGRAM_LEN],
                socket,
            },
        })
    }
}

/// Socket connected to the device: only its datagrams are received, and ICMP port unreachable (nothing listens
/// there) fails the next read or write instead of going unnoticed until the link setup times out.
async fn connect(addr: &str) -> std::io::Result<UdpSocket> {
    let peer = tokio::net::lookup_host(addr)
        .await?
        .next()
        .ok_or_else(|| std::io::Error::other("no addresses found"))?;
    let local: SocketAddr = if peer.is_ipv4() {
        "0.0.0.0:0".parse().expect("valid")
    } else {
        "[::]:0".parse().expect("valid")
    };
    let socket = UdpSocket::bind(local).await?;
    socket.connect(peer).await?;
    Ok(socket)
}

struct UdpTx {
    framer: TxFramer,
    socket: std::sync::Arc<UdpSocket>,
}

impl UdpTx {
    async fn send_frame(&mut self) -> Result<bool, String> {
        let len = self.framer.flush();
        if len == 0 {
            return Ok(false);
        }
        let frame = &self.framer.buf()[..len];
        trace!("tx datagram: {len}: {frame:02x?}");
        self.socket.send(frame).await.map_err(describe_io_error)?;
        Ok(true)
    }
}

impl MessageTx for UdpTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        loop {
            match self.framer.write(kind, message) {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    // datagram is full (or the message continues into the next one): send and retry
                    if !self.send_frame().await? {
                        return Err("framing error: cannot progress".into());
                    }
                }
                Err(()) => {
                    return Err(format!(
                        "framing error: message too big ({})",
                        message.len()
                    ));
                }
            }
        }
    }

    async fn flush(&mut self) -> Result<(), String> {
        self.send_frame().await.map(|_| ())
    }
}

struct UdpRx {
    framer: RxFramer,
    /// Receive buffer, a datagram is staged into the framer right away
    datagram: Vec<u8>,
    socket: std::sync::Arc<UdpSocket>,
}

impl MessageRx for UdpRx {
    /// Cancel-safe: staged datagrams live in the framer, the only await is `recv`, which is cancel-safe.
    /// Several messages per datagram are returned one per call.
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        loop {
            // Consumes the message returned by the previous call (if any) and tries to assemble
            // the next one from what is already staged, before reading more.
            self.framer.reassemble();
            if self.framer.message().is_some() {
                break;
            }
            let len = self
                .socket
                .recv(&mut self.datagram)
                .await
                .map_err(describe_io_error)?;
            let datagram = &self.datagram[..len];
            trace!("rx datagram: {len}: {datagram:02x?}");
            if self.framer.stage(datagram).is_err() {
                return Err("out of staging area, this is a bug".into());
            }
        }
        // second lookup instead of returning from inside the loop: keeps the borrow checker happy
        Ok(self.framer.message().expect("checked above"))
    }
}

fn describe_io_error(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::ConnectionRefused {
        format!("UDP: {e} (nothing listens on the device port)")
    } else {
        format!("UDP: {e}")
    }
}

#[cfg(test)]
mod tests {
    //! Host event loop against [ww_device::Server] over real UDP sockets on localhost.
    use super::*;
    use crate::event_loop::device_e2e_tests::{
        DEV_MAX_MESSAGE, connect as connect_cmd, serve, talk,
    };
    use ww_device::LinkEvent;

    /// Tests send `()` as a handle
    struct To(String);

    impl Transport for To {
        type Tx = UdpTx;
        type Rx = UdpRx;
        async fn connect(&mut self, _: DeviceHandle) -> Result<Opened<UdpTx, UdpRx>, String> {
            UdpTransport.connect(Box::new(self.0.clone())).await
        }
    }

    // Device side: ww_device's UDP medium on a tokio socket

    struct DevSocket(UdpSocket);

    impl ww_device::udp::DatagramSocket for DevSocket {
        type Addr = SocketAddr;
        type RecvError = std::io::Error;
        type SendError = std::io::Error;
        async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
            self.0.recv_from(buf).await
        }
        async fn send_to(&self, datagram: &[u8], addr: SocketAddr) -> std::io::Result<()> {
            self.0.send_to(datagram, addr).await.map(|_| ())
        }
    }

    async fn device(socket: UdpSocket, events_tx: mpsc::UnboundedSender<LinkEvent>) {
        use ww_device::udp::{UdpConnection, UdpSink, UdpSource};
        const DATAGRAM: usize = ww_link::UDP_MAX_DATAGRAM_LEN;
        let conn = UdpConnection::new(DevSocket(socket));
        let mut tx_frame = [0u8; DATAGRAM];
        let mut rx = ww_device::RxBuffer::<DATAGRAM, DEV_MAX_MESSAGE>::new();
        serve(
            ww_device::FramedTx::new(UdpSink::new(&conn), &mut tx_frame),
            ww_device::FramedRx::new(UdpSource::new(&conn), rx.assembly_buf(DATAGRAM)),
            events_tx,
        )
        .await
    }

    /// Device on a localhost socket, returns its address
    async fn start_device() -> (
        String,
        mpsc::UnboundedReceiver<LinkEvent>,
        tokio::task::JoinHandle<()>,
    ) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap().to_string();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        // the medium is not Send (a Cell for the peer, as on embassy), run it on the test's LocalSet
        let dev = tokio::task::spawn_local(device(socket, events_tx));
        (addr, events_rx, dev)
    }

    /// Host event loop, each on its own socket (port)
    fn start_host(addr: String) -> (mpsc::Sender<Command>, tokio::task::JoinHandle<()>) {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let host = tokio::spawn(crate::event_loop::core::worker(cmd_rx, To(addr)));
        (cmd_tx, host)
    }

    async fn start_udp() -> (
        mpsc::Sender<Command>,
        mpsc::UnboundedReceiver<LinkEvent>,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let (addr, events_rx, dev) = start_device().await;
        let (cmd_tx, host) = start_host(addr);
        (cmd_tx, events_rx, dev, host)
    }

    #[tokio::test]
    async fn host_and_device_talk_over_udp() {
        tokio::task::LocalSet::new()
            .run_until(async { talk(start_udp().await).await })
            .await
    }

    /// Echo request with `i` in it, as `talk()` sends them
    async fn echo(cmd_tx: &mpsc::Sender<Command>, i: u32) -> Vec<u8> {
        use wire_weaver::shrink_wrap::UVlq32Backfill;
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let bytes = [0u8; UVlq32Backfill::LEN]
            .into_iter()
            .chain(i.to_le_bytes())
            .collect();
        cmd_tx
            .send(Command::SendMessage {
                bytes,
                done_tx: Some((done_tx, std::time::Duration::from_secs(5))),
            })
            .await
            .unwrap();
        done_rx.await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn second_host_takes_over() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (addr, mut events_rx, dev) = start_device().await;
                let (cmd_tx, host) = start_host(addr.clone());
                connect_cmd(&cmd_tx, (0, 3)).await.result.unwrap();
                assert_eq!(events_rx.recv().await, Some(LinkEvent::Up));
                assert_eq!(&echo(&cmd_tx, 1).await[..4], 1u32.to_le_bytes());

                // another host connects while the first one is up, e.g., the first one crashed
                let (cmd_tx2, host2) = start_host(addr);
                connect_cmd(&cmd_tx2, (0, 3)).await.result.unwrap();
                assert_eq!(
                    events_rx.recv().await,
                    Some(LinkEvent::Down(ww_device::DownReason::Transport))
                );
                assert_eq!(events_rx.recv().await, Some(LinkEvent::Up));
                assert_eq!(&echo(&cmd_tx2, 2).await[..4], 2u32.to_le_bytes());

                drop((cmd_tx, cmd_tx2));
                host.await.unwrap();
                host2.await.unwrap();
                dev.abort();
            })
            .await
    }

    #[tokio::test]
    async fn nothing_listening_fails_link_setup() {
        // bound and dropped: nothing listens there
        let addr = UdpSocket::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let host = tokio::spawn(crate::event_loop::core::worker(
            cmd_rx,
            To(addr.to_string()),
        ));
        let r = connect_cmd(&cmd_tx, (0, 3)).await.result;
        let Err(e) = r else {
            panic!("connected to nothing")
        };
        // Linux and macOS report ICMP port unreachable on a connected socket, so this does not wait for the
        // GetDeviceInfo retries to run out
        let e = format!("{e:#}");
        assert!(e.contains("UDP"), "{e}");
        #[cfg(unix)]
        assert!(e.contains("nothing listens"), "{e}");
        drop(cmd_tx);
        host.await.unwrap();
    }

    #[tokio::test]
    async fn unresolvable_name_fails_connect() {
        let r = UdpTransport
            .connect(Box::new("no-such-host.invalid:9000".to_string()))
            .await;
        let Err(e) = r else {
            panic!("resolved an invalid name")
        };
        assert!(e.contains("no-such-host.invalid"), "{e}");
    }
}
