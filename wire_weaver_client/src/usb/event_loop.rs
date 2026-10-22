//! USB [Transport]: nusb endpoints + [ww_framer] with the USB framer configuration from [ww_link]: no checksum,
//! no frame prefix, or the `SeqPrefix` diagnostic when the interface protocol says so.
//! Tx and rx halves are used from two independent tasks by the generic event loop,
//! see [core](crate::event_loop::core).
//!
//! Live link handover (CLT-44, Linux): [Transport::pause] sends every OUT transfer and cancels and reaps the IN ones
//! into the framer and leaves a [UsbResidualOwned], [Transport::let_go] releases the interface keeping the device
//! open, [Transport::take_back] claims it again. Releasing an interface at alternate setting 0 sends nothing on the bus and keeps the endpoints'
//! data toggles, so the program that claims it next goes on where this one stopped.

use nusb::descriptors::TransferType;
use nusb::transfer::TransferError;
use tokio::sync::mpsc;
use tracing::{debug, trace};
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};

use super::ww_nusb::{Sink, Source};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
#[cfg(feature = "local_socket")]
use crate::event_loop::transport::Paused;
use crate::event_loop::transport::{
    MessageRx, MessageTx, Opened, SharedClaim, Transport, handover_requested,
};
use ww_framer::prefix::FrameReport;
use ww_tap::{Dir, RecorderLink, Tap};

use crate::framing::{UsbRx as RxFramer, UsbTx as TxFramer};

pub async fn usb_worker(cmd_rx: mpsc::Receiver<Command>) {
    debug!("usb worker started");
    crate::event_loop::core::worker(cmd_rx, NusbTransport).await;
    debug!("usb worker exited");
}

pub(crate) struct NusbTransport;

pub(crate) struct NusbTx {
    framer: TxFramer,
    /// None while paused for a live handover
    sink: Option<Sink>,
    tap: RecorderLink,
    /// Keeps the device open while endpoints are in use, and across a live handover's let go
    #[cfg_attr(not(feature = "local_socket"), allow(dead_code))]
    device: nusb::Device,
    /// None once let go of in a live handover
    #[cfg_attr(not(feature = "local_socket"), allow(dead_code))]
    interface: Option<nusb::Interface>,
    #[cfg_attr(not(feature = "local_socket"), allow(dead_code))]
    eps: Eps,
    /// The device's key in the hub, for a live handover
    #[cfg(feature = "local_socket")]
    hub_key: String,
    /// Keeps the holders registry claim (CLT-38) for as long as the interface is held
    claim: SharedClaim,
}

pub(crate) struct NusbRx {
    framer: RxFramer,
    /// None while paused for a live handover
    source: Option<Source>,
    tap: RecorderLink,
    /// Shared with [NusbTx]: released once both halves let go of the interface
    #[cfg_attr(not(feature = "local_socket"), allow(dead_code))]
    claim: SharedClaim,
}

/// The WireWeaver interface and its endpoints, to open them again.
#[derive(Copy, Clone)]
#[cfg_attr(not(feature = "local_socket"), allow(dead_code))]
struct Eps {
    interface: u8,
    ep_out: u8,
    ep_in: u8,
    max_packet: usize,
    bulk: bool,
    seq_prefix: bool,
}

/// How long the device has to take the OUT packets sent before a live handover's pause.
#[cfg(feature = "local_socket")]
const PAUSE_OUT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

type Tx = NusbTx;
type Rx = NusbRx;

/// What a USB link leaves for the medium that takes it over (CLT-44), the bytes of its
/// [MediumResidualOwned](crate::adopt::MediumResidualOwned): the interface it is on, and what the framers hold.
#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq, Eq))]
#[cfg_attr(not(feature = "local_socket"), allow(dead_code))]
pub(crate) struct UsbResidualOwned {
    pub interface: u8,
    pub ep_in: u8,
    pub ep_out: u8,
    pub max_packet: u16,
    pub bulk: bool,
    pub seq_prefix: bool,
    pub framers: crate::framing::FramerCarryOwned,
}

/// Largest USB packet.
const MAX_PACKET: u16 = 1024;

#[cfg_attr(not(feature = "local_socket"), allow(dead_code))]
impl UsbResidualOwned {
    const KIND: crate::adopt::MediumKindOwned = crate::adopt::MediumKindOwned::Usb;

    pub fn encode(&self) -> Result<crate::adopt::MediumResidualOwned, String> {
        Ok(crate::adopt::MediumResidualOwned {
            kind: Self::KIND,
            bytes: self.to_ww_bytes_owned().map_err(|e| format!("{e:?}"))?,
        })
    }

    /// Decodes the residual of a USB link and bounds its sizes.
    pub fn decode(medium: &crate::adopt::MediumResidualOwned) -> Result<Self, String> {
        let r = Self::from_ww_bytes_owned(medium.of(Self::KIND)?).map_err(|e| format!("{e:?}"))?;
        if !(8..=MAX_PACKET).contains(&r.max_packet) {
            return Err(format!("max packet {}", r.max_packet));
        }
        r.framers.check(r.max_packet as usize)?;
        Ok(r)
    }

    /// The link was on this interface, with these endpoints: the adopting medium opens the same.
    fn fits(&self, eps: Eps) -> Result<(), String> {
        let theirs = (
            self.interface,
            self.ep_in,
            self.ep_out,
            self.max_packet as usize,
            self.bulk,
            self.seq_prefix,
        );
        let ours = (
            eps.interface,
            eps.ep_in,
            eps.ep_out,
            eps.max_packet,
            eps.bulk,
            eps.seq_prefix,
        );
        if theirs == ours {
            Ok(())
        } else {
            Err(format!(
                "the adopted link was on another interface: {theirs:?}, opened {ours:?}"
            ))
        }
    }
}

/// Endpoints on the claimed interface, IN transfers submitted.
fn endpoints(interface: &nusb::Interface, eps: Eps) -> Result<(Sink, Source), String> {
    let sink =
        Sink::new(interface, eps.ep_out, eps.max_packet, eps.bulk).map_err(|e| e.to_string())?;
    let source =
        Source::new(interface, eps.ep_in, eps.max_packet, eps.bulk).map_err(|e| e.to_string())?;
    Ok((sink, source))
}

impl Transport for NusbTransport {
    fn link_setup_timeout(&self) -> std::time::Duration {
        crate::event_loop::transport::link_setup_timeout::USB
    }

    type Tx = Tx;
    type Rx = Rx;

    async fn connect(
        &mut self,
        handle: DeviceHandle,
        tap: ww_tap::RecorderLink,
    ) -> Result<Opened<Tx, Rx>, String> {
        // an adopted link (CLT-44): the framers go on where the other program's stopped
        let (handle, adopted) = match handle.downcast::<crate::event_loop::transport::Adopting>() {
            Ok(a) => (a.handle, Some(UsbResidualOwned::decode(&a.medium)?)),
            Err(handle) => (handle, None),
        };
        let target = handle
            .downcast::<super::connect::UsbTarget>()
            .map_err(|_| "expected a USB device handle".to_string())?;
        let dev = super::connect::connect(&target).map_err(|e| format!("{e:#}"))?;
        let claim = dev.claim.map(std::sync::Arc::new);
        let di = &target.info;
        trace!("max_packet_size: {}", dev.max_packet_size);
        let eps = Eps {
            interface: dev.interface_number,
            ep_out: dev.ep_out,
            ep_in: dev.ep_in,
            max_packet: dev.max_packet_size,
            bulk: dev.transfer_type == TransferType::Bulk,
            seq_prefix: dev.seq_prefix,
        };
        let mut tx_framer = TxFramer::new(dev.seq_prefix, dev.max_packet_size);
        // must hold one maximum message plus one more packet, see FramedRx docs
        let mut rx_framer = RxFramer::new(
            dev.seq_prefix,
            crate::DEFAULT_MAX_MESSAGE_SIZE + dev.max_packet_size,
        );
        if let Some(adopted) = adopted {
            adopted.fits(eps)?;
            adopted.framers.apply(&mut tx_framer, &mut rx_framer)?;
        }
        let (sink, source) = endpoints(&dev.interface, eps)?;
        let mut tap = tap;
        let peer = di.serial_number().unwrap_or_default();
        tap.session(&crate::tap::host_session(
            ww_tap::Medium::Usb,
            crate::framing::usb_tap_framer(dev.seq_prefix),
            dev.max_packet_size,
            peer,
        ));
        let tx = NusbTx {
            framer: tx_framer,
            sink: Some(sink),
            tap: tap.clone(),
            device: dev.device,
            interface: Some(dev.interface),
            eps,
            #[cfg(feature = "local_socket")]
            hub_key: super::connect::hub_key(&target.info),
            claim: claim.clone(),
        };
        let rx = NusbRx {
            framer: rx_framer,
            source: Some(source),
            tap,
            claim,
        };
        Ok(Opened { tx, rx })
    }

    // what releasing an interface does to the bus and the data toggles is known on Linux only
    #[cfg(feature = "local_socket")]
    const LIVE: bool = cfg!(target_os = "linux");

    #[cfg(feature = "local_socket")]
    async fn pause(tx: &mut Tx, rx: &mut Rx) -> Result<Paused, String> {
        // OUT: everything queued goes out and is taken by the device
        tx.send_frame().await?;
        let mut sink = tx.sink.take().ok_or("paused already")?;
        let drained = sink.drain(PAUSE_OUT_TIMEOUT).await;
        sink.close().await;
        // IN: cancelled; what the device sent this host is reaped, the rest waits in the device
        let source = rx.source.take().ok_or("paused already")?;
        // done with what the last read returned; messages already complete in the staged data come first
        let mut messages = vec![];
        rx.framer.take_complete(&mut messages);
        let framer = &mut rx.framer;
        let tap = &mut rx.tap;
        let mut staged = Ok(());
        let reaped = source
            .pause(|packet| {
                if tap.wants_frames() {
                    tap.frame(Dir::Rx, packet);
                }
                if staged.is_err() {
                    return;
                }
                match framer.stage(packet) {
                    Ok(report) => crate::framing::log_report(report),
                    Err(()) => staged = Err(()),
                }
                framer.take_complete(&mut messages);
            })
            .await;
        let eps = tx.eps;
        let medium = drained
            .and(reaped)
            .and(staged.map_err(|_| "out of staging area, this is a bug".to_string()))
            .and_then(|()| {
                UsbResidualOwned {
                    interface: eps.interface,
                    ep_in: eps.ep_in,
                    ep_out: eps.ep_out,
                    max_packet: eps.max_packet as u16,
                    bulk: eps.bulk,
                    seq_prefix: eps.seq_prefix,
                    framers: crate::framing::FramerCarryOwned::take(&mut tx.framer, &rx.framer),
                }
                .encode()
            });
        match medium {
            Ok(medium) => Ok(Paused {
                key: tx.hub_key.clone(),
                medium,
                messages,
            }),
            Err(e) => {
                // the link goes on as well as it can: endpoints again, whatever ends it ends it
                if let Some(interface) = &tx.interface
                    && let Ok((sink, source)) = endpoints(interface, tx.eps)
                {
                    tx.sink = Some(sink);
                    rx.source = Some(source);
                }
                Err(e)
            }
        }
    }

    #[cfg(feature = "local_socket")]
    async fn let_go(tx: &mut Tx, _rx: &mut Rx) -> Result<(), String> {
        // the endpoints went in the pause: this releases the interface, the device stays open
        tx.interface = None;
        Ok(())
    }

    #[cfg(feature = "local_socket")]
    async fn take_back(tx: &mut Tx, rx: &mut Rx) -> Result<(), String> {
        let interface = match tx.interface.take() {
            Some(interface) => interface,
            None => tx
                .device
                .claim_interface(tx.eps.interface)
                .await
                .map_err(|e| format!("claiming the interface again: {e}"))?,
        };
        let (sink, source) = endpoints(&interface, tx.eps)?;
        tx.interface = Some(interface);
        tx.sink = Some(sink);
        rx.source = Some(source);
        Ok(())
    }

    #[cfg(feature = "local_socket")]
    fn switch(tx: &mut Tx, _rx: &mut Rx) -> Result<(), String> {
        // the device file goes with the halves; the interface must be let go of, or the next medium can't claim it
        match tx.interface {
            None => Ok(()),
            Some(_) => Err("the interface is still held".into()),
        }
    }

    #[cfg(feature = "local_socket")]
    fn take_claim(tx: &mut Tx, rx: &mut Rx) -> SharedClaim {
        let rx_claim = rx.claim.take();
        tx.claim.take().or(rx_claim)
    }

    #[cfg(feature = "local_socket")]
    fn claim_of(tx: &Tx) -> SharedClaim {
        tx.claim.clone()
    }

    #[cfg(feature = "local_socket")]
    fn put_claim(tx: &mut Tx, rx: &mut Rx, claim: SharedClaim) {
        tx.claim = claim.clone();
        rx.claim = claim;
    }
}

impl NusbTx {
    async fn send_frame(&mut self) -> Result<bool, String> {
        let len = self.framer.flush();
        if len == 0 {
            return Ok(false);
        }
        let frame = &self.framer.buf()[..len];
        trace!("tx frame: {len}: {frame:02x?}");
        if self.tap.wants_frames() {
            self.tap.frame(Dir::Tx, frame);
        }
        self.sink
            .as_mut()
            .ok_or("paused for a live handover")?
            .write_packet(frame)
            .await
            .map_err(describe_transfer_error)?;
        Ok(true)
    }
}

impl MessageTx for NusbTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        loop {
            match self.framer.write(kind, message) {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    // frame is full (or the message continues into the next one): send and retry
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

    async fn handover_requested(&mut self) {
        handover_requested(&self.claim).await
    }

    async fn close(self) {
        if let Some(sink) = self.sink {
            sink.close().await;
        }
    }
}

impl MessageRx for NusbRx {
    /// Cancel-safe: staged packets live in the framer, the only await is the packet read.
    /// Several messages per packet are returned one per call.
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        loop {
            // Consumes the message returned by the previous call (if any) and tries to assemble
            // the next one from what is already staged, before reading more.
            self.framer.reassemble();
            if self.framer.message().is_some() {
                break;
            }
            let mut staged = Ok(FrameReport::Unchecked);
            let framer = &mut self.framer;
            let tap = &mut self.tap;
            self.source
                .as_mut()
                .ok_or("paused for a live handover")?
                .read_packet_with(|packet| {
                    trace!("rx frame: {}: {packet:02x?}", packet.len());
                    if tap.wants_frames() {
                        tap.frame(Dir::Rx, packet);
                    }
                    staged = framer.stage(packet);
                })
                .await
                .map_err(describe_transfer_error)?;
            match staged {
                Ok(report) => crate::framing::log_report(report),
                Err(()) => return Err("out of staging area, this is a bug".into()),
            }
        }
        // second lookup instead of returning from inside the loop: keeps the borrow checker happy
        Ok(self.framer.message().expect("checked above"))
    }

    async fn close(self) {
        if let Some(source) = self.source {
            source.close().await;
        }
    }
}

fn describe_transfer_error(e: TransferError) -> String {
    format!("USB transfer error: {e:?}")
}

#[cfg(test)]
mod tests {
    //! Regression test for a deadlock between a half-duplex device and a half-duplex host: thousands of
    //! requests fired at once used to stall the loop until the write timeout hit. The mock device below is
    //! deliberately half-duplex (does not read while blocked writing) and channel depths mimic nusb queue
    //! sizes. Nothing here is timing dependent. With a single-task host that awaits writes inline this
    //! hangs deterministically.
    //!
    //! The transport is mocked at the message level, so the generic loop is exercised while USB framing is
    //! covered separately below.

    use super::*;
    use crate::device_info::ConnectionInfo;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::oneshot;
    use ww_link::{DeviceInfo, Message};
    use ww_version::{
        ApiHashPair, CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version,
        VersionOwned,
    };

    /// Host-side tx transfer slots (nusb TX_QUEUE_SIZE)
    const HOST_TX_DEPTH: usize = 4;
    /// Host-side rx transfer slots (nusb RX_QUEUE_SIZE)
    const HOST_RX_DEPTH: usize = 64;
    const REPLY_LEN: usize = 600;

    type Msg = (u8, Vec<u8>);

    /// Counts closed transport halves, each close takes a while, like releasing a real USB interface
    #[derive(Clone, Default)]
    struct Closed(Arc<AtomicUsize>);

    impl Closed {
        async fn close(&self) {
            tokio::time::sleep(Duration::from_millis(20)).await;
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    crate::event_loop::medium::other_medium!(tx: MockTx; rx: MockRx;);

    struct MockTx(mpsc::Sender<Msg>, Closed);

    impl MessageTx for MockTx {
        async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
            self.0
                .send((kind, message.to_vec()))
                .await
                .map_err(|_| "closed".to_string())
        }
        async fn flush(&mut self) -> Result<(), String> {
            Ok(())
        }
        async fn close(self) {
            self.1.close().await;
        }
    }

    struct MockRx {
        ch: mpsc::Receiver<Msg>,
        last: Msg,
        closed: Closed,
    }

    impl MessageRx for MockRx {
        async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
            self.last = self.ch.recv().await.ok_or("closed")?;
            Ok((self.last.0, &self.last.1))
        }
        async fn close(self) {
            self.closed.close().await;
        }
    }

    struct MockTransport(Option<Opened<MockTx, MockRx>>);

    impl Transport for MockTransport {
        fn link_setup_timeout(&self) -> std::time::Duration {
            crate::event_loop::transport::link_setup_timeout::IN_PROCESS
        }

        type Tx = MockTx;
        type Rx = MockRx;
        async fn connect(
            &mut self,
            _: DeviceHandle,
            _: ww_tap::RecorderLink,
        ) -> Result<Opened<MockTx, MockRx>, String> {
            self.0.take().ok_or("already connected".into())
        }
    }

    /// Half-duplex device: read a message, answer every request with one Value event,
    /// awaiting each write before reading again.
    async fn half_duplex_device(mut from_host: mpsc::Receiver<Msg>, to_host: mpsc::Sender<Msg>) {
        let mut replies: Vec<Msg> = vec![];
        let mut scratch = [0u8; 1024];
        while let Some((kind, payload)) = from_host.recv().await {
            let reply = match Message::decode(kind, &payload).unwrap() {
                Message::GetDeviceInfo { token } => Some(Message::DeviceInfo(DeviceInfo {
                    dev_link_version: ww_link::LINK_VERSION,
                    api_model_version: CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
                    user_api_version: FullVersion::new("test", Version::new(0, 1, 0)),
                    hash: ApiHashPair::empty(),
                    dev_max_message_len: 1024,
                    packet_accumulation_time_us: 100,
                    token,
                })),
                Message::LinkSetup(setup) => Some(Message::LinkReady { token: setup.token }),
                Message::Disconnect(_) => return,
                Message::Data { bytes, .. } => {
                    let mut rd = wire_weaver::shrink_wrap::BufReader::new(bytes);
                    let seq = wire_weaver::shrink_wrap::UVlq32(rd.read_uvlq32().unwrap());
                    let mut value = bytes[bytes.len() - rd.bytes_left()..].to_vec();
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
                    let (k, p) = Message::Data { channel: 0, bytes }
                        .encode(&mut scratch)
                        .unwrap();
                    replies.push((k, p.to_vec()));
                    None
                }
                _ => None,
            };
            if let Some(m) = reply {
                let (k, p) = m.encode(&mut scratch).unwrap();
                replies.push((k, p.to_vec()));
            }
            // half-duplex: all replies are written out (each awaited) before reading again
            for r in replies.drain(..) {
                if to_host.send(r).await.is_err() {
                    return;
                }
            }
        }
    }

    /// Same reassemble-before-read pattern as [NusbRx::read_message], over an in-memory packet queue.
    /// Guards against the bug where only the first message of a packet was ever returned.
    #[test]
    fn several_messages_per_packet_are_all_delivered() {
        let mut tx = TxFramer::new(false, 64);
        let mut packets: Vec<Vec<u8>> = vec![];
        for (k, m) in [(0u8, &[1u8, 2, 3][..]), (1, &[4, 5]), (5, &[6])] {
            assert_eq!(tx.write(k, m), Ok(true));
        }
        let len = tx.flush();
        packets.push(tx.buf()[..len].to_vec());
        // and a message spanning two packets
        let big: Vec<u8> = (0..100).collect();
        loop {
            match tx.write(2, &big) {
                Ok(true) => break,
                Ok(false) => {
                    let len = tx.flush();
                    packets.push(tx.buf()[..len].to_vec());
                }
                Err(()) => panic!(),
            }
        }
        let len = tx.flush();
        packets.push(tx.buf()[..len].to_vec());

        let mut rx = RxFramer::new(false, crate::DEFAULT_MAX_MESSAGE_SIZE + 64);
        let mut packets = packets.into_iter();
        let mut read_message = |rx: &mut RxFramer| -> Option<(u8, Vec<u8>)> {
            loop {
                rx.reassemble();
                if rx.message().is_some() {
                    break;
                }
                rx.stage(&packets.next()?).unwrap();
            }
            rx.message().map(|(k, m)| (k, m.to_vec()))
        };
        assert_eq!(read_message(&mut rx), Some((0, vec![1, 2, 3])));
        assert_eq!(read_message(&mut rx), Some((1, vec![4, 5])));
        assert_eq!(read_message(&mut rx), Some((5, vec![6])));
        assert_eq!(read_message(&mut rx), Some((2, big)));
        assert_eq!(read_message(&mut rx), None);
    }

    /// Start the event loop with a mock device and connect to it
    async fn connected_worker(
        closed: Closed,
    ) -> (mpsc::Sender<Command>, tokio::task::JoinHandle<()>) {
        let (host_tx, dev_rx) = mpsc::channel::<Msg>(HOST_TX_DEPTH);
        let (dev_tx, host_rx) = mpsc::channel::<Msg>(HOST_RX_DEPTH);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        tokio::spawn(half_duplex_device(dev_rx, dev_tx));
        let worker = tokio::spawn(crate::event_loop::core::worker(
            cmd_rx,
            MockTransport(Some(Opened {
                tx: MockTx(host_tx, closed.clone()),
                rx: MockRx {
                    ch: host_rx,
                    last: (0, vec![]),
                    closed,
                },
            })),
        ));

        let (connected_tx, connected_rx) = oneshot::channel::<ConnectionInfo>();
        cmd_tx
            .send(Command::Connect {
                handle: Box::new(()),
                max_seq: crate::DEFAULT_MAX_SEQ,
                link_setup_timeout_ms: None,
                tap: None,
                client_meta: None,
                client_version: Box::new(FullVersionOwned::new(
                    "test".into(),
                    VersionOwned::new(0, 1, 0),
                )),
                keep_streams_on_loss: false,
                connected_tx: Some(connected_tx),
                failed_tx: None,
            })
            .await
            .unwrap();
        connected_rx.await.unwrap().result.unwrap();
        (cmd_tx, worker)
    }

    #[tokio::test]
    async fn thousands_of_requests_do_not_deadlock() {
        const N: usize = 2000;
        let (cmd_tx, worker) = connected_worker(Closed::default()).await;

        // fire everything at once, exactly like a user looping over Commander::send_call
        let mut done = Vec::with_capacity(N);
        for i in 0..N as u32 {
            let (done_tx, done_rx) = oneshot::channel();
            cmd_tx
                .send(Command::SendMessage {
                    bytes: [0u8; wire_weaver::shrink_wrap::UVlq32Backfill::LEN]
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

        // a second Connect on the same loop is refused, not silently dropped
        let (connected_tx, connected_rx) = oneshot::channel::<ConnectionInfo>();
        cmd_tx
            .send(Command::Connect {
                handle: Box::new(()),
                max_seq: crate::DEFAULT_MAX_SEQ,
                link_setup_timeout_ms: None,
                tap: None,
                client_meta: None,
                client_version: Box::new(FullVersionOwned::new(
                    "".into(),
                    VersionOwned::new(0, 0, 0),
                )),
                keep_streams_on_loss: false,
                connected_tx: Some(connected_tx),
                failed_tx: None,
            })
            .await
            .unwrap();
        assert!(connected_rx.await.unwrap().result.is_err());

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

    /// Users re-connect right after disconnect returns, it used to return before a USB interface was released,
    /// so claiming it again failed with "interface is busy".
    #[tokio::test]
    async fn disconnect_returns_after_transport_is_closed() {
        for keep_streams in [false, true] {
            let closed = Closed::default();
            let (cmd_tx, worker) = connected_worker(closed.clone()).await;
            let (disconnected_tx, disconnected_rx) = oneshot::channel();
            let reason = ww_link::DisconnectReason::RequestByUser;
            let disconnected_tx = Some(disconnected_tx);
            let cmd = if keep_streams {
                Command::DisconnectKeepStreams {
                    disconnected_tx,
                    reason,
                }
            } else {
                Command::DisconnectAndExit {
                    disconnected_tx,
                    reason,
                }
            };
            cmd_tx.send(cmd).await.unwrap();
            disconnected_rx.await.unwrap();
            assert_eq!(
                closed.0.load(Ordering::SeqCst),
                2,
                "tx and rx must be closed"
            );
            worker.await.unwrap();
        }
    }
}
