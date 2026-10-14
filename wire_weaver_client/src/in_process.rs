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
//! USB serial number.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use anyhow::bail;
use tokio::sync::mpsc;

use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::transport::{MessageRx, MessageTx, Opened, Selected, Transport};

type Message = (u8, Vec<u8>);

/// Host side ends of a new connection, sent to a device
struct Connection {
    to_device: mpsc::UnboundedReceiver<Message>,
    to_host: mpsc::UnboundedSender<Message>,
}

/// A registered device
struct Registered {
    connections: mpsc::UnboundedSender<Connection>,
    /// Some for [usb_device]: messages are framed into packets of at most this size, each moved as one
    /// [Message] with kind 0
    usb_packet: Option<usize>,
}

static DEVICES: LazyLock<Mutex<HashMap<String, Registered>>> = LazyLock::new(Default::default);

/// Register a device under `path` and return its message halves for [ww_device::Server]. A device already registered
/// under the same path is replaced, its connected host (if any) stays connected to it until either side drops.
///
/// `max_message_len` is reported to the host as the longest message the device can receive.
pub fn device(path: &str, max_message_len: usize) -> (DeviceTx, DeviceRx) {
    let (connections_tx, connections) = register(path, None);
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
    usb_packet: Option<usize>,
) -> (
    mpsc::UnboundedSender<Connection>,
    mpsc::UnboundedReceiver<Connection>,
) {
    let (connections_tx, connections) = mpsc::unbounded_channel();
    DEVICES.lock().unwrap().insert(
        path.to_string(),
        Registered {
            connections: connections_tx.clone(),
            usb_packet,
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
    let (connections_tx, connections) = register(path, Some(max_packet_size));
    let to_host = Arc::new(Mutex::new(None));
    (
        UsbSink {
            to_host: to_host.clone(),
            max_packet_size,
        },
        UsbSource {
            path: path.to_string(),
            connections_tx,
            connections,
            to_device: None,
            to_host,
            max_packet_size,
        },
    )
}

/// Device IN endpoint of a simulated USB device, see [usb_device].
pub struct UsbSink {
    to_host: Arc<Mutex<Option<mpsc::UnboundedSender<Message>>>>,
    max_packet_size: usize,
}

impl ww_device::PacketSink for UsbSink {
    type Error = Disconnected;

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
pub struct UsbSource {
    path: String,
    connections_tx: mpsc::UnboundedSender<Connection>,
    connections: mpsc::UnboundedReceiver<Connection>,
    to_device: Option<mpsc::UnboundedReceiver<Message>>,
    to_host: Arc<Mutex<Option<mpsc::UnboundedSender<Message>>>>,
    max_packet_size: usize,
}

impl ww_device::PacketSource for UsbSource {
    type Error = Disconnected;

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

impl Drop for UsbSource {
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

struct InProcessTransport;

impl Transport for InProcessTransport {
    type Tx = HostTx;
    type Rx = HostRx;

    async fn connect(&mut self, handle: DeviceHandle) -> Result<Opened<HostTx, HostRx>, String> {
        let path = handle
            .downcast::<String>()
            .map_err(|_| "expected in-process device path".to_string())?;
        let (device, usb_packet) = DEVICES
            .lock()
            .unwrap()
            .get(path.as_str())
            .map(|r| (r.connections.clone(), r.usb_packet))
            .ok_or_else(|| format!("in-process device '{path}' is gone"))?;
        let (to_device_tx, to_device) = mpsc::unbounded_channel();
        let (to_host, to_host_rx) = mpsc::unbounded_channel();
        device
            .send(Connection { to_device, to_host })
            .map_err(|_| format!("in-process device '{path}' is gone"))?;
        Ok(Opened {
            tx: HostTx {
                to_device: to_device_tx,
                framer: usb_packet.map(UsbTxFramer::new),
            },
            rx: HostRx {
                from_device: to_host_rx,
                message: None,
                framer: usb_packet.map(|p| UsbRxFramer::new(crate::DEFAULT_MAX_MESSAGE_SIZE + p)),
            },
        })
    }
}

type UsbTxFramer = ww_framer::TxOwned<ww_link::UsbHead, ww_link::UsbChecksum, ww_link::UsbTail>;
type UsbRxFramer =
    ww_framer::FramedRxOwned<ww_link::UsbHead, ww_link::UsbChecksum, ww_link::UsbTail>;

struct HostTx {
    to_device: mpsc::UnboundedSender<Message>,
    /// Some for a simulated USB device: messages are packed into packets
    framer: Option<UsbTxFramer>,
}

fn send(to_device: &mpsc::UnboundedSender<Message>, message: Message) -> Result<(), String> {
    to_device
        .send(message)
        .map_err(|_| "in-process device is gone".to_string())
}

impl MessageTx for HostTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        let Some(framer) = self.framer.as_mut() else {
            return send(&self.to_device, (kind, message.to_vec()));
        };
        loop {
            match framer.write(kind, message) {
                Ok(true) => return Ok(()),
                // frame is full (or the message continues into the next one): send and retry
                Ok(false) => match framer.flush_to_vec() {
                    Some(packet) => send(&self.to_device, (0, packet))?,
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
            send(&self.to_device, (0, packet))?;
        }
        Ok(())
    }
}

struct HostRx {
    from_device: mpsc::UnboundedReceiver<Message>,
    /// Last received message, returned borrowed
    message: Option<Message>,
    /// Some for a simulated USB device: packets are reassembled into messages
    framer: Option<UsbRxFramer>,
}

impl MessageRx for HostRx {
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
            framer
                .stage(&packet)
                .map_err(|_| "out of staging area, this is a bug".to_string())?;
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
        type Tx = HostTx;
        type Rx = HostRx;
        async fn connect(&mut self, _: DeviceHandle) -> Result<Opened<HostTx, HostRx>, String> {
            InProcessTransport.connect(Box::new(self.0.clone())).await
        }
    }

    #[tokio::test]
    async fn host_and_device_talk_over_simulated_usb() {
        const PACKET: usize = 64;
        let path = "in_process/usb_talk";
        let (sink, source) = usb_device(path, PACKET);
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
