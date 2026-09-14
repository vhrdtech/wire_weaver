//! Async USB wrapper around the sans-IO [Core]: owns the command channel, nusb endpoints,
//! the framer and timers, and only shuffles [Input]s in and [Output]s out.
//!
//! Rx and tx are decoupled: the receive path is always polled, and a slow device only ever
//! results in commands not being accepted (backpressure onto the [Commander](crate::Commander)),
//! never in receiving stopping. Otherwise, a half-duplex device (one that does not read while it
//! is blocked writing) and a half-duplex host deadlock as soon as both directions fill up.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use nusb::descriptors::TransferType;
use nusb::transfer::TransferError;
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

use super::tracing::Tracer;
use super::ww_nusb::{Sink, Source};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::core::{Core, Input, MAX_MESSAGE_SIZE, Output};
use crate::event_loop::framing::Framing;

/// USB packets are checked by the hardware, but a CRC over each split message still catches
/// packets lost or reordered on re-connection.
type UsbFraming = Framing<ww_link::Head, ww_link::Checksum, ww_link::Tail>;

/// Stop accepting commands while this many frames are waiting to be sent.
const TX_HIGH_WATERMARK: usize = 16;
/// How long to try sending the last frames (Disconnect) to a device on exit.
const EXIT_FLUSH_TIMEOUT: Duration = Duration::from_millis(200);

/// Packet tx half, implemented for nusb and for a mock in tests.
pub(crate) trait TxEndpoint {
    /// Cancel-safe. After Ok, [Self::submit] will not block.
    async fn wait_ready(&mut self) -> Result<(), String>;
    /// Synchronous, only after [Self::wait_ready] returned Ok.
    fn submit(&mut self, frame: &[u8]) -> Result<(), String>;
}

/// Packet rx half.
pub(crate) trait RxEndpoint {
    /// Cancel-safe.
    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, String>;
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

struct Link<Tx, Rx> {
    tx: Tx,
    rx: Rx,
    framing: UsbFraming,
    /// Frames waiting for a tx buffer
    pending_tx: VecDeque<Vec<u8>>,
}

enum Step {
    Cmd(Option<Command>),
    Timer,
    TxReady(Result<(), String>),
    Rx(Result<usize, String>),
}

pub(crate) async fn worker<C: Connector>(mut cmd_rx: mpsc::Receiver<Command>, mut connector: C) {
    let mut core = Core::new();
    let mut link: Option<Link<C::Tx, C::Rx>> = None;
    let mut rx_buf = vec![0u8; 1024];
    let mut frames: Vec<Vec<u8>> = vec![];

    let result = loop {
        // Drain outputs first (synchronous), do not feed new inputs until done
        let mut exit = None;
        while let Some(output) = core.poll_output() {
            match output {
                Output::Connect(handle) => match connector.connect(handle) {
                    Ok(o) => {
                        link = Some(Link {
                            tx: o.tx,
                            rx: o.rx,
                            framing: UsbFraming::new(o.max_packet_size, MAX_MESSAGE_SIZE),
                            pending_tx: VecDeque::new(),
                        });
                        core.handle(Instant::now(), Input::TransportUp);
                    }
                    Err(e) => core.handle(Instant::now(), Input::TransportError(e)),
                },
                Output::Send { kind, payload } => {
                    let Some(l) = link.as_mut() else {
                        warn!("Send without transport, dropping");
                        continue;
                    };
                    if let Err(e) = l.framing.write(kind, &payload, &mut frames) {
                        // Core is responsible for not sending oversized messages, so this is a bug or a
                        // wrong frame size, not something the device did
                        core.handle(Instant::now(), Input::TransportError(e.to_string()));
                    }
                    l.pending_tx.extend(frames.drain(..));
                }
                Output::Flush => {
                    if let Some(l) = link.as_mut() {
                        l.framing.flush(&mut frames);
                        l.pending_tx.extend(frames.drain(..));
                    }
                }
                Output::Exit(result) => {
                    exit = Some(result);
                    break;
                }
            }
        }
        if let Some(result) = exit {
            if let Some(mut l) = link.take() {
                l.framing.flush(&mut frames);
                l.pending_tx.extend(frames.drain(..));
                if tokio::time::timeout(EXIT_FLUSH_TIMEOUT, flush_pending(&mut l))
                    .await
                    .is_err()
                {
                    warn!("device did not accept the last frames on exit");
                }
                // give the last (Disconnect) transfer a chance to actually go out before endpoints are dropped
                tokio::time::sleep(Duration::from_millis(3)).await;
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

        let step = match link.as_mut() {
            Some(l) => {
                let has_tx = !l.pending_tx.is_empty();
                let accept_cmds = l.pending_tx.len() < TX_HIGH_WATERMARK;
                tokio::select! {
                    // rx first: it must never starve
                    biased;
                    r = l.rx.read_packet(&mut rx_buf) => Step::Rx(r),
                    r = l.tx.wait_ready(), if has_tx => Step::TxReady(r),
                    _ = timer => Step::Timer,
                    cmd = cmd_rx.recv(), if accept_cmds => Step::Cmd(cmd),
                }
            }
            None => {
                tokio::select! {
                    cmd = cmd_rx.recv() => Step::Cmd(cmd),
                    _ = timer => Step::Timer,
                }
            }
        };

        let now = Instant::now();
        match step {
            Step::Cmd(Some(cmd)) => core.handle(now, Input::Command(cmd)),
            Step::Cmd(None) => core.handle(now, Input::CommanderDropped),
            Step::Timer => core.handle(now, Input::Timer),
            Step::TxReady(Err(e)) => core.handle(now, Input::TransportError(e)),
            Step::TxReady(Ok(())) => {
                if let Some(l) = link.as_mut()
                    && let Some(frame) = l.pending_tx.pop_front()
                {
                    trace!("tx frame: {}: {frame:02x?}", frame.len());
                    if let Err(e) = l.tx.submit(&frame) {
                        core.handle(now, Input::TransportError(e));
                    }
                }
            }
            Step::Rx(Err(e)) => core.handle(now, Input::TransportError(e)),
            Step::Rx(Ok(len)) => {
                let frame = &rx_buf[..len];
                trace!("rx frame: {len}: {frame:02x?}");
                if let Some(l) = link.as_mut() {
                    if let Err(e) = l.framing.stage(frame) {
                        warn!("{e}, dropping frame");
                    }
                    while let Some((kind, payload)) = l.framing.next_message() {
                        core.handle(now, Input::Message { kind, payload });
                    }
                }
            }
        }
    };
    drop(link);

    let (exited_tx, residual) = core.into_residual(cmd_rx, result);
    if let Some(tx) = exited_tx {
        _ = tx.send(residual);
    }
}

async fn flush_pending<Tx: TxEndpoint, Rx>(l: &mut Link<Tx, Rx>) {
    while let Some(frame) = l.pending_tx.pop_front() {
        if l.tx.wait_ready().await.is_err() {
            return;
        }
        trace!("tx frame: {}: {frame:02x?}", frame.len());
        if l.tx.submit(&frame).is_err() {
            return;
        }
    }
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
    async fn wait_ready(&mut self) -> Result<(), String> {
        self.sink
            .wait_ready()
            .await
            .map_err(describe_transfer_error)
    }

    fn submit(&mut self, frame: &[u8]) -> Result<(), String> {
        self.tracer.tx(frame);
        self.sink.submit(frame).map_err(describe_transfer_error)
    }
}

impl RxEndpoint for NusbRx {
    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let len = self
            .source
            .read_packet(buf)
            .await
            .map_err(describe_transfer_error)?;
        self.tracer.rx(&buf[..len]);
        Ok(len)
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
    //! To see the failure: make the host await `wait_ready` inside the `Output::Send` arm (as the old
    //! loop did) and remove `biased` from the select — the test then hangs deterministically.

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

    struct MockTx {
        ch: mpsc::Sender<Vec<u8>>,
        permit: Option<mpsc::OwnedPermit<Vec<u8>>>,
    }

    impl TxEndpoint for MockTx {
        async fn wait_ready(&mut self) -> Result<(), String> {
            if self.permit.is_none() {
                let p = self
                    .ch
                    .clone()
                    .reserve_owned()
                    .await
                    .map_err(|_| "closed".to_string())?;
                self.permit = Some(p);
            }
            Ok(())
        }
        fn submit(&mut self, frame: &[u8]) -> Result<(), String> {
            self.permit
                .take()
                .expect("wait_ready first")
                .send(frame.to_vec());
            Ok(())
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
                    tx: MockTx {
                        ch: host_tx,
                        permit: None,
                    },
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
        worker.await.unwrap();
    }
}
