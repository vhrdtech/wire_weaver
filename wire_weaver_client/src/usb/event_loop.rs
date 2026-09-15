//! USB [Transport]: nusb endpoints + [ww_framer] with the USB framer configuration from [ww_link].
//! Tx and rx halves are used from two independent tasks by the generic event loop,
//! see [core](crate::event_loop::core).

use nusb::descriptors::TransferType;
use nusb::transfer::TransferError;
use tokio::sync::mpsc;
use tracing::{debug, trace};

use super::ww_nusb::{Sink, Source};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::transport::{MessageRx, MessageTx, Opened, Transport};

type TxFramer = ww_framer::TxOwned<ww_link::UsbHead, ww_link::UsbChecksum, ww_link::UsbTail>;
type RxFramer = ww_framer::FramedRxOwned<ww_link::UsbHead, ww_link::UsbChecksum, ww_link::UsbTail>;

pub async fn usb_worker(cmd_rx: mpsc::Receiver<Command>) {
    debug!("usb worker started");
    crate::event_loop::core::worker(cmd_rx, NusbTransport).await;
    debug!("usb worker exited");
}

struct NusbTransport;

pub(crate) struct NusbTx {
    framer: TxFramer,
    sink: Sink,
    /// Keeps the device open while endpoints are in use
    _device: nusb::Device,
}

pub(crate) struct NusbRx {
    framer: RxFramer,
    source: Source,
}

impl Transport for NusbTransport {
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
        Ok(Opened {
            tx: NusbTx {
                framer: TxFramer::new(dev.max_packet_size),
                sink,
                _device: dev.device,
            },
            rx: NusbRx {
                // must hold one maximum message plus one more packet, see FramedRx docs
                framer: RxFramer::new(crate::DEFAULT_MAX_MESSAGE_SIZE + dev.max_packet_size),
                source,
            },
        })
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
        self.sink
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
            let mut stage_err = false;
            let framer = &mut self.framer;
            self.source
                .read_packet_with(|packet| {
                    trace!("rx frame: {}: {packet:02x?}", packet.len());
                    stage_err = framer.stage(packet).is_err();
                })
                .await
                .map_err(describe_transfer_error)?;
            if stage_err {
                return Err("out of staging area, this is a bug".into());
            }
        }
        // second lookup instead of returning from inside the loop: keeps the borrow checker happy
        Ok(self.framer.message().expect("checked above"))
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

    struct MockTx(mpsc::Sender<Msg>);

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
    }

    struct MockRx {
        ch: mpsc::Receiver<Msg>,
        last: Msg,
    }

    impl MessageRx for MockRx {
        async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
            self.last = self.ch.recv().await.ok_or("closed")?;
            Ok((self.last.0, &self.last.1))
        }
    }

    struct MockTransport(Option<Opened<MockTx, MockRx>>);

    impl Transport for MockTransport {
        type Tx = MockTx;
        type Rx = MockRx;
        fn connect(&mut self, _: DeviceHandle) -> Result<Opened<MockTx, MockRx>, String> {
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
                Message::GetDeviceInfo => Some(Message::DeviceInfo(DeviceInfo {
                    dev_link_version: CompactVersion::new(GlobalTypeId::new(512), 0, 1, 0),
                    api_model_version: CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
                    user_api_version: FullVersion::new("test", Version::new(0, 1, 0)),
                    hash: ApiHashPair::empty(),
                    dev_max_message_len: 1024,
                    packet_accumulation_time_us: 100,
                })),
                Message::LinkSetup(_) => Some(Message::LinkReady),
                Message::Disconnect(_) => return,
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
        let mut tx = TxFramer::new(64);
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

        let mut rx = RxFramer::new(crate::DEFAULT_MAX_MESSAGE_SIZE + 64);
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

    #[tokio::test]
    async fn thousands_of_requests_do_not_deadlock() {
        const N: usize = 2000;
        let (host_tx, dev_rx) = mpsc::channel::<Msg>(HOST_TX_DEPTH);
        let (dev_tx, host_rx) = mpsc::channel::<Msg>(HOST_RX_DEPTH);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        tokio::spawn(half_duplex_device(dev_rx, dev_tx));
        let worker = tokio::spawn(crate::event_loop::core::worker(
            cmd_rx,
            MockTransport(Some(Opened {
                tx: MockTx(host_tx),
                rx: MockRx {
                    ch: host_rx,
                    last: (0, vec![]),
                },
            })),
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

        // a second Connect on the same loop is refused, not silently dropped
        let (connected_tx, connected_rx) = oneshot::channel::<ConnectionInfo>();
        cmd_tx
            .send(Command::Connect {
                handle: Box::new(()),
                client_version: Box::new(FullVersionOwned::new(
                    "".into(),
                    VersionOwned::new(0, 0, 0),
                )),
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
}
