//! Async USB wrapper around the sans-IO [TxCore] / [RxCore]: two tasks, one per direction, each
//! owning its endpoint and framer. Rx never waits on tx, so a half-duplex device cannot deadlock the host;
//! backpressure to the [Commander](crate::Commander) is simply the tx task being blocked in a write.

use nusb::descriptors::TransferType;
use nusb::transfer::TransferError;
use tokio::sync::mpsc;
use tracing::{debug, trace};

use super::ww_nusb::{Sink, Source};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::framing::{MessageRx, MessageTx, Opened, Transport};

pub async fn usb_worker(cmd_rx: mpsc::Receiver<Command>) {
    debug!("usb worker started");
    crate::event_loop::core::worker(cmd_rx, NusbConnection).await;
    debug!("usb worker exited");
}

struct NusbConnection;

pub(crate) struct NusbTx {
    framer: ww_framer::TxOwned<ww_link::UsbHead, ww_link::UsbChecksum, ww_link::UsbTail>,
    sink: Sink,
    /// Keeps the device open while endpoints are in use
    _device: nusb::Device,
}

pub(crate) struct NusbRx {
    source: Source,
    framer: ww_framer::FramedRxOwned<ww_link::UsbHead, ww_link::UsbChecksum, ww_link::UsbTail>,
}

impl Transport for NusbConnection {
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
                framer: ww_framer::TxOwned::new(dev.max_packet_size),
                sink,
                _device: dev.device,
            },
            rx: NusbRx {
                source,
                framer: ww_framer::FramedRxOwned::new(
                    dev.max_packet_size + crate::DEFAULT_MAX_MESSAGE_SIZE,
                ),
            },
        })
    }
}

impl MessageTx for NusbTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        loop {
            match self.framer.write(kind, message) {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    let len = self.framer.flush();
                    if len == 0 {
                        return Err("framing error: cannot progress".into());
                    }
                    let frame = &self.framer.buf()[..len];
                    self.sink
                        .write_packet(frame)
                        .await
                        .map_err(describe_transfer_error)?;
                }
                Err(()) => {
                    return Err(format!("framing error: message too big({})", message.len()));
                }
            }
        }
    }

    async fn flush(&mut self) -> Result<(), String> {
        let len = self.framer.flush();
        if len == 0 {
            return Ok(());
        }
        let frame = &self.framer.buf()[..len];
        self.sink
            .write_packet(frame)
            .await
            .map_err(describe_transfer_error)?;
        Ok(())
    }
}

impl MessageRx for NusbRx {
    async fn read_message(&mut self, message: &mut [u8]) -> Result<(u8, usize), String> {
        if let Some((k, m)) = self.framer.message() {
            let len = m.len();
            message[..len].copy_from_slice(m);
            return Ok((k, len));
        }
        loop {
            let mut stage_err = false;
            self.source
                .read_packet_with(|packet| {
                    if let Err(()) = self.framer.stage(packet) {
                        stage_err = true;
                    }
                })
                .await
                .map_err(describe_transfer_error)?;
            if stage_err {
                return Err("out of staging area, this is a bug".into());
            }

            self.framer.reassemble();
            match self.framer.message() {
                Some((k, m)) => {
                    // got at least one message, others will be returned at the beginning of this function on next call
                    let len = m.len();
                    message[..len].copy_from_slice(m);
                    return Ok((k, len));
                }
                None => {
                    // need more frames to re-assemble next message
                    continue;
                }
            }
        }
    }
}

fn describe_transfer_error(e: TransferError) -> String {
    format!("USB transfer error: {e:?}")
}

#[cfg(test)]
mod tests {
    // //! Regression test for a deadlock between a half-duplex device and a half-duplex host:
    // //! thousands of requests fired at once used to stall the loop until the write timeout hit.
    // //!
    // //! The mock device below is deliberately half-duplex (does not read while blocked writing),
    // //! rx/tx channel depths mimic nusb queue sizes. Nothing here is timing dependent.
    // //!
    // //! To see the failure: run rx and tx on one task (e.g., `select!` over cmd_rx and read_packet with
    // //! writes awaited inline, as the old loop did) — the test then hangs deterministically.

    // use super::*;
    // use crate::device_info::ConnectionInfo;
    // use crate::event_loop::framing::Framing;
    // use tokio::sync::oneshot;
    // use ww_link::{DeviceInfo, Message};
    // use ww_version::{
    //     ApiHashPair, CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version,
    //     VersionOwned,
    // };

    // const PKT: usize = 64;
    // /// Host-side tx transfer slots (nusb TX_QUEUE_SIZE)
    // const HOST_TX_DEPTH: usize = 4;
    // /// Host-side rx transfer slots (nusb RX_QUEUE_SIZE)
    // const HOST_RX_DEPTH: usize = 64;
    // /// Each reply spans ~10 packets, so one host packet holding ~7 requests makes the device emit ~70 packets,
    // /// more than the host has rx slots for. That is what closes the deadlock cycle when both sides are half-duplex.
    // /// Same thing happens with tiny replies and 512 B USB packets (~80 requests per packet).
    // const REPLY_LEN: usize = 600;

    // struct MockTx(mpsc::Sender<Vec<u8>>);

    // impl FrameSink for MockTx {
    //     async fn write_packet(&mut self, frame: &[u8]) -> Result<(), String> {
    //         self.0
    //             .send(frame.to_vec())
    //             .await
    //             .map_err(|_| "closed".to_string())
    //     }
    // }

    // struct MockRx(mpsc::Receiver<Vec<u8>>);

    // impl FrameSource for MockRx {
    //     async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, String> {
    //         let f = self.0.recv().await.ok_or("closed")?;
    //         buf[..f.len()].copy_from_slice(&f);
    //         Ok(f.len())
    //     }
    // }

    // struct MockConnector {
    //     opened: Option<Opened<MockTx, MockRx>>,
    // }

    // impl Connector for MockConnector {
    //     type Tx = MockTx;
    //     type Rx = MockRx;
    //     fn connect(&mut self, _: DeviceHandle) -> Result<Opened<MockTx, MockRx>, String> {
    //         self.opened.take().ok_or("already connected".into())
    //     }
    // }

    // /// Half-duplex device: read a packet, decode, answer every request with one Value event,
    // /// awaiting each write before reading the next packet.
    // async fn half_duplex_device(
    //     mut from_host: mpsc::Receiver<Vec<u8>>,
    //     to_host: mpsc::Sender<Vec<u8>>,
    // ) {
    //     let mut framing: Framing<ww_link::UsbHead, ww_link::UsbChecksum, ww_link::UsbTail> =
    //         Framing::new(PKT, 1024);
    //     let mut frames = vec![];
    //     let mut scratch = [0u8; 256];
    //     let mut send =
    //         |msg: &Message<'_>, framing: &mut Framing<_, _, _>, frames: &mut Vec<Vec<u8>>| {
    //             let (k, p) = msg.encode(&mut scratch).unwrap();
    //             framing.write(k, p, frames).unwrap();
    //             framing.flush(frames);
    //         };
    //     while let Some(pkt) = from_host.recv().await {
    //         framing.stage(&pkt).unwrap();
    //         while let Some((kind, payload)) = framing.next_message() {
    //             match Message::decode(kind, payload).unwrap() {
    //                 Message::GetDeviceInfo => send(
    //                     &Message::DeviceInfo(DeviceInfo {
    //                         dev_link_version: CompactVersion::new(GlobalTypeId::new(512), 0, 1, 0),
    //                         api_model_version: CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
    //                         user_api_version: FullVersion::new("test", Version::new(0, 1, 0)),
    //                         hash: ApiHashPair::empty(),
    //                         dev_max_message_len: 1024,
    //                         packet_accumulation_time_us: 100,
    //                     }),
    //                     &mut framing,
    //                     &mut frames,
    //                 ),
    //                 Message::LinkSetup(_) => send(&Message::LinkReady, &mut framing, &mut frames),
    //                 Message::Data { bytes, .. } => {
    //                     let seq = u16::from_le_bytes([bytes[0], bytes[1]]);
    //                     let mut value = bytes[2..].to_vec();
    //                     value.resize(REPLY_LEN, 0xEE);
    //                     let event = ww_client_server::Event {
    //                         seq,
    //                         result: Ok(ww_client_server::EventKind::Value {
    //                             data: wire_weaver::shrink_wrap::tail_bytes::TailBytes(&value),
    //                         }),
    //                     };
    //                     let mut buf = [0u8; 1024];
    //                     let bytes = wire_weaver::shrink_wrap::SerializeShrinkWrap::to_ww_bytes(
    //                         &event, &mut buf,
    //                     )
    //                     .unwrap();
    //                     // one frame per response: worst case for host rx
    //                     send(
    //                         &Message::Data { channel: 0, bytes },
    //                         &mut framing,
    //                         &mut frames,
    //                     );
    //                 }
    //                 Message::Disconnect(_) => return,
    //                 _ => {}
    //             }
    //         }
    //         // half-duplex: all responses are written out (each awaited) before reading again
    //         for f in frames.drain(..) {
    //             if to_host.send(f).await.is_err() {
    //                 return;
    //             }
    //         }
    //     }
    // }

    // #[tokio::test]
    // async fn thousands_of_requests_do_not_deadlock() {
    //     const N: usize = 2000;
    //     let (host_tx, dev_rx) = mpsc::channel::<Vec<u8>>(HOST_TX_DEPTH);
    //     let (dev_tx, host_rx) = mpsc::channel::<Vec<u8>>(HOST_RX_DEPTH);
    //     let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

    //     tokio::spawn(half_duplex_device(dev_rx, dev_tx));
    //     let worker = tokio::spawn(worker(
    //         cmd_rx,
    //         MockConnector {
    //             opened: Some(Opened {
    //                 tx: MockTx(host_tx),
    //                 rx: MockRx(host_rx),
    //                 max_packet_size: PKT,
    //             }),
    //         },
    //     ));

    //     let (connected_tx, connected_rx) = oneshot::channel::<ConnectionInfo>();
    //     cmd_tx
    //         .send(Command::Connect {
    //             handle: Box::new(()),
    //             client_version: Box::new(FullVersionOwned::new(
    //                 "test".into(),
    //                 VersionOwned::new(0, 1, 0),
    //             )),
    //             connected_tx: Some(connected_tx),
    //             failed_tx: None,
    //         })
    //         .await
    //         .unwrap();
    //     connected_rx.await.unwrap().result.unwrap();

    //     // fire everything at once, exactly like a user looping over Commander::send_call
    //     let mut done = Vec::with_capacity(N);
    //     for i in 0..N as u32 {
    //         let (done_tx, done_rx) = oneshot::channel();
    //         cmd_tx
    //             .send(Command::SendMessage {
    //                 bytes: [0u8, 0]
    //                     .iter()
    //                     .chain(i.to_le_bytes().iter())
    //                     .copied()
    //                     .collect(),
    //                 done_tx: Some((done_tx, Duration::from_secs(5))),
    //             })
    //             .await
    //             .unwrap();
    //         done.push((i, done_rx));
    //     }
    //     for (i, rx) in done {
    //         let bytes = tokio::time::timeout(Duration::from_secs(30), rx)
    //             .await
    //             .expect("stalled")
    //             .unwrap()
    //             .unwrap_or_else(|e| panic!("request {i} failed: {e:?}"));
    //         assert_eq!(&bytes[..4], i.to_le_bytes());
    //         assert_eq!(bytes.len(), REPLY_LEN);
    //     }

    //     let (disconnected_tx, disconnected_rx) = oneshot::channel();
    //     cmd_tx
    //         .send(Command::DisconnectAndExit {
    //             disconnected_tx: Some(disconnected_tx),
    //             reason: ww_link::DisconnectReason::RequestByUser,
    //         })
    //         .await
    //         .unwrap();
    //     disconnected_rx.await.unwrap();
    //     tokio::time::timeout(Duration::from_secs(5), worker)
    //         .await
    //         .expect("worker did not exit")
    //         .unwrap();
    // }
}
