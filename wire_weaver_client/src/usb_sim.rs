//! Simulated USB device on a local socket: a device served by one process that any number of host processes can
//! open, one of them holding its interface at a time, as with a real USB device. Reached with
//! [ClientConfig::usb_sim_path](crate::ClientConfig::usb_sim_path), held in the holders registry under the key
//! `usbsim:<socket path>` and opened by the ww hub under the same key.
//!
//! What it models of USB, and the in-process [usb_device](crate::in_process::usb_device) doesn't: the device lives on
//! when its host goes. Packets carry the USB framer configuration of [ww_link] both ways (each one a USB packet of at
//! most the endpoint's size); a host claims the interface before it sends or receives, releases it (or closes the
//! connection, as a process exiting closes its usbfs file) and another host claims it after, while the device sees
//! one medium all along: its link goes on, or times out by its own peer timeout, never because a socket closed. IN
//! packets wait in the device while nobody holds the interface (a real endpoint is NAKed until the host polls it).
//! That is what a live link handover (CLT-44) needs to be tested across processes: the holder releases, the hub
//! claims, and the device must not notice.
//!
//! On the socket, messages as [ww_local_socket::MessageWriter] frames them (`| len | kind | payload |`), kinds:
//! - host → device: [PACKET] (an OUT packet), [CLAIM], [RELEASE];
//! - device → host: [PACKET] (an IN packet), [CLAIMED] (payload one byte: 0 granted, 1 held by another host),
//!   [RELEASED] (nothing more comes for this host after it, every IN packet sent before it was for this host).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::bail;
use interprocess::local_socket::tokio::{RecvHalf, SendHalf};
use interprocess::local_socket::traits::tokio::Stream as _;
use tokio::sync::{mpsc, watch};
use tracing::{debug, trace, warn};
use ww_link::{UsbFraming, UsbFramingProtocol};
use ww_local_socket::{Listener, MessageReader, MessageWriter};

use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::transport::{
    MessageRx, MessageTx, Opened, Selected, SharedClaim, Transport, handover_requested,
};
use crate::framing::{UsbRx as RxFramer, UsbTx as TxFramer};

/// An OUT packet (host to device) or an IN packet (device to host).
pub const PACKET: u8 = 0;
/// Host asks to hold the interface.
pub const CLAIM: u8 = 1;
/// Host lets go of the interface.
pub const RELEASE: u8 = 2;
/// Device answers [CLAIM]: one byte, 0 granted, 1 held by another host.
pub const CLAIMED: u8 = 3;
/// Device answers [RELEASE].
pub const RELEASED: u8 = 4;

/// The device's descriptor bytes a host learns before it claims: `| max_packet: u16 LE | seq_prefix: u8 |`, sent as
/// the first message of every connection.
const INFO: u8 = 5;

/// Longest packet a simulated endpoint takes (a high-speed bulk endpoint).
const MAX_PACKET: usize = 512;

// Device side

/// What the connections and the device's endpoints share.
struct Bus {
    /// The connection holding the interface and where its IN packets go
    holder: Mutex<Option<(u64, mpsc::UnboundedSender<(u8, Vec<u8>)>)>>,
    /// Bumped on every claim and release
    changed: watch::Sender<u64>,
    /// A host claimed the interface at least once: the device is configured
    configured: watch::Sender<bool>,
}

impl Bus {
    fn bump(&self) {
        self.changed.send_modify(|n| *n += 1);
    }
}

/// Serves a simulated USB device on `listener`, speaking packets of at most `max_packet_size` bytes (64 for full
/// speed, 512 for high speed) with [UsbFraming]. The device wraps the returned halves in [ww_device::FramedTx] /
/// [ww_device::FramedRx] as it does its USB endpoints. Needs a tokio runtime; accepting stops when the halves are
/// dropped.
pub fn device(listener: Listener, max_packet_size: usize) -> (UsbSink, UsbSource) {
    device_framed::<UsbFraming>(listener, max_packet_size)
}

/// Same as [device], with the framing `F` the device uses ([UsbFraming] or [ww_link::UsbSeqFraming]); hosts learn it
/// as from a real device's interface protocol.
pub fn device_framed<F: UsbFramingProtocol>(
    listener: Listener,
    max_packet_size: usize,
) -> (UsbSink<F>, UsbSource<F>) {
    assert!(max_packet_size <= MAX_PACKET && max_packet_size >= 8);
    let seq_prefix = F::INTERFACE_PROTOCOL == ww_link::USB_INTERFACE_PROTOCOL_SEQ;
    let bus = Arc::new(Bus {
        holder: Mutex::new(None),
        changed: watch::channel(0).0,
        configured: watch::channel(false).0,
    });
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let accepting = tokio::spawn(accept(
        listener,
        bus.clone(),
        out_tx,
        max_packet_size,
        seq_prefix,
    ));
    (
        UsbSink {
            bus: bus.clone(),
            changed: bus.changed.subscribe(),
            max_packet_size,
            _framing: std::marker::PhantomData,
        },
        UsbSource {
            configured: bus.configured.subscribe(),
            out_rx,
            max_packet_size,
            accepting,
            _framing: std::marker::PhantomData,
        },
    )
}

async fn accept(
    listener: Listener,
    bus: Arc<Bus>,
    out_tx: mpsc::UnboundedSender<Vec<u8>>,
    max_packet: usize,
    seq_prefix: bool,
) {
    let mut next_id = 1u64;
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                warn!("usb sim: accept: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
        };
        let id = next_id;
        next_id += 1;
        debug!("usb sim: host {id} ({peer:?}) opened the device");
        let (rx, tx) = stream.split();
        tokio::spawn(serve_host(
            id,
            rx,
            tx,
            bus.clone(),
            out_tx.clone(),
            max_packet,
            seq_prefix,
        ));
    }
}

/// One host's connection: claims, releases and OUT packets in, IN packets and answers out, in order.
async fn serve_host(
    id: u64,
    rx: RecvHalf,
    tx: SendHalf,
    bus: Arc<Bus>,
    out_tx: mpsc::UnboundedSender<Vec<u8>>,
    max_packet: usize,
    seq_prefix: bool,
) {
    let (to_host, mut to_host_rx) = mpsc::unbounded_channel::<(u8, Vec<u8>)>();
    let mut info = (max_packet as u16).to_le_bytes().to_vec();
    info.push(seq_prefix as u8);
    _ = to_host.send((INFO, info));
    let writer = tokio::spawn(async move {
        let mut writer = MessageWriter::new(tx);
        while let Some((kind, bytes)) = to_host_rx.recv().await {
            writer.push(kind, &bytes);
            // what else is queued goes in the same write
            while let Ok((kind, bytes)) = to_host_rx.try_recv() {
                writer.push(kind, &bytes);
            }
            if writer.flush().await.is_err() {
                break;
            }
        }
        let _ = writer.shutdown().await;
    });
    let mut reader = MessageReader::new(rx, MAX_PACKET);
    let holds = |bus: &Bus| bus.holder.lock().unwrap().as_ref().map(|h| h.0) == Some(id);
    while let Ok((kind, bytes)) = reader.read_message().await {
        match kind {
            PACKET if holds(&bus) => {
                if bytes.len() > max_packet {
                    warn!("usb sim: host {id} sent a packet of {} bytes", bytes.len());
                    continue;
                }
                _ = out_tx.send(bytes.to_vec());
            }
            PACKET => trace!("usb sim: host {id} sent a packet without holding the interface"),
            CLAIM => {
                let granted = {
                    let mut holder = bus.holder.lock().unwrap();
                    match holder.as_ref() {
                        Some((h, _)) if *h != id => false,
                        _ => {
                            *holder = Some((id, to_host.clone()));
                            true
                        }
                    }
                };
                _ = to_host.send((CLAIMED, vec![!granted as u8]));
                if granted {
                    debug!("usb sim: host {id} claimed the interface");
                    bus.configured.send_replace(true);
                    bus.bump();
                }
            }
            RELEASE => {
                let released = {
                    let mut holder = bus.holder.lock().unwrap();
                    let ours = holder.as_ref().is_some_and(|h| h.0 == id);
                    if ours {
                        *holder = None;
                    }
                    ours
                };
                // after every IN packet already queued for this host: the same channel
                _ = to_host.send((RELEASED, vec![]));
                if released {
                    debug!("usb sim: host {id} released the interface");
                    bus.bump();
                }
            }
            other => debug!("usb sim: host {id} sent unknown kind {other}"),
        }
    }
    // the host is gone: its claim goes, as the kernel releases a closed usbfs file's interfaces
    let was_holder = {
        let mut holder = bus.holder.lock().unwrap();
        let ours = holder.as_ref().is_some_and(|h| h.0 == id);
        if ours {
            *holder = None;
        }
        ours
    };
    if was_holder {
        debug!("usb sim: host {id} closed the device holding the interface");
        bus.bump();
    }
    drop(to_host);
    let _ = writer.await;
}

/// Device IN endpoint of a simulated USB device, see [device]. A packet waits while no host holds the interface.
pub struct UsbSink<F = UsbFraming> {
    bus: Arc<Bus>,
    changed: watch::Receiver<u64>,
    max_packet_size: usize,
    _framing: std::marker::PhantomData<fn() -> F>,
}

/// Error of [UsbSink] and [UsbSource]: the listener is gone.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Gone;

impl<F: UsbFramingProtocol> ww_device::PacketSink for UsbSink<F> {
    type Error = Gone;
    type Framing = F;

    async fn write_packet(&mut self, packet: &[u8]) -> Result<(), Gone> {
        assert!(
            packet.len() <= self.max_packet_size,
            "packet longer than the endpoint's max packet size"
        );
        loop {
            let sent = {
                let mut holder = self.bus.holder.lock().unwrap();
                match holder.as_ref() {
                    Some((_, to_host)) => {
                        let sent = to_host.send((PACKET, packet.to_vec())).is_ok();
                        if !sent {
                            *holder = None;
                        }
                        sent
                    }
                    None => false,
                }
            };
            if sent {
                return Ok(());
            }
            // NAKed until a host holds the interface
            self.changed.changed().await.map_err(|_| Gone)?;
        }
    }
}

/// Device OUT endpoint of a simulated USB device, see [device]. Dropping it stops accepting hosts.
pub struct UsbSource<F = UsbFraming> {
    configured: watch::Receiver<bool>,
    out_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    max_packet_size: usize,
    accepting: tokio::task::JoinHandle<()>,
    _framing: std::marker::PhantomData<fn() -> F>,
}

impl<F: UsbFramingProtocol> ww_device::PacketSource for UsbSource<F> {
    type Error = Gone;
    type Framing = F;

    fn max_packet_len(&self) -> usize {
        self.max_packet_size
    }

    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, Gone> {
        // cancel-safe: tokio's recv() does not lose packets when dropped
        let packet = self.out_rx.recv().await.ok_or(Gone)?;
        buf[..packet.len()].copy_from_slice(&packet);
        Ok(packet.len())
    }

    /// Once a host claimed the interface for the first time; after that the device stays configured, whichever
    /// host comes and goes.
    async fn wait_connected(&mut self) {
        if self.configured.wait_for(|c| *c).await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

impl<F> Drop for UsbSource<F> {
    fn drop(&mut self) {
        self.accepting.abort();
    }
}

// Host side

/// Where to connect, and the device's claim in the holders registry.
pub(crate) struct UsbSimTarget {
    pub(crate) path: PathBuf,
    pub(crate) claim: Option<crate::holders::Claim>,
}

/// The holders registry key of the simulated device on `path`.
pub fn key_for(path: &Path) -> String {
    format!("usbsim:{}", path.display())
}

pub(crate) async fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
    supervised: bool,
) -> Result<Selected, anyhow::Error> {
    let Some(path) = c.pieces.iter().rev().find_map(|p| match p {
        ConfigPiece::UsbSimPath { path } => Some(PathBuf::from(path)),
        _ => None,
    }) else {
        bail!("Internal error: simulated USB selected without a path");
    };
    let key = key_for(&path);
    #[cfg(feature = "hub")]
    if let crate::hub::Route::Hub(tunnel) = crate::hub::route(c, &key, false).await? {
        return crate::local_socket::started(cmd_rx, &key, *tunnel, true);
    }
    let claim = crate::local_socket::hold(c, &key, supervised)?;
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    tokio::spawn(crate::event_loop::core::worker(cmd_rx, UsbSimTransport));
    Ok(Selected::Device {
        info: Box::new(crate::DeviceInfo {
            location: key,
            manufacturer: String::new(),
            product: String::new(),
            serials: vec![],
            user_label: String::new(),
            api: None,
            usb: None,
        }),
        handle: Box::new(UsbSimTarget { path, claim }),
        via_hub: false,
    })
}

pub(crate) struct UsbSimTransport;

/// An opened simulated device whose interface this host holds.
pub(crate) struct Claimed {
    pub(crate) rx: MessageReader<RecvHalf>,
    pub(crate) tx: MessageWriter<SendHalf>,
    pub(crate) max_packet: usize,
    pub(crate) seq_prefix: bool,
}

/// Opens the device on `path` and claims its interface.
pub(crate) async fn open(path: &Path) -> Result<Claimed, String> {
    let shown = path.display();
    let (stream, _) = ww_local_socket::connect(path)
        .await
        .map_err(|e| format!("simulated USB device {shown}: {e}"))?;
    let (rx, tx) = stream.split();
    let mut rx = MessageReader::new(rx, MAX_PACKET);
    let tx = MessageWriter::new(tx);
    let (max_packet, seq_prefix) = match rx.read_message().await {
        Ok((INFO, [a, b, seq])) => (u16::from_le_bytes([*a, *b]) as usize, *seq != 0),
        Ok(_) => return Err(format!("{shown}: not a simulated USB device")),
        Err(e) => return Err(format!("simulated USB device {shown}: {e}")),
    };
    let mut claimed = Claimed {
        rx,
        tx,
        max_packet,
        seq_prefix,
    };
    claimed.claim().await.map_err(|e| format!("{shown}: {e}"))?;
    Ok(claimed)
}

impl Claimed {
    /// Claims the interface (again, after a [Self::release]).
    pub(crate) async fn claim(&mut self) -> Result<(), String> {
        self.tx.push(CLAIM, &[]);
        self.tx.flush().await.map_err(|e| e.to_string())?;
        loop {
            match self.rx.read_message().await.map_err(|e| e.to_string())? {
                (CLAIMED, [0]) => return Ok(()),
                (CLAIMED, _) => return Err("the interface is held by another host".into()),
                // IN packets can't come before the claim; anything else is old
                (kind, _) => trace!("usb sim: kind {kind} while claiming, dropped"),
            }
        }
    }

    /// Lets go of the interface: every IN packet the device sent this host before is handed to `packet`, nothing
    /// comes after.
    pub(crate) async fn release(&mut self, mut packet: impl FnMut(&[u8])) -> Result<(), String> {
        self.tx.push(RELEASE, &[]);
        self.tx.flush().await.map_err(|e| e.to_string())?;
        loop {
            match self.rx.read_message().await.map_err(|e| e.to_string())? {
                (RELEASED, _) => return Ok(()),
                (PACKET, bytes) => packet(bytes),
                (kind, _) => trace!("usb sim: kind {kind} while releasing, dropped"),
            }
        }
    }
}

impl Transport for UsbSimTransport {
    fn link_setup_timeout(&self) -> std::time::Duration {
        crate::event_loop::transport::link_setup_timeout::USB
    }

    type Tx = UsbSimTx;
    type Rx = UsbSimRx;

    async fn connect(
        &mut self,
        handle: DeviceHandle,
        tap: ww_tap::RecorderLink,
    ) -> Result<Opened<UsbSimTx, UsbSimRx>, String> {
        let target = handle
            .downcast::<UsbSimTarget>()
            .map_err(|_| "expected a simulated USB device".to_string())?;
        let Claimed {
            rx,
            tx,
            max_packet,
            seq_prefix,
        } = open(&target.path).await?;
        let claim: SharedClaim = target.claim.map(Arc::new);
        let mut tap = tap;
        ww_tap::Tap::session(
            &mut tap,
            &crate::tap::host_session(
                ww_tap::Medium::InProcessUsb,
                crate::framing::usb_tap_framer(seq_prefix),
                max_packet,
                &key_for(&target.path),
            ),
        );
        Ok(Opened {
            tx: UsbSimTx {
                framer: TxFramer::new(seq_prefix, max_packet),
                writer: tx,
                tap: tap.clone(),
                claim: claim.clone(),
            },
            rx: UsbSimRx {
                // one maximum message plus one more packet, see FramedRx docs
                framer: RxFramer::new(seq_prefix, crate::DEFAULT_MAX_MESSAGE_SIZE + max_packet),
                reader: rx,
                tap,
                _claim: claim,
            },
        })
    }
}

pub(crate) struct UsbSimTx {
    framer: TxFramer,
    writer: MessageWriter<SendHalf>,
    tap: ww_tap::RecorderLink,
    claim: SharedClaim,
}

impl UsbSimTx {
    /// Queues the frame being assembled, if any; the socket write happens on [MessageTx::flush] or when a frame
    /// filled up.
    fn push_frame(&mut self) -> bool {
        let Some(frame) = self.framer.flush_to_vec() else {
            return false;
        };
        if ww_tap::Tap::wants_frames(&self.tap) {
            ww_tap::Tap::frame(&mut self.tap, ww_tap::Dir::Tx, &frame);
        }
        self.writer.push(PACKET, &frame);
        true
    }
}

impl MessageTx for UsbSimTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        loop {
            match self.framer.write(kind, message) {
                Ok(true) => return Ok(()),
                // frame is full (or the message continues into the next one): send and retry
                Ok(false) => {
                    if !self.push_frame() {
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
        self.push_frame();
        self.writer.flush().await.map_err(|e| e.to_string())
    }

    async fn handover_requested(&mut self) {
        handover_requested(&self.claim).await
    }

    async fn close(mut self) {
        let _ = self.writer.flush().await;
        let _ = self.writer.shutdown().await;
    }
}

pub(crate) struct UsbSimRx {
    framer: RxFramer,
    reader: MessageReader<RecvHalf>,
    tap: ww_tap::RecorderLink,
    /// Shared with [UsbSimTx]: released once both halves let go of the device
    _claim: SharedClaim,
}

impl MessageRx for UsbSimRx {
    /// Cancel-safe: staged packets live in the framer, a partly read packet in the reader.
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        loop {
            self.framer.reassemble();
            if self.framer.message().is_some() {
                break;
            }
            let (kind, packet) = self
                .reader
                .read_message()
                .await
                .map_err(|e| match e.kind() {
                    std::io::ErrorKind::UnexpectedEof => "simulated USB device is gone".to_string(),
                    _ => e.to_string(),
                })?;
            if kind != PACKET {
                trace!("usb sim: kind {kind} while running, dropped");
                continue;
            }
            if ww_tap::Tap::wants_frames(&self.tap) {
                ww_tap::Tap::frame(&mut self.tap, ww_tap::Dir::Rx, packet);
            }
            let report = self
                .framer
                .stage(packet)
                .map_err(|_| "out of staging area, this is a bug".to_string())?;
            crate::framing::log_report(report);
        }
        Ok(self.framer.message().expect("checked above"))
    }
}

#[cfg(test)]
mod tests {
    //! Host event loop against [ww_device::Server] over the simulated USB device on a socket, and the claim rules.
    use super::*;
    use crate::event_loop::device_e2e_tests::{DEV_MAX_MESSAGE, serve, talk};
    use ww_device::{PacketSink, PacketSource};

    fn socket(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ww-usbsim-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("dev.sock")
    }

    /// Tests send `()` as a handle
    struct To(PathBuf);

    impl Transport for To {
        fn link_setup_timeout(&self) -> std::time::Duration {
            crate::event_loop::transport::link_setup_timeout::USB
        }

        type Tx = UsbSimTx;
        type Rx = UsbSimRx;
        async fn connect(
            &mut self,
            _: DeviceHandle,
            tap: ww_tap::RecorderLink,
        ) -> Result<Opened<UsbSimTx, UsbSimRx>, String> {
            let target = UsbSimTarget {
                path: self.0.clone(),
                claim: None,
            };
            UsbSimTransport.connect(Box::new(target), tap).await
        }
    }

    #[tokio::test]
    async fn host_and_device_talk() {
        const PACKET: usize = 64;
        let path = socket("talk");
        let listener = ww_local_socket::listen(&path, true).unwrap();
        let (sink, source) = device_framed::<ww_link::UsbSeqFraming>(listener, PACKET);
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
        let host = tokio::spawn(crate::event_loop::core::worker(cmd_rx, To(path)));
        talk((cmd_tx, events_rx, dev, host)).await
    }

    /// One host holds the interface at a time; IN packets wait while nobody holds it and go to the next holder; the
    /// device's endpoints don't notice hosts coming and going.
    #[tokio::test]
    async fn one_holder_at_a_time() {
        let path = socket("claims");
        let listener = ww_local_socket::listen(&path, true).unwrap();
        let (mut sink, mut source) = device(listener, 64);
        let mut a = open(&path).await.unwrap();
        assert_eq!((a.max_packet, a.seq_prefix), (64, false));
        source.wait_connected().await;
        let b = open(&path).await;
        assert_eq!(
            b.err().unwrap(),
            format!("{}: the interface is held by another host", path.display())
        );
        a.tx.push(PACKET, &[1, 2]);
        a.tx.flush().await.unwrap();
        let mut buf = [0u8; 64];
        assert_eq!(source.read_packet(&mut buf).await.unwrap(), 2);
        sink.write_packet(&[7]).await.unwrap();
        let mut reaped = vec![];
        a.release(|p| reaped.push(p.to_vec())).await.unwrap();
        assert_eq!(reaped, [vec![7]]);
        // nobody holds it: the packet waits
        let held = tokio::spawn(async move {
            sink.write_packet(&[8]).await.unwrap();
            sink
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!held.is_finished());
        let mut b = open(&path).await.unwrap();
        held.await.unwrap();
        assert_eq!(b.rx.read_message().await.unwrap(), (PACKET, &[8u8][..]));
        // a host that goes without releasing lets go too
        drop(b);
        let mut a2 = open(&path).await;
        for _ in 0..50 {
            if a2.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            a2 = open(&path).await;
        }
        a2.unwrap();
        drop(a);
    }
}
