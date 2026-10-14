//! The simulated device: a thread that boots (the bootloader runs on [Flash]), runs a firmware on one link until it
//! resets (a reboot request or a power cut), and boots again, always on the same link endpoint, as a real device
//! comes back under the same USB serial, IP address and port.
//!
//! The link is a one-line choice per scenario ([Link]). Every medium is the real one, from the `ww_device` side up:
//! - USB: `ww_device`'s `FramedTx` / `FramedRx` with the USB framer configuration over the simulated USB of
//!   `wire_weaver_client::in_process` (real packet sizes; no enumeration, toggles or NAKs);
//! - WebSocket: `ww_device::ws` over a TCP socket on localhost, `wire_weaver_client`'s `ws` transport;
//! - UDP: `ww_device::udp` over a UDP socket on localhost, `wire_weaver_client`'s `udp` transport.

use std::sync::{Arc, Mutex};

use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver::{EventWriter, MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_client::in_process::{self, TokioClock};
use ww_device::{DisconnectReason, LinkConfig};
use ww_dfu::{BootTarget, Offers, Role};

use crate::firmware::{self, Firmware};
use crate::flash::{self, Flash};
use crate::node::{self, Node};
use crate::record::{Dir, Recorder, TapPackets, TapRx, TapTx, WsSplitter};

/// Longest message the device takes and sends.
pub const MAX_MESSAGE: usize = 2048;

/// The medium between host and device.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// Simulated USB, 64-byte packets.
    UsbFullSpeed,
    /// Simulated USB, 512-byte packets.
    UsbHighSpeed,
    /// WebSocket on localhost.
    WebSocket,
    /// UDP on localhost, one `ww_framer` frame per datagram.
    Udp,
}

impl Link {
    /// What the wire units are called in a transcript.
    pub fn units(self) -> &'static str {
        match self {
            Link::UsbFullSpeed => "USB packets (full speed, 64 B max)",
            Link::UsbHighSpeed => "USB packets (high speed, 512 B max)",
            Link::WebSocket => "WebSocket frames (host frames unmasked, mask key zeroed)",
            Link::Udp => "UDP datagrams",
        }
    }
}

/// Where the host finds the device.
#[derive(Clone, Debug)]
pub enum Addr {
    InProcess(String),
    WebSocket(String),
    Udp(String),
}

/// A running simulated device, stopped when dropped.
pub struct Device {
    pub flash: Arc<Mutex<Flash>>,
    pub rec: Recorder,
    pub addr: Addr,
    boots: mpsc::UnboundedReceiver<flash::Firmware>,
    _stop: mpsc::UnboundedSender<()>,
}

impl Device {
    /// Start a device with `flash` on `link`; `name` must be unique per test (the simulated USB serial number).
    pub fn start(name: &str, link: Link, flash: Flash) -> Device {
        let flash = Arc::new(Mutex::new(flash));
        let rec = Recorder::default();
        let (boots_tx, boots) = mpsc::unbounded_channel();
        let (stop_tx, stop_rx) = mpsc::unbounded_channel();
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let (thread_flash, thread_rec, name) = (flash.clone(), rec.clone(), name.to_string());
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(device_thread(
                    name,
                    link,
                    thread_flash,
                    thread_rec,
                    boots_tx,
                    stop_rx,
                    addr_tx,
                ))
        });
        Device {
            flash,
            rec,
            addr: addr_rx.recv().unwrap(),
            boots,
            _stop: stop_tx,
        }
    }

    /// Wait until the device booted again, returns which firmware runs.
    pub async fn booted(&mut self) -> flash::Firmware {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.boots.recv())
            .await
            .expect("device boots")
            .expect("device thread runs")
    }
}

/// How a firmware run ended.
enum End {
    Reset,
    Stop,
}

/// The endpoint the device keeps across resets.
enum Media {
    Usb { path: String, packet: usize },
    WebSocket(TcpListener),
    Udp(UdpSocket),
}

async fn device_thread(
    name: String,
    link: Link,
    flash: Arc<Mutex<Flash>>,
    rec: Recorder,
    boots: mpsc::UnboundedSender<flash::Firmware>,
    mut stop: mpsc::UnboundedReceiver<()>,
    addr: std::sync::mpsc::Sender<Addr>,
) {
    let media = match link {
        Link::UsbFullSpeed | Link::UsbHighSpeed => {
            let path = format!("dfu/{name}");
            _ = addr.send(Addr::InProcess(path.clone()));
            Media::Usb {
                path,
                packet: if link == Link::UsbFullSpeed { 64 } else { 512 },
            }
        }
        Link::WebSocket => {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("ws://{}/ww", listener.local_addr().unwrap());
            _ = addr.send(Addr::WebSocket(url));
            Media::WebSocket(listener)
        }
        Link::Udp => {
            let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            _ = addr.send(Addr::Udp(socket.local_addr().unwrap().to_string()));
            Media::Udp(socket)
        }
    };
    loop {
        let fw = loop {
            // a power cut while the bootloader works: it runs again
            if let Ok(fw) = flash.lock().unwrap().boot(&rec) {
                break fw;
            }
        };
        rec.boot(fw);
        let profile = flash.lock().unwrap().profile;
        let plan_area = flash.lock().unwrap().plan_area_size;
        let role = match fw {
            flash::Firmware::Updater => Role::Bootloader,
            _ => Role::App,
        };
        let all = Offers {
            boot_control: true,
            inventory: true,
            stage: true,
            stage_bulk: true,
        };
        let node = Node::new(
            flash.clone(),
            role,
            all,
            node::limits(profile, role, plan_area),
        );
        _ = boots.send(fw);
        let end = match fw {
            flash::Firmware::App => {
                let fw = firmware::app::AppServer {
                    node,
                    plan_credit: firmware::plan_credit(),
                };
                run(fw, &media, &rec, &mut stop).await
            }
            flash::Firmware::LeanApp => {
                let mut node = node;
                node.offers = Offers {
                    boot_control: true,
                    inventory: false,
                    stage: false,
                    stage_bulk: false,
                };
                run(
                    firmware::lean_app::LeanAppServer { node },
                    &media,
                    &rec,
                    &mut stop,
                )
                .await
            }
            flash::Firmware::Updater => {
                let fw = firmware::updater::UpdaterServer {
                    node,
                    plan_credit: firmware::plan_credit(),
                };
                run(fw, &media, &rec, &mut stop).await
            }
        };
        if let End::Stop = end {
            return;
        }
    }
}

async fn run<F: Firmware>(
    fw: F,
    media: &Media,
    rec: &Recorder,
    stop: &mut mpsc::UnboundedReceiver<()>,
) -> End {
    let mut tx_buf = vec![0u8; MAX_MESSAGE + ww_device::ws::WS_OVERHEAD];
    let mut rx_buf = vec![0u8; 2 * MAX_MESSAGE];
    match media {
        Media::Usb { path, packet } => {
            let (sink, source) = in_process::usb_device(path, *packet);
            let tx = ww_device::FramedTx::new(
                TapPackets {
                    inner: sink,
                    rec: rec.clone(),
                },
                &mut tx_buf[..*packet],
            );
            let rx = ww_device::FramedRx::new(
                TapPackets {
                    inner: source,
                    rec: rec.clone(),
                },
                &mut rx_buf[..MAX_MESSAGE + packet],
            );
            serve(fw, tx, rx, rec, stop).await
        }
        Media::WebSocket(listener) => {
            let conn = ww_device::ws::WsConnection::new(WsSocket {
                listener,
                stream: None,
                rec: rec.clone(),
                rx: WsSplitter::new(Dir::ToDevice),
                tx: WsSplitter::new(Dir::ToHost),
                tx_ahead: 0,
            });
            let tx = ww_device::ws::WsTx::new(&conn, &mut tx_buf);
            let rx = ww_device::ws::WsRx::new(&conn, &mut rx_buf);
            serve(fw, tx, rx, rec, stop).await
        }
        Media::Udp(socket) => {
            use ww_device::udp::{UdpConnection, UdpSink, UdpSource};
            const DATAGRAM: usize = ww_link::UDP_MAX_DATAGRAM_LEN;
            let conn = UdpConnection::new(UdpTap {
                socket,
                rec: rec.clone(),
            });
            let tx = ww_device::FramedTx::new(UdpSink::new(&conn), &mut tx_buf[..DATAGRAM]);
            let rx = ww_device::FramedRx::new(
                UdpSource::new(&conn),
                &mut rx_buf[..MAX_MESSAGE + DATAGRAM],
            );
            serve(fw, tx, rx, rec, stop).await
        }
    }
}

async fn serve<F: Firmware>(
    fw: F,
    tx: impl ww_device::MessageTx,
    rx: impl ww_device::MessageRx,
    rec: &Recorder,
    stop: &mut mpsc::UnboundedReceiver<()>,
) -> End {
    let mut scratch = vec![0u8; MAX_MESSAGE];
    let mut event_scratch = vec![0u8; MAX_MESSAGE];
    let config = LinkConfig::new(
        fw.version(),
        fw.api_hash(),
        ww_client_server::COMPACT_VERSION,
    );
    let tx = TapTx {
        inner: tx,
        rec: rec.clone(),
    };
    let rx = TapRx {
        inner: rx,
        rec: rec.clone(),
        recorded: false,
    };
    let mut server = ww_device::Server::new(
        config,
        tx,
        rx,
        TokioClock::new(),
        &mut scratch,
        &mut event_scratch,
    );
    let mut backend = Backend(fw);
    loop {
        tokio::select! {
            ready = server.wait() => {
                _ = server.handle(ready, &mut backend).await;
            }
            _ = stop.recv() => return End::Stop,
        }
        let node = backend.0.node();
        if node.power_cut {
            rec.note(&format!(
                "power cut while the device applies step {}: no reply",
                node.cut_step.unwrap_or_default()
            ));
            return End::Reset;
        }
        if let Some(into) = node.reboot.take() {
            // the reply is out with the Disconnect, then the device resets
            server.disconnect(DisconnectReason::RequestByUser).await;
            rec.note(&format!(
                "device resets ({})",
                match into {
                    BootTarget::App => "into the app",
                    BootTarget::Updater => "into the updater",
                }
            ));
            return End::Reset;
        }
    }
}

struct Backend<F>(F);

impl<F: Firmware> WireWeaverAsyncApiBackend for Backend<F> {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        _medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        if self.0.node().power_cut {
            return Ok(&[]);
        }
        // sync server: events sent by handlers are queued and sent out after it returns
        let r = out
            .queued(|queue| self.0.process(data, scratch, queue))
            .await;
        if self.0.node().power_cut {
            // no reply: the power is gone
            return Ok(&[]);
        }
        r
    }

    fn version(&self) -> ww_device::ww_link::FullVersion<'_> {
        self.0.version()
    }
}

/// `ww_device::ws::Socket` on the device's listener, recording frames.
struct WsSocket<'l> {
    listener: &'l TcpListener,
    stream: Option<TcpStream>,
    rec: Recorder,
    rx: WsSplitter,
    tx: WsSplitter,
    /// Bytes recorded but not written yet: recorded before the write, so the record is complete once the host has a
    /// reply
    tx_ahead: usize,
}

impl ww_device::ws::Socket for WsSocket<'_> {
    type Error = std::io::Error;

    async fn accept(&mut self) {
        self.stream = None;
        let (stream, _) = self.listener.accept().await.unwrap();
        stream.set_nodelay(true).unwrap();
        self.stream = Some(stream);
        self.rx = WsSplitter::new(Dir::ToDevice);
        self.tx = WsSplitter::new(Dir::ToHost);
        self.tx_ahead = 0;
    }

    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use tokio::io::AsyncReadExt;
        let n = self.stream.as_mut().unwrap().read(buf).await?;
        self.rx.feed(&self.rec, &buf[..n]);
        Ok(n)
    }

    async fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        use tokio::io::AsyncWriteExt;
        // a partial write is called again with the rest, which is recorded already
        self.tx
            .feed(&self.rec, &bytes[self.tx_ahead.min(bytes.len())..]);
        let n = self.stream.as_mut().unwrap().write(bytes).await?;
        self.tx_ahead = bytes.len() - n;
        Ok(n)
    }
}

/// `ww_device::udp::DatagramSocket` on the device's socket, recording datagrams.
struct UdpTap<'s> {
    socket: &'s UdpSocket,
    rec: Recorder,
}

impl ww_device::udp::DatagramSocket for UdpTap<'_> {
    type Addr = std::net::SocketAddr;
    type RecvError = std::io::Error;
    type SendError = std::io::Error;

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, Self::Addr)> {
        let (n, from) = self.socket.recv_from(buf).await?;
        self.rec.packet(Dir::ToDevice, &buf[..n]);
        Ok((n, from))
    }

    async fn send_to(&self, datagram: &[u8], addr: Self::Addr) -> std::io::Result<()> {
        self.rec.packet(Dir::ToHost, datagram);
        self.socket.send_to(datagram, addr).await.map(|_| ())
    }
}
