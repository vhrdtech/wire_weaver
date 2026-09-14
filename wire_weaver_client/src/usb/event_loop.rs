//! Async USB wrapper around the sans-IO [TxCore] / [RxCore]: two tasks, one per direction, each
//! owning its endpoint and framer. Rx never waits on tx, so a half-duplex device cannot deadlock the host;
//! backpressure to the [Commander](crate::Commander) is simply the tx task being blocked in a write.

use std::time::{Duration, Instant};

use nusb::descriptors::TransferType;
use nusb::transfer::TransferError;
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

use super::tracing::Tracer;
use super::ww_nusb::{Sink, Source};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::{Command, EventLoopExitReason};
use crate::event_loop::core::{
    MAX_MESSAGE_SIZE, RxCore, RxInput, RxOutput, ToRx, ToTx, TxCore, TxInput, TxOutput,
};
use crate::event_loop::framing::Framing;

/// USB packets are checked by the hardware, but a CRC over each message still catches
/// packets lost or reordered on re-connection.
type UsbFraming = Framing<ww_link::Head, ww_link::Checksum, ww_link::Tail>;

/// Give the last (Disconnect) transfer a chance to actually go out before endpoints are dropped.
const EXIT_LINGER: Duration = Duration::from_millis(3);

/// Packet tx half, implemented for nusb and for a mock in tests.
pub(crate) trait TxEndpoint: Send + 'static {
    fn write_packet(&mut self, frame: &[u8]) -> impl Future<Output = Result<(), String>> + Send;
}

/// Packet rx half.
pub(crate) trait RxEndpoint: Send + 'static {
    /// Cancel-safe.
    fn read_packet(&mut self, buf: &mut [u8])
    -> impl Future<Output = Result<usize, String>> + Send;
}

pub(crate) struct Opened<Tx, Rx> {
    pub tx: Tx,
    pub rx: Rx,
    pub max_packet_size: usize,
}

/// Opens endpoints for a handle from [Command::Connect].
pub(crate) trait Connector {
    type Tx: TxEndpoint;
    type Rx: RxEndpoint;
    fn connect(&mut self, handle: DeviceHandle) -> Result<Opened<Self::Tx, Self::Rx>, String>;
}

pub async fn usb_worker(cmd_rx: mpsc::Receiver<Command>) {
    debug!("usb worker started");
    worker(cmd_rx, NusbConnector).await;
    debug!("usb worker exited");
}

pub(crate) async fn worker<C: Connector>(cmd_rx: mpsc::Receiver<Command>, connector: C) {
    let (to_rx_tx, to_rx_rx) = mpsc::unbounded_channel::<ToRx>();
    let (to_tx_tx, to_tx_rx) = mpsc::unbounded_channel::<ToTx>();
    // rx endpoint is opened by the tx task (it handles Connect) and handed over
    let (rx_ep_tx, rx_ep_rx) = mpsc::channel::<(C::Rx, usize)>(1);

    let rx_task = tokio::spawn(rx_task::<C::Rx>(rx_ep_rx, to_rx_rx, to_tx_tx));
    let (tx_core_result, cmd_rx) = tx_task(cmd_rx, connector, to_tx_rx, to_rx_tx, rx_ep_tx).await;
    let rx_core = match rx_task.await {
        Ok(rx_core) => rx_core,
        Err(e) => {
            warn!("rx task panicked: {e}");
            RxCore::new()
        }
    };

    let (tx_core, result) = tx_core_result;
    let (exited_tx, residual) = tx_core.into_residual(cmd_rx, rx_core.into_connected_tx(), result);
    if let Some(tx) = exited_tx {
        _ = tx.send(residual);
    }
}

async fn tx_task<C: Connector>(
    mut cmd_rx: mpsc::Receiver<Command>,
    mut connector: C,
    mut from_rx: mpsc::UnboundedReceiver<ToTx>,
    to_rx: mpsc::UnboundedSender<ToRx>,
    rx_ep_tx: mpsc::Sender<(C::Rx, usize)>,
) -> (
    (TxCore, anyhow::Result<EventLoopExitReason>),
    mpsc::Receiver<Command>,
) {
    let mut core = TxCore::new();
    let mut ep: Option<(C::Tx, UsbFraming)> = None;
    let mut frames: Vec<Vec<u8>> = vec![];

    let result = loop {
        // Drain outputs first, do not feed new inputs until done.
        // Awaiting writes here is fine: rx runs on its own task.
        let mut exit = None;
        while let Some(output) = core.poll_output() {
            match output {
                TxOutput::Connect(handle) => match connector.connect(handle) {
                    Ok(o) => {
                        ep = Some((o.tx, UsbFraming::new(o.max_packet_size, MAX_MESSAGE_SIZE)));
                        if rx_ep_tx.try_send((o.rx, o.max_packet_size)).is_err() {
                            core.handle(
                                Instant::now(),
                                TxInput::TransportError("rx task is gone".into()),
                            );
                        } else {
                            core.handle(Instant::now(), TxInput::TransportUp);
                        }
                    }
                    Err(e) => core.handle(Instant::now(), TxInput::TransportError(e)),
                },
                TxOutput::Send { kind, payload } => {
                    let Some((_, framing)) = ep.as_mut() else {
                        warn!("Send without transport, dropping");
                        continue;
                    };
                    if let Err(e) = framing.write(kind, &payload, &mut frames) {
                        // Core is responsible for not sending oversized messages, so this is a bug or a
                        // wrong frame size, not something the device did
                        core.handle(Instant::now(), TxInput::TransportError(e.to_string()));
                    }
                }
                TxOutput::Flush => {
                    if let Some((_, framing)) = ep.as_mut() {
                        framing.flush(&mut frames);
                    }
                }
                TxOutput::ToRx(msg) => {
                    if to_rx.send(msg).is_err() {
                        // rx task exited; it would have sent PeerGone first, which is queued in from_rx
                    }
                }
                TxOutput::Exit(result) => {
                    exit = Some(result);
                    break;
                }
            }
            if let Some((tx, _)) = ep.as_mut()
                && let Err(e) = write_frames(tx, &mut frames).await
            {
                frames.clear();
                core.handle(Instant::now(), TxInput::TransportError(e));
            }
        }
        if let Some(result) = exit {
            if let Some((mut tx, mut framing)) = ep.take() {
                framing.flush(&mut frames);
                _ = write_frames(&mut tx, &mut frames).await;
                tokio::time::sleep(EXIT_LINGER).await;
            }
            break result;
        }

        let deadline = core.poll_timeout();
        let timer = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at.into()).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            msg = from_rx.recv() => match msg {
                Some(msg) => core.handle(Instant::now(), TxInput::FromRx(msg)),
                None => core.handle(Instant::now(), TxInput::TransportError("rx task is gone".into())),
            },
            cmd = cmd_rx.recv() => match cmd {
                Some(cmd) => core.handle(Instant::now(), TxInput::Command(cmd)),
                None => core.handle(Instant::now(), TxInput::CommanderDropped),
            },
            _ = timer => core.handle(Instant::now(), TxInput::Timer),
        }
    };
    drop(ep); // closes the device from tx side, rx read then fails or is stopped by ToRx::Stop
    ((core, result), cmd_rx)
}

async fn write_frames<T: TxEndpoint>(tx: &mut T, frames: &mut Vec<Vec<u8>>) -> Result<(), String> {
    for frame in frames.drain(..) {
        trace!("tx frame: {}: {frame:02x?}", frame.len());
        tx.write_packet(&frame).await?;
    }
    Ok(())
}

async fn rx_task<R: RxEndpoint>(
    mut ep_rx: mpsc::Receiver<(R, usize)>,
    mut from_tx: mpsc::UnboundedReceiver<ToRx>,
    to_tx: mpsc::UnboundedSender<ToTx>,
) -> RxCore {
    let mut core = RxCore::new();
    let mut ep: Option<(R, UsbFraming)> = None;
    let mut buf = vec![0u8; 1024];

    loop {
        // Everything tx told us goes in first, see RxCore docs on ordering
        let mut exit = false;
        while let Ok(msg) = from_tx.try_recv() {
            core.handle(Instant::now(), RxInput::FromTx(msg));
        }
        while let Some(output) = core.poll_output() {
            match output {
                RxOutput::ToTx(msg) => _ = to_tx.send(msg),
                RxOutput::Exit => exit = true,
            }
        }
        if exit {
            break;
        }

        let deadline = core.poll_timeout();
        let timer = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at.into()).await,
                None => std::future::pending().await,
            }
        };
        let waiting_for_ep = ep.is_none();
        let read = async {
            match ep.as_mut() {
                Some((r, _)) => r.read_packet(&mut buf).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            // rx endpoint arrives with the first Connect; a later one replaces after a re-connect
            new_ep = ep_rx.recv(), if waiting_for_ep => match new_ep {
                Some((r, max_packet_size)) => ep = Some((r, UsbFraming::new(max_packet_size, MAX_MESSAGE_SIZE))),
                None => {
                    // tx task is gone without saying Stop (should not happen), exit
                    break;
                }
            },
            msg = from_tx.recv() => match msg {
                Some(msg) => core.handle(Instant::now(), RxInput::FromTx(msg)),
                None => break,
            },
            r = read => {
                let now = Instant::now();
                match r {
                    Ok(len) => {
                        let frame = &buf[..len];
                        trace!("rx frame: {len}: {frame:02x?}");
                        let (_, framing) = ep.as_mut().expect("read only when ep is Some");
                        if let Err(e) = framing.stage(frame) {
                            warn!("{e}, dropping frame");
                        }
                        // drain tx messages before decoding, so that Expect precedes its answer
                        while let Ok(msg) = from_tx.try_recv() {
                            core.handle(now, RxInput::FromTx(msg));
                        }
                        while let Some((kind, payload)) = framing.next_message() {
                            core.handle(now, RxInput::Message { kind, payload });
                        }
                    }
                    Err(e) => {
                        ep = None;
                        core.handle(now, RxInput::TransportError(e));
                    }
                }
            }
            _ = timer => core.handle(Instant::now(), RxInput::Timer),
        }
    }
    drop(ep);
    core
}

// nusb implementation

struct NusbConnector;

pub(crate) struct NusbTx {
    sink: Sink,
    tracer: Tracer,
    /// Keeps the device open while endpoints are in use
    _device: nusb::Device,
}

pub(crate) struct NusbRx {
    source: Source,
    tracer: Tracer,
}

impl Connector for NusbConnector {
    type Tx = NusbTx;
    type Rx = NusbRx;

    fn connect(&mut self, handle: DeviceHandle) -> Result<Opened<NusbTx, NusbRx>, String> {
        let di = handle
            .downcast::<nusb::DeviceInfo>()
            .map_err(|_| "expected nusb::DeviceInfo handle".to_string())?;
        let dev = super::connect::connect(&di).map_err(|e| format!("{e:#}"))?;
        trace!("max_packet_size: {}", dev.max_packet_size);
        let is_bulk = dev.transfer_type == TransferType::Bulk;
        let sink =
            Sink::new(&dev.interface, dev.max_packet_size, is_bulk).map_err(|e| e.to_string())?;
        let source =
            Source::new(&dev.interface, dev.max_packet_size, is_bulk).map_err(|e| e.to_string())?;
        let tracer = Tracer::new(&di).map_err(|e| format!("usb tracing: {e:#}"))?;
        Ok(Opened {
            tx: NusbTx {
                sink,
                tracer: tracer.clone(),
                _device: dev.device,
            },
            rx: NusbRx { source, tracer },
            max_packet_size: dev.max_packet_size,
        })
    }
}

impl TxEndpoint for NusbTx {
    fn write_packet(&mut self, frame: &[u8]) -> impl Future<Output = Result<(), String>> + Send {
        async move {
            self.tracer.tx(frame);
            self.sink
                .write_packet(frame)
                .await
                .map_err(describe_transfer_error)
        }
    }
}

impl RxEndpoint for NusbRx {
    fn read_packet(
        &mut self,
        buf: &mut [u8],
    ) -> impl Future<Output = Result<usize, String>> + Send {
        async move {
            let len = self
                .source
                .read_packet(buf)
                .await
                .map_err(describe_transfer_error)?;
            self.tracer.rx(&buf[..len]);
            Ok(len)
        }
    }
}

fn describe_transfer_error(e: TransferError) -> String {
    format!("USB transfer error: {e:?}")
}

#[cfg(test)]
mod tests {
    //! Regression test for a deadlock between a half-duplex device and a half-duplex host:
    //! thousands of requests fired at once used to stall the loop until the write timeout hit.
    //!
    //! The mock device below is deliberately half-duplex (does not read while blocked writing),
    //! rx/tx channel depths mimic nusb queue sizes. Nothing here is timing dependent.
    //!
    //! To see the failure: run rx and tx on one task (e.g., `select!` over cmd_rx and read_packet with
    //! writes awaited inline, as the old loop did) — the test then hangs deterministically.

    use super::*;
    use crate::device_info::ConnectionInfo;
    use crate::event_loop::framing::Framing;
    use tokio::sync::oneshot;
    use ww_link::{DeviceInfo, Message};
    use ww_version::{
        ApiHashPair, CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version,
        VersionOwned,
    };

    const PKT: usize = 64;
    /// Host-side tx transfer slots (nusb TX_QUEUE_SIZE)
    const HOST_TX_DEPTH: usize = 4;
    /// Host-side rx transfer slots (nusb RX_QUEUE_SIZE)
    const HOST_RX_DEPTH: usize = 64;
    /// Each reply spans ~10 packets, so one host packet holding ~7 requests makes the device emit ~70 packets,
    /// more than the host has rx slots for. That is what closes the deadlock cycle when both sides are half-duplex.
    /// Same thing happens with tiny replies and 512 B USB packets (~80 requests per packet).
    const REPLY_LEN: usize = 600;

    struct MockTx(mpsc::Sender<Vec<u8>>);

    impl TxEndpoint for MockTx {
        async fn write_packet(&mut self, frame: &[u8]) -> Result<(), String> {
            self.0
                .send(frame.to_vec())
                .await
                .map_err(|_| "closed".to_string())
        }
    }

    struct MockRx(mpsc::Receiver<Vec<u8>>);

    impl RxEndpoint for MockRx {
        async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, String> {
            let f = self.0.recv().await.ok_or("closed")?;
            buf[..f.len()].copy_from_slice(&f);
            Ok(f.len())
        }
    }

    struct MockConnector {
        opened: Option<Opened<MockTx, MockRx>>,
    }

    impl Connector for MockConnector {
        type Tx = MockTx;
        type Rx = MockRx;
        fn connect(&mut self, _: DeviceHandle) -> Result<Opened<MockTx, MockRx>, String> {
            self.opened.take().ok_or("already connected".into())
        }
    }

    /// Half-duplex device: read a packet, decode, answer every request with one Value event,
    /// awaiting each write before reading the next packet.
    async fn half_duplex_device(
        mut from_host: mpsc::Receiver<Vec<u8>>,
        to_host: mpsc::Sender<Vec<u8>>,
    ) {
        let mut framing: Framing<ww_link::Head, ww_link::Checksum, ww_link::Tail> =
            Framing::new(PKT, 1024);
        let mut frames = vec![];
        let mut scratch = [0u8; 256];
        let mut send =
            |msg: &Message<'_>, framing: &mut Framing<_, _, _>, frames: &mut Vec<Vec<u8>>| {
                let (k, p) = msg.encode(&mut scratch).unwrap();
                framing.write(k, p, frames).unwrap();
                framing.flush(frames);
            };
        while let Some(pkt) = from_host.recv().await {
            framing.stage(&pkt).unwrap();
            while let Some((kind, payload)) = framing.next_message() {
                match Message::decode(kind, payload).unwrap() {
                    Message::GetDeviceInfo => send(
                        &Message::DeviceInfo(DeviceInfo {
                            dev_link_version: CompactVersion::new(GlobalTypeId::new(512), 0, 1, 0),
                            api_model_version: CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
                            user_api_version: FullVersion::new("test", Version::new(0, 1, 0)),
                            hash: ApiHashPair::empty(),
                            dev_max_message_len: 1024,
                            packet_accumulation_time_us: 100,
                        }),
                        &mut framing,
                        &mut frames,
                    ),
                    Message::LinkSetup(_) => send(&Message::LinkReady, &mut framing, &mut frames),
                    Message::Data { bytes, .. } => {
                        let seq = u16::from_le_bytes([bytes[0], bytes[1]]);
                        let mut value = bytes[2..].to_vec();
                        value.resize(REPLY_LEN, 0xEE);
                        let event = ww_client_server::Event {
                            seq,
                            result: Ok(ww_client_server::EventKind::Value {
                                data: wire_weaver::shrink_wrap::tail_bytes::TailBytes(&value),
                            }),
                        };
                        let mut buf = [0u8; 1024];
                        let bytes = wire_weaver::shrink_wrap::SerializeShrinkWrap::to_ww_bytes(
                            &event, &mut buf,
                        )
                        .unwrap();
                        // one frame per response: worst case for host rx
                        send(
                            &Message::Data { channel: 0, bytes },
                            &mut framing,
                            &mut frames,
                        );
                    }
                    Message::Disconnect(_) => return,
                    _ => {}
                }
            }
            // half-duplex: all responses are written out (each awaited) before reading again
            for f in frames.drain(..) {
                if to_host.send(f).await.is_err() {
                    return;
                }
            }
        }
    }

    #[tokio::test]
    async fn thousands_of_requests_do_not_deadlock() {
        const N: usize = 2000;
        let (host_tx, dev_rx) = mpsc::channel::<Vec<u8>>(HOST_TX_DEPTH);
        let (dev_tx, host_rx) = mpsc::channel::<Vec<u8>>(HOST_RX_DEPTH);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        tokio::spawn(half_duplex_device(dev_rx, dev_tx));
        let worker = tokio::spawn(worker(
            cmd_rx,
            MockConnector {
                opened: Some(Opened {
                    tx: MockTx(host_tx),
                    rx: MockRx(host_rx),
                    max_packet_size: PKT,
                }),
            },
        ));

        let (connected_tx, connected_rx) = oneshot::channel::<ConnectionInfo>();
        cmd_tx
            .send(Command::Connect {
                handle: Box::new(()),
                client_version: Box::new(FullVersionOwned::new(
                    "test".into(),
                    VersionOwned::new(0, 1, 0),
                )),
                connected_tx: Some(connected_tx),
                failed_tx: None,
            })
            .await
            .unwrap();
        connected_rx.await.unwrap().result.unwrap();

        // fire everything at once, exactly like a user looping over Commander::send_call
        let mut done = Vec::with_capacity(N);
        for i in 0..N as u32 {
            let (done_tx, done_rx) = oneshot::channel();
            cmd_tx
                .send(Command::SendMessage {
                    bytes: [0u8, 0]
                        .iter()
                        .chain(i.to_le_bytes().iter())
                        .copied()
                        .collect(),
                    done_tx: Some((done_tx, Duration::from_secs(5))),
                })
                .await
                .unwrap();
            done.push((i, done_rx));
        }
        for (i, rx) in done {
            let bytes = tokio::time::timeout(Duration::from_secs(30), rx)
                .await
                .expect("stalled")
                .unwrap()
                .unwrap_or_else(|e| panic!("request {i} failed: {e:?}"));
            assert_eq!(&bytes[..4], i.to_le_bytes());
            assert_eq!(bytes.len(), REPLY_LEN);
        }

        let (disconnected_tx, disconnected_rx) = oneshot::channel();
        cmd_tx
            .send(Command::DisconnectAndExit {
                disconnected_tx: Some(disconnected_tx),
                reason: ww_link::DisconnectReason::RequestByUser,
            })
            .await
            .unwrap();
        disconnected_rx.await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .expect("worker did not exit")
            .unwrap();
    }
}
