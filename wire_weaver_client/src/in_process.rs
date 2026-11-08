//! In-process transport: a device running in the same process as the host (simulators, tests), connected through
//! message channels instead of a real medium. Everything above the medium is the same as with USB or RTT: the host
//! event loop with link setup, version check, timeouts and streams on one side, [ww_device::Server] with a generated
//! server on the other.
//!
//! The device registers a path with [device] and runs a [ww_device::Server] with the returned halves, the host
//! connects with [ClientConfig::in_process_path](crate::ClientConfig::in_process_path):
//! ```ignore
//! let (tx, rx) = wire_weaver_client::in_process::device("sim/blinky", 1024);
//! std::thread::spawn(move || {
//!     // ww_device futures are not Send, so the device runs on its own thread or in a LocalSet
//!     tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
//!         let mut scratch = [0u8; 1024];
//!         let mut server = ww_device::Server::new(link_config, tx, rx, TokioClock::new(), &mut scratch);
//!         server.run(&mut backend).await
//!     })
//! });
//! let blinky = Blinky::config(|c| c.in_process_path("sim/blinky".into())).connect().await?;
//! ```
//!
//! Messages are moved as is, without a framer. A device accepts one host at a time: a new connection replaces the
//! previous one, as if a USB cable was moved to another host.
//!
//! # Simulated USB
//!
//! A device registered with [usb_device] instead speaks USB packets: the host packs messages into packets of at most
//! the given size with the USB framer configuration of [ww_link], exactly as the USB transport does over nusb, and
//! the device runs [ww_device::FramedTx] / [ww_device::FramedRx] over the returned [UsbSink] / [UsbSource], as it
//! would over its USB endpoints. Everything on the wire is then what a real full-speed (64 B) or high-speed (512 B)
//! bulk endpoint pair carries; only USB itself (enumeration, data toggles, NAKs) is not simulated. The host connects
//! with the same [ClientConfig::in_process_path](crate::ClientConfig::in_process_path), the path standing in for the
//! USB serial number. [usb_device_framed] picks the framing, e.g. [ww_link::UsbSeqFraming] with its frame counter,
//! as a real device's interface protocol does.
//!
//! # Simulated UDP
//!
//! A device registered with [udp_device] speaks UDP datagrams: the host packs messages into datagrams with the UDP
//! framer configuration of [ww_link] (frame counter, CRC-32 on every message), exactly as the UDP transport does over
//! a socket, and the device serves the returned [UdpPort] with `ww_device::udp` ([UdpConnection](ww_device::udp::UdpConnection),
//! [UdpSink](ww_device::udp::UdpSink), [UdpSource](ww_device::udp::UdpSource)), as it would a real socket. Each
//! connection is a new host address, so a reconnect is a peer takeover as over UDP. Only the kernel is left out: a
//! datagram is in the device's queue as soon as it is sent, so tokio's paused clock (`start_paused`) drives the
//! accumulation windows and link setup retries of both ends exactly, which it can't over a real socket.
//!
//! # Simulated WebSocket
//!
//! With the `ws` feature, a device registered with [ws_device] speaks WebSocket over an in-memory pipe: the host runs
//! the `ws` transport's own client (handshake, one binary message per link message) and the device serves the returned
//! [WsPort] with `ww_device::ws`, as it would a TCP socket. As with simulated UDP, nothing waits in a kernel, so the
//! paused clock decides the order in which both ends' frames go out.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use anyhow::bail;
use tokio::sync::mpsc;

use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::transport::{MessageRx, MessageTx, Opened, Selected, Transport};
use ww_link::{UdpFraming, UsbFraming, UsbFramingProtocol};
use ww_tap::Tap;

type Message = (u8, Vec<u8>);

/// Host side ends of a new connection, sent to a device
struct Connection {
    to_device: mpsc::UnboundedReceiver<Message>,
    to_host: mpsc::UnboundedSender<Message>,
}

/// A registered device
struct Registered {
    /// Unused by a [udp_device], whose hosts send datagrams to its [UdpPort] instead; tells devices apart
    connections: mpsc::UnboundedSender<Connection>,
    medium: Medium,
}

/// What a registered device speaks
#[derive(Clone)]
enum Medium {
    /// [device]: messages as they are
    Messages,
    /// [usb_device]: messages are framed into packets of at most this size, each moved as one [Message] with kind 0
    Usb(UsbPacket),
    /// [udp_device]: messages are framed into datagrams, each moved as one [Message] with kind 0
    Udp(UdpHosts),
    /// [ws_device]: each connection is a new in-memory pipe, the device's end goes here
    #[cfg(feature = "ws")]
    Ws(mpsc::UnboundedSender<tokio::io::DuplexStream>),
}

/// Where the hosts of a [udp_device] send datagrams, and where the device's replies to each go
#[derive(Clone)]
struct UdpHosts {
    to_device: mpsc::UnboundedSender<(UdpAddr, Vec<u8>)>,
    to_host: Arc<Mutex<HashMap<UdpAddr, mpsc::UnboundedSender<Message>>>>,
}

/// Packets of a simulated USB device
#[derive(Copy, Clone)]
struct UsbPacket {
    max_size: usize,
    /// The device uses [ww_link::UsbSeqFraming]
    seq_prefix: bool,
}

static DEVICES: LazyLock<Mutex<HashMap<String, Registered>>> = LazyLock::new(Default::default);

/// Register a device under `path` and return its message halves for [ww_device::Server]. A device already registered
/// under the same path is replaced, its connected host (if any) stays connected to it until either side drops.
///
/// `max_message_len` is reported to the host as the longest message the device can receive.
pub fn device(path: &str, max_message_len: usize) -> (DeviceTx, DeviceRx) {
    let (connections_tx, connections) = register(path, Medium::Messages);
    let to_host = Arc::new(Mutex::new(None));
    (
        DeviceTx {
            to_host: to_host.clone(),
        },
        DeviceRx {
            path: path.to_string(),
            connections_tx,
            connections,
            to_device: None,
            to_host,
            message: None,
            max_message_len,
        },
    )
}

fn register(
    path: &str,
    medium: Medium,
) -> (
    mpsc::UnboundedSender<Connection>,
    mpsc::UnboundedReceiver<Connection>,
) {
    let (connections_tx, connections) = mpsc::unbounded_channel();
    DEVICES.lock().unwrap().insert(
        path.to_string(),
        Registered {
            connections: connections_tx.clone(),
            medium,
        },
    );
    (connections_tx, connections)
}

/// Unregister `path` if it is still registered to the device that owns `connections_tx`
fn unregister(path: &str, connections_tx: &mpsc::UnboundedSender<Connection>) {
    let mut devices = DEVICES.lock().unwrap();
    if devices
        .get(path)
        .is_some_and(|r| r.connections.same_channel(connections_tx))
    {
        devices.remove(path);
    }
}

/// Paths of all registered devices.
pub fn devices() -> Vec<String> {
    DEVICES.lock().unwrap().keys().cloned().collect()
}

/// Error of [DeviceTx] and [DeviceRx]: the host disconnected or is not connected yet.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Disconnected;

/// Device tx half, see [device].
pub struct DeviceTx {
    to_host: Arc<Mutex<Option<mpsc::UnboundedSender<Message>>>>,
}

impl ww_device::MessageTx for DeviceTx {
    type Error = Disconnected;

    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), Disconnected> {
        let to_host = self.to_host.lock().unwrap();
        let to_host = to_host.as_ref().ok_or(Disconnected)?;
        to_host
            .send((kind, message.to_vec()))
            .map_err(|_| Disconnected)
    }

    async fn flush(&mut self) -> Result<(), Disconnected> {
        Ok(())
    }

    fn reset(&mut self) {}
}

/// Device rx half, see [device]. Dropping it unregisters the device.
pub struct DeviceRx {
    path: String,
    /// To tell whether the registry entry is still this device's on drop
    connections_tx: mpsc::UnboundedSender<Connection>,
    connections: mpsc::UnboundedReceiver<Connection>,
    to_device: Option<mpsc::UnboundedReceiver<Message>>,
    to_host: Arc<Mutex<Option<mpsc::UnboundedSender<Message>>>>,
    message: Option<Message>,
    max_message_len: usize,
}

impl ww_device::MessageRx for DeviceRx {
    type Error = Disconnected;

    async fn wait_message(&mut self) -> Result<(), Disconnected> {
        if self.message.is_some() {
            return Ok(());
        }
        let to_device = self.to_device.as_mut().ok_or(Disconnected)?;
        // cancel-safe: tokio's recv() does not lose messages when dropped
        match to_device.recv().await {
            Some(message) => {
                self.message = Some(message);
                Ok(())
            }
            None => {
                self.to_device = None;
                *self.to_host.lock().unwrap() = None;
                Err(Disconnected)
            }
        }
    }

    fn message(&self) -> Option<(u8, &[u8])> {
        self.message
            .as_ref()
            .map(|(kind, bytes)| (*kind, bytes.as_slice()))
    }

    fn consume(&mut self) {
        self.message = None;
    }

    async fn wait_connected(&mut self) {
        match self.connections.recv().await {
            Some(c) => {
                self.to_device = Some(c.to_device);
                *self.to_host.lock().unwrap() = Some(c.to_host);
                self.message = None;
            }
            // cannot happen, connections_tx is held by self
            None => core::future::pending().await,
        }
    }

    fn reset(&mut self) {
        self.message = None;
    }

    fn max_message_len(&self) -> usize {
        self.max_message_len
    }
}

impl Drop for DeviceRx {
    fn drop(&mut self) {
        unregister(&self.path, &self.connections_tx);
    }
}

/// Register a device under `path` that speaks USB packets of at most `max_packet_size` bytes (64 for full speed,
/// 512 for high speed bulk endpoints), see [Simulated USB](self#simulated-usb). The device wraps the returned halves
/// in [ww_device::FramedTx] / [ww_device::FramedRx], as it does its USB endpoints. A device already registered under
/// the same path is replaced.
pub fn usb_device(path: &str, max_packet_size: usize) -> (UsbSink, UsbSource) {
    usb_device_framed::<UsbFraming>(path, max_packet_size)
}

/// Same as [usb_device], with the framing `F` the device uses ([UsbFraming] or [ww_link::UsbSeqFraming]); the host
/// learns it as from a real device's interface protocol.
pub fn usb_device_framed<F: UsbFramingProtocol>(
    path: &str,
    max_packet_size: usize,
) -> (UsbSink<F>, UsbSource<F>) {
    let packet = UsbPacket {
        max_size: max_packet_size,
        seq_prefix: F::INTERFACE_PROTOCOL == ww_link::USB_INTERFACE_PROTOCOL_SEQ,
    };
    let (connections_tx, connections) = register(path, Medium::Usb(packet));
    let to_host = Arc::new(Mutex::new(None));
    (
        UsbSink {
            to_host: to_host.clone(),
            max_packet_size,
            _framing: PhantomData,
        },
        UsbSource {
            path: path.to_string(),
            connections_tx,
            connections,
            to_device: None,
            to_host,
            max_packet_size,
            _framing: PhantomData,
        },
    )
}

/// Device IN endpoint of a simulated USB device, see [usb_device].
pub struct UsbSink<F = UsbFraming> {
    to_host: Arc<Mutex<Option<mpsc::UnboundedSender<Message>>>>,
    max_packet_size: usize,
    _framing: PhantomData<fn() -> F>,
}

impl<F: UsbFramingProtocol> ww_device::PacketSink for UsbSink<F> {
    type Error = Disconnected;
    type Framing = F;

    async fn write_packet(&mut self, packet: &[u8]) -> Result<(), Disconnected> {
        assert!(
            packet.len() <= self.max_packet_size,
            "packet longer than the endpoint's max packet size"
        );
        let to_host = self.to_host.lock().unwrap();
        let to_host = to_host.as_ref().ok_or(Disconnected)?;
        to_host.send((0, packet.to_vec())).map_err(|_| Disconnected)
    }
}

/// Device OUT endpoint of a simulated USB device, see [usb_device]. Dropping it unregisters the device.
pub struct UsbSource<F = UsbFraming> {
    path: String,
    connections_tx: mpsc::UnboundedSender<Connection>,
    connections: mpsc::UnboundedReceiver<Connection>,
    to_device: Option<mpsc::UnboundedReceiver<Message>>,
    to_host: Arc<Mutex<Option<mpsc::UnboundedSender<Message>>>>,
    max_packet_size: usize,
    _framing: PhantomData<fn() -> F>,
}

impl<F: UsbFramingProtocol> ww_device::PacketSource for UsbSource<F> {
    type Error = Disconnected;
    type Framing = F;

    fn max_packet_len(&self) -> usize {
        self.max_packet_size
    }

    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, Disconnected> {
        let to_device = self.to_device.as_mut().ok_or(Disconnected)?;
        // cancel-safe: tokio's recv() does not lose packets when dropped
        match to_device.recv().await {
            Some((_, packet)) => {
                buf[..packet.len()].copy_from_slice(&packet);
                Ok(packet.len())
            }
            None => {
                self.to_device = None;
                *self.to_host.lock().unwrap() = None;
                Err(Disconnected)
            }
        }
    }

    async fn wait_connected(&mut self) {
        match self.connections.recv().await {
            Some(c) => {
                self.to_device = Some(c.to_device);
                *self.to_host.lock().unwrap() = Some(c.to_host);
            }
            // cannot happen, connections_tx is held by self
            None => core::future::pending().await,
        }
    }
}

impl<F> Drop for UsbSource<F> {
    fn drop(&mut self) {
        unregister(&self.path, &self.connections_tx);
    }
}

/// Register a device under `path` that speaks UDP datagrams, see [Simulated UDP](self#simulated-udp). The device
/// serves the returned socket with `ww_device::udp`, as it does a real one. A device already registered under the
/// same path is replaced.
pub fn udp_device(path: &str) -> UdpPort {
    let (to_device, datagrams) = mpsc::unbounded_channel();
    let hosts = UdpHosts {
        to_device,
        to_host: Default::default(),
    };
    let (connections_tx, _) = register(path, Medium::Udp(hosts.clone()));
    UdpPort {
        path: path.to_string(),
        connections_tx,
        datagrams: tokio::sync::Mutex::new(datagrams),
        to_host: hosts.to_host,
    }
}

/// Address of a host connected to a [udp_device], a new one for every connection (as a new socket's port is).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct UdpAddr(u32);

/// The socket of a simulated UDP device, see [udp_device]. Dropping it unregisters the device.
pub struct UdpPort {
    path: String,
    /// To tell whether the registry entry is still this device's on drop
    connections_tx: mpsc::UnboundedSender<Connection>,
    /// Only `recv_from` locks it, which `ww_device::udp` calls from one place
    datagrams: tokio::sync::Mutex<mpsc::UnboundedReceiver<(UdpAddr, Vec<u8>)>>,
    to_host: Arc<Mutex<HashMap<UdpAddr, mpsc::UnboundedSender<Message>>>>,
}

impl ww_device::udp::DatagramSocket for UdpPort {
    type Addr = UdpAddr;
    type RecvError = Disconnected;
    type SendError = Disconnected;

    /// Cancel-safe: tokio's recv() does not lose datagrams when dropped. A datagram longer than `buf` is truncated.
    async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, UdpAddr), Disconnected> {
        let mut datagrams = self.datagrams.lock().await;
        // cannot end: the registry holds a sender while the device is registered, and only drop unregisters it
        let (from, datagram) = datagrams.recv().await.ok_or(Disconnected)?;
        let len = datagram.len().min(buf.len());
        buf[..len].copy_from_slice(&datagram[..len]);
        Ok((len, from))
    }

    /// A datagram to a host that is gone (or never was) is lost, as over UDP.
    async fn send_to(&self, datagram: &[u8], addr: UdpAddr) -> Result<(), Disconnected> {
        let mut hosts = self.to_host.lock().unwrap();
        if let Some(host) = hosts.get(&addr)
            && host.send((0, datagram.to_vec())).is_err()
        {
            hosts.remove(&addr);
        }
        Ok(())
    }
}

impl Drop for UdpPort {
    fn drop(&mut self) {
        unregister(&self.path, &self.connections_tx);
    }
}

/// Register a device under `path` that speaks WebSocket over an in-memory pipe, see
/// [Simulated WebSocket](self#simulated-websocket). The device serves the returned socket with `ww_device::ws`, as it
/// does a TCP socket. A device already registered under the same path is replaced.
#[cfg(feature = "ws")]
pub fn ws_device(path: &str) -> WsPort {
    let (pipes_tx, pipes) = mpsc::unbounded_channel();
    let (connections_tx, _) = register(path, Medium::Ws(pipes_tx));
    WsPort {
        path: path.to_string(),
        connections_tx,
        pipes,
        pipe: None,
    }
}

/// Bytes each way an in-memory WebSocket pipe holds before a write waits for the reader, as a TCP socket's buffers.
#[cfg(feature = "ws")]
const WS_PIPE_LEN: usize = 64 * 1024;

/// The listening socket of a simulated WebSocket device, see [ws_device]. Dropping it unregisters the device.
#[cfg(feature = "ws")]
pub struct WsPort {
    path: String,
    /// To tell whether the registry entry is still this device's on drop
    connections_tx: mpsc::UnboundedSender<Connection>,
    pipes: mpsc::UnboundedReceiver<tokio::io::DuplexStream>,
    /// The connected host's pipe
    pipe: Option<tokio::io::DuplexStream>,
}

#[cfg(feature = "ws")]
impl ww_device::ws::Socket for WsPort {
    type Error = std::io::Error;

    /// Cancel-safe: tokio's recv() does not lose a pipe when dropped.
    async fn accept(&mut self) {
        self.pipe = None;
        match self.pipes.recv().await {
            Some(pipe) => self.pipe = Some(pipe),
            // cannot happen, the registry holds the sender while the device is registered
            None => core::future::pending().await,
        }
    }

    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use tokio::io::AsyncReadExt;
        match self.pipe.as_mut() {
            Some(pipe) => pipe.read(buf).await,
            None => Ok(0),
        }
    }

    async fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        use tokio::io::AsyncWriteExt;
        match self.pipe.as_mut() {
            Some(pipe) => pipe.write(bytes).await,
            None => Err(std::io::ErrorKind::NotConnected.into()),
        }
    }
}

#[cfg(feature = "ws")]
impl Drop for WsPort {
    fn drop(&mut self) {
        unregister(&self.path, &self.connections_tx);
    }
}

/// [ww_device::Clock] on top of tokio's timer, for devices running on the host.
#[derive(Copy, Clone)]
pub struct TokioClock(tokio::time::Instant);

impl TokioClock {
    pub fn new() -> Self {
        TokioClock(tokio::time::Instant::now())
    }
}

impl Default for TokioClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ww_device::Clock for TokioClock {
    fn now(&self) -> ww_device::Instant {
        ww_device::Instant::from_micros(self.0.elapsed().as_micros() as u64)
    }

    async fn wait_until(&self, at: ww_device::Instant) {
        tokio::time::sleep_until(self.0 + Duration::from_micros(at.as_micros())).await
    }
}

// Host side

/// The in-process transport bound to `path`, for event loop tests that take a transport.
#[cfg(test)]
pub(crate) fn test_transport(path: &str) -> impl Transport + 'static {
    struct Bound(String);
    impl Transport for Bound {
        type Tx = HostTx;
        type Rx = HostRx;
        fn link_setup_timeout(&self) -> Duration {
            crate::event_loop::transport::link_setup_timeout::IN_PROCESS
        }
        async fn connect(
            &mut self,
            _: DeviceHandle,
            tap: ww_tap::RecorderLink,
            max_message_len: usize,
        ) -> Result<Opened<HostTx, HostRx>, String> {
            InProcessTransport
                .connect(Box::new(self.0.clone()), tap, max_message_len)
                .await
        }
    }
    Bound(path.to_string())
}

pub(crate) fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let Some(path) = c.pieces.iter().rev().find_map(|p| match p {
        ConfigPiece::InProcessPath { path } => Some(path.clone()),
        _ => None,
    }) else {
        bail!("Internal error: in-process selected without a path");
    };
    if !DEVICES.lock().unwrap().contains_key(&path) {
        let unmatched = devices().into_iter().map(device_info).collect();
        return Ok(Selected::NotFound { unmatched });
    }
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    tokio::spawn(crate::event_loop::core::worker(cmd_rx, InProcessTransport));
    Ok(Selected::Device {
        info: Box::new(device_info(path.clone())),
        handle: Box::new(path),
        via_hub: false,
    })
}

fn device_info(path: String) -> crate::DeviceInfo {
    crate::DeviceInfo {
        location: format!("in_process {path}"),
        manufacturer: String::new(),
        product: String::new(),
        serials: vec![],
        user_label: String::new(),
        api: None,
        usb: None,
    }
}

pub(crate) struct InProcessTransport;

impl Transport for InProcessTransport {
    fn link_setup_timeout(&self) -> std::time::Duration {
        crate::event_loop::transport::link_setup_timeout::IN_PROCESS
    }

    type Tx = HostTx;
    type Rx = HostRx;

    async fn connect(
        &mut self,
        handle: DeviceHandle,
        tap: ww_tap::RecorderLink,
        max_message_len: usize,
    ) -> Result<Opened<HostTx, HostRx>, String> {
        let path = handle
            .downcast::<String>()
            .map_err(|_| "expected in-process device path".to_string())?;
        let (device, medium) = DEVICES
            .lock()
            .unwrap()
            .get(path.as_str())
            .map(|r| (r.connections.clone(), r.medium.clone()))
            .ok_or_else(|| format!("in-process device '{path}' is gone"))?;
        #[cfg(feature = "ws")]
        if let Medium::Ws(pipes) = &medium {
            let (host, device) = tokio::io::duplex(WS_PIPE_LEN);
            pipes
                .send(device)
                .map_err(|_| format!("in-process device '{path}' is gone"))?;
            let url = format!("ws://in-process/{path}");
            let opened = crate::ws::handshake(&url, host, max_message_len).await?;
            let mut tap = tap;
            tap.session(&crate::tap::host_session(
                ww_tap::Medium::InProcessWebSocket,
                ww_tap::Framer::None,
                0,
                &path,
            ));
            return Ok(Opened {
                tx: HostTx::Ws(opened.tx),
                rx: HostRx::Ws(opened.rx),
            });
        }
        let (to_host, to_host_rx) = mpsc::unbounded_channel();
        let to_device = match &medium {
            Medium::Udp(hosts) => {
                static NEXT_ADDR: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(1);
                let addr = UdpAddr(NEXT_ADDR.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
                hosts.to_host.lock().unwrap().insert(addr, to_host);
                ToDevice::Udp(hosts.to_device.clone(), addr)
            }
            #[cfg(feature = "ws")]
            Medium::Ws(_) => unreachable!("connected above"),
            Medium::Messages | Medium::Usb(_) => {
                let (to_device_tx, to_device) = mpsc::unbounded_channel();
                device
                    .send(Connection { to_device, to_host })
                    .map_err(|_| format!("in-process device '{path}' is gone"))?;
                ToDevice::Connection(to_device_tx)
            }
        };
        let mut tap = tap;
        tap.session(&match &medium {
            Medium::Usb(p) => crate::tap::host_session(
                ww_tap::Medium::InProcessUsb,
                crate::framing::usb_tap_framer(p.seq_prefix),
                p.max_size,
                &path,
            ),
            Medium::Udp(_) => crate::tap::host_session(
                ww_tap::Medium::InProcessUdp,
                crate::tap::FRAMED_UDP,
                ww_link::UDP_MAX_DATAGRAM_LEN,
                &path,
            ),
            Medium::Messages => {
                crate::tap::host_session(ww_tap::Medium::InProcess, ww_tap::Framer::None, 0, &path)
            }
            #[cfg(feature = "ws")]
            Medium::Ws(_) => unreachable!("connected above"),
        });
        let (tx_framer, rx_framer) = match &medium {
            Medium::Messages => (None, None),
            #[cfg(feature = "ws")]
            Medium::Ws(_) => unreachable!("connected above"),
            Medium::Usb(p) => (
                Some(TxFramer::Usb(UsbTxFramer::new(p.seq_prefix, p.max_size))),
                Some(RxFramer::Usb(UsbRxFramer::new(
                    p.seq_prefix,
                    max_message_len + p.max_size,
                ))),
            ),
            // datagrams from the device are taken whatever their size, as over a socket
            Medium::Udp(_) => (
                Some(TxFramer::Udp(HelloTx::<UdpFraming>::new(
                    ww_link::UDP_MAX_DATAGRAM_LEN,
                ))),
                Some(RxFramer::Udp(HelloRx::<UdpFraming>::new(
                    max_message_len + MAX_RX_DATAGRAM_LEN,
                ))),
            ),
        };
        Ok(Opened {
            tx: HostTx::Channel(ChannelTx {
                to_device,
                framer: tx_framer,
                tap: tap.clone(),
            }),
            rx: HostRx::Channel(ChannelRx {
                from_device: to_host_rx,
                message: None,
                framer: rx_framer,
                tap,
            }),
        })
    }
}

/// Largest UDP payload, the longest datagram a simulated UDP host takes
const MAX_RX_DATAGRAM_LEN: usize = 65_535;

use crate::framing::{HelloRx, HelloTx, UsbRx as UsbRxFramer, UsbTx as UsbTxFramer};

/// Host tx half of an in-process connection
pub(crate) enum HostTx {
    Channel(ChannelTx),
    #[cfg(feature = "ws")]
    Ws(crate::ws::WsTx<tokio::io::DuplexStream>),
}

impl MessageTx for HostTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        match self {
            HostTx::Channel(tx) => tx.write_message(kind, message).await,
            #[cfg(feature = "ws")]
            HostTx::Ws(tx) => tx.write_message(kind, message).await,
        }
    }

    async fn flush(&mut self) -> Result<(), String> {
        match self {
            HostTx::Channel(tx) => tx.flush().await,
            #[cfg(feature = "ws")]
            HostTx::Ws(tx) => tx.flush().await,
        }
    }

    async fn close(self) {
        match self {
            HostTx::Channel(tx) => tx.close().await,
            #[cfg(feature = "ws")]
            HostTx::Ws(tx) => tx.close().await,
        }
    }
}

/// Host rx half of an in-process connection
pub(crate) enum HostRx {
    Channel(ChannelRx),
    #[cfg(feature = "ws")]
    Ws(crate::ws::WsRx<tokio::io::DuplexStream>),
}

impl MessageRx for HostRx {
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        match self {
            HostRx::Channel(rx) => rx.read_message().await,
            #[cfg(feature = "ws")]
            HostRx::Ws(rx) => rx.read_message().await,
        }
    }

    async fn close(self) {
        match self {
            HostRx::Channel(rx) => rx.close().await,
            #[cfg(feature = "ws")]
            HostRx::Ws(rx) => rx.close().await,
        }
    }
}

pub(crate) struct ChannelTx {
    to_device: ToDevice,
    /// Some for a simulated USB or UDP device: messages are packed into packets or datagrams
    framer: Option<TxFramer>,
    tap: ww_tap::RecorderLink,
}

/// Where a host sends to
enum ToDevice {
    /// The device's end of this connection
    Connection(mpsc::UnboundedSender<Message>),
    /// The socket of a simulated UDP device, from this host's address
    Udp(mpsc::UnboundedSender<(UdpAddr, Vec<u8>)>, UdpAddr),
}

impl ToDevice {
    fn send(&self, message: Message) -> Result<(), String> {
        let sent = match self {
            ToDevice::Connection(tx) => tx.send(message).is_ok(),
            ToDevice::Udp(tx, from) => tx.send((*from, message.1)).is_ok(),
        };
        sent.then_some(())
            .ok_or_else(|| "in-process device is gone".to_string())
    }
}

/// Host tx framer of a simulated medium
enum TxFramer {
    Usb(UsbTxFramer),
    Udp(HelloTx<UdpFraming>),
}

impl TxFramer {
    fn write(&mut self, kind: u8, message: &[u8]) -> Result<bool, ()> {
        match self {
            TxFramer::Usb(f) => f.write(kind, message),
            TxFramer::Udp(f) => f.write(kind, message),
        }
    }

    fn flush_to_vec(&mut self) -> Option<Vec<u8>> {
        match self {
            TxFramer::Usb(f) => f.flush_to_vec(),
            TxFramer::Udp(f) => f.flush_to_vec(),
        }
    }
}

/// Host rx framer of a simulated medium
enum RxFramer {
    Usb(UsbRxFramer),
    Udp(HelloRx<UdpFraming>),
}

impl RxFramer {
    fn stage(&mut self, frame: &[u8]) -> Result<ww_framer::prefix::FrameReport, ()> {
        match self {
            RxFramer::Usb(f) => f.stage(frame),
            RxFramer::Udp(f) => f.stage(frame),
        }
    }

    fn reassemble(&mut self) {
        match self {
            RxFramer::Usb(f) => f.reassemble(),
            RxFramer::Udp(f) => f.reassemble(),
        }
    }

    fn message(&self) -> Option<(u8, &[u8])> {
        match self {
            RxFramer::Usb(f) => f.message(),
            RxFramer::Udp(f) => f.message(),
        }
    }
}

/// Send a simulated USB packet or UDP datagram, recording it first.
fn send_packet(
    to_device: &ToDevice,
    tap: &mut ww_tap::RecorderLink,
    packet: Vec<u8>,
) -> Result<(), String> {
    if tap.wants_frames() {
        tap.frame(ww_tap::Dir::Tx, &packet);
    }
    to_device.send((0, packet))
}

impl MessageTx for ChannelTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        let Some(framer) = self.framer.as_mut() else {
            return self.to_device.send((kind, message.to_vec()));
        };
        loop {
            match framer.write(kind, message) {
                Ok(true) => return Ok(()),
                // frame is full (or the message continues into the next one): send and retry
                Ok(false) => match framer.flush_to_vec() {
                    Some(packet) => send_packet(&self.to_device, &mut self.tap, packet)?,
                    None => return Err("framing error: cannot progress".into()),
                },
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
        if let Some(packet) = self.framer.as_mut().and_then(|f| f.flush_to_vec()) {
            send_packet(&self.to_device, &mut self.tap, packet)?;
        }
        Ok(())
    }
}

pub(crate) struct ChannelRx {
    from_device: mpsc::UnboundedReceiver<Message>,
    /// Last received message, returned borrowed
    message: Option<Message>,
    /// Some for a simulated USB or UDP device: packets or datagrams are reassembled into messages
    framer: Option<RxFramer>,
    tap: ww_tap::RecorderLink,
}

impl MessageRx for ChannelRx {
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        if self.framer.is_none() {
            // cancel-safe: nothing is lost if recv() is dropped, the previous message is only replaced after it
            // returned
            let message = self
                .from_device
                .recv()
                .await
                .ok_or_else(|| "in-process device is gone".to_string())?;
            let (kind, bytes) = self.message.insert(message);
            return Ok((*kind, bytes.as_slice()));
        }
        // cancel-safe: staged packets live in the framer, the only await is the packet read
        loop {
            let framer = self.framer.as_mut().expect("checked above");
            framer.reassemble();
            if framer.message().is_some() {
                break;
            }
            let (_, packet) = self
                .from_device
                .recv()
                .await
                .ok_or_else(|| "in-process device is gone".to_string())?;
            if self.tap.wants_frames() {
                self.tap.frame(ww_tap::Dir::Rx, &packet);
            }
            let report = framer
                .stage(&packet)
                .map_err(|_| "out of staging area, this is a bug".to_string())?;
            crate::framing::log_report(report);
        }
        Ok(self
            .framer
            .as_ref()
            .and_then(|f| f.message())
            .expect("checked above"))
    }
}

#[cfg(test)]
mod tests {
    //! Host event loop against [ww_device::Server] over simulated USB: real USB framing on both ends, 64 B packets.
    use super::*;
    use crate::event_loop::device_e2e_tests::{DEV_MAX_MESSAGE, serve, talk};

    /// Tests send `()` as a handle
    struct To(String);

    impl Transport for To {
        fn link_setup_timeout(&self) -> std::time::Duration {
            crate::event_loop::transport::link_setup_timeout::IN_PROCESS
        }

        type Tx = HostTx;
        type Rx = HostRx;
        async fn connect(
            &mut self,
            _: DeviceHandle,
            tap: ww_tap::RecorderLink,
            max_message_len: usize,
        ) -> Result<Opened<HostTx, HostRx>, String> {
            InProcessTransport
                .connect(Box::new(self.0.clone()), tap, max_message_len)
                .await
        }
    }

    #[tokio::test]
    async fn host_and_device_talk_over_simulated_usb() {
        talk_over_simulated_usb::<UsbFraming>("in_process/usb_talk").await
    }

    /// The device uses the frame counter diagnostic: the host learns it as from the interface protocol.
    #[tokio::test]
    async fn host_and_device_talk_over_simulated_usb_with_frame_counter() {
        talk_over_simulated_usb::<ww_link::UsbSeqFraming>("in_process/usb_seq_talk").await
    }

    /// The device serves `ww_device::udp` over the simulated socket: UDP framing on both ends, peer adopted at link
    /// setup. On the paused clock, which a real socket can't run on.
    #[tokio::test(start_paused = true)]
    async fn host_and_device_talk_over_simulated_udp() {
        tokio::task::LocalSet::new()
            .run_until(talk_over_simulated_udp())
            .await
    }

    async fn talk_over_simulated_udp() {
        const PATH: &str = "in_process/udp_talk";
        const DATAGRAM: usize = ww_link::UDP_MAX_DATAGRAM_LEN;
        let port = udp_device(PATH);
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        // the medium is not Send (a Cell for the peer, as on embassy), run it on the test's LocalSet
        let dev = tokio::task::spawn_local(async move {
            use ww_device::udp::{UdpConnection, UdpSink, UdpSource};
            let conn = UdpConnection::new(port);
            let mut tx_frame = [0u8; DATAGRAM];
            let mut rx = ww_device::RxBuffer::<DATAGRAM, DEV_MAX_MESSAGE>::new();
            serve(
                ww_device::FramedTx::new(UdpSink::new(&conn), &mut tx_frame),
                ww_device::FramedRx::new(UdpSource::new(&conn), rx.assembly_buf(DATAGRAM)),
                events_tx,
            )
            .await
        });
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let host = tokio::spawn(crate::event_loop::core::worker(
            cmd_rx,
            To(PATH.to_string()),
        ));
        talk((cmd_tx, events_rx, dev, host)).await
    }

    /// The device serves `ww_device::ws` over the in-memory pipe: the host's own WebSocket client on one end, on the
    /// paused clock.
    #[cfg(feature = "ws")]
    #[tokio::test(start_paused = true)]
    async fn host_and_device_talk_over_simulated_ws() {
        tokio::task::LocalSet::new()
            .run_until(talk_over_simulated_ws())
            .await
    }

    #[cfg(feature = "ws")]
    async fn talk_over_simulated_ws() {
        const PATH: &str = "in_process/ws_talk";
        let port = ws_device(PATH);
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        // the medium is not Send (NoopRawMutex, as on embassy), run it on the test's LocalSet
        let dev = tokio::task::spawn_local(async move {
            use ww_device::ws::{WS_OVERHEAD, WsConnection, WsRx, WsTx};
            let conn = WsConnection::new(port);
            let mut tx_buf = [0u8; DEV_MAX_MESSAGE + WS_OVERHEAD];
            let mut rx_buf = [0u8; DEV_MAX_MESSAGE + WS_OVERHEAD];
            serve(
                WsTx::new(&conn, &mut tx_buf),
                WsRx::new(&conn, &mut rx_buf),
                events_tx,
            )
            .await
        });
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let host = tokio::spawn(crate::event_loop::core::worker(
            cmd_rx,
            To(PATH.to_string()),
        ));
        talk((cmd_tx, events_rx, dev, host)).await
    }

    async fn talk_over_simulated_usb<F: UsbFramingProtocol + 'static>(path: &str) {
        const PACKET: usize = 64;
        let (sink, source) = usb_device_framed::<F>(path, PACKET);
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let dev = tokio::spawn(async move {
            let mut tx_frame = [0u8; PACKET];
            let mut rx = ww_device::RxBuffer::<PACKET, DEV_MAX_MESSAGE>::new();
            serve(
                ww_device::FramedTx::new(sink, &mut tx_frame),
                ww_device::FramedRx::new(source, rx.assembly_buf(PACKET)),
                events_tx,
            )
            .await
        });
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let host = tokio::spawn(crate::event_loop::core::worker(
            cmd_rx,
            To(path.to_string()),
        ));
        talk((cmd_tx, events_rx, dev, host)).await
    }
}
