use std::net::SocketAddr;
use futures::channel::mpsc::{Receiver, Sender};
use futures::{FutureExt, SinkExt, StreamExt};
use futures::channel::mpsc;
use futures::stream::{SplitSink, SplitStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::WriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::Framed;
use tracing::{error, info, instrument, trace, warn};
use vhl_stdlib::serdes::{NibbleBuf, NibbleBufMut};
use xpi::owned::{Event, NodeId};
use xpi::xwfd;
use crate::codec::mvlb_crc32_codec::MvlbCrc32Codec;
use crate::node::addressing::RemoteNodeAddr;
use crate::node::async_std::internal_event::InternalEvent;
use crate::remote::remote_descriptor::RemoteDescriptor;

const TX_BBBUFFER_MTU: usize = 1024;
const TX_BBBUFFER_LEN: usize = 10_240;

#[instrument(skip(listener, tx_to_event_loop, tx_internal))]
pub(crate) async fn tcp_server_acceptor(
    self_id: NodeId,
    listener: TcpListener,
    tx_to_event_loop: Sender<Event>,
    mut tx_internal: Sender<InternalEvent>,
) {
    loop {
        match listener.accept().await {
            Ok((tcp_stream, remote_addr)) => {
                info!("Got new connection from: {remote_addr}");
                let (tx, rx) = mpsc::channel(64);
                let to_event_loop = tx_to_event_loop.clone();
                let to_event_loop_internal = tx_internal.clone();
                // tokio::spawn(async move {
                //     tcp_event_loop(self_id, remote_addr, tcp_stream, to_event_loop.clone(), to_event_loop_internal, rx).await
                // });
                todo!();
                let remote_descriptor = RemoteDescriptor {
                    reachable: vec![],
                    addr: RemoteNodeAddr::Tcp(remote_addr),
                    to_event_loop: tx,
                };
                match tx_internal
                    .send(InternalEvent::ConnectRemoteTcp(remote_descriptor))
                    .await {
                    Ok(_) => {}
                    Err(_) => error!("tx_internal: send failed")
                }
            }
            Err(e) => {
                warn!("{e:?}");
            }
        }
    }
}

#[instrument(skip(to_event_loop, to_event_loop_internal, from_event_loop))]
pub async fn tcp_event_loop(
    _self_id: NodeId,
    addr: SocketAddr,
    mut frames_sink: SplitSink<Framed<TcpStream, MvlbCrc32Codec>, Vec<u8>>,
    mut frames_source: SplitStream<Framed<TcpStream, MvlbCrc32Codec>>,
    mut to_event_loop: Sender<Event>,
    mut to_event_loop_internal: Sender<InternalEvent>,
    mut from_event_loop: Receiver<Event>,
) {
    info!("Entering tcp event loop on {addr}");
    let mut frames_source = frames_source.fuse();

    loop {
        futures::select! {
            frame = frames_source.next() => {
                match frame {
                    Some(Ok(frame)) => {
                        process_incoming_slice(frame, &mut to_event_loop).await;
                    }
                    Some(Err(e)) => {
                       error!("Decoder from tcp error: {:?}", e);
                    }
                    None => {

                    }
                }
            }
            // read_result = tcp_rx.read(&mut buf).fuse() => {
            //     match read_result {
            //         Ok(len) => if len > 0 {
            //             process_incoming_slice(&buf[..len], &mut to_event_loop).await;
            //         },
            //         Err(e) => {
            //             error!("Failed to read from tcp {:?}", e);
            //              match to_event_loop_internal
            //                 .send(InternalEvent::DropRemoteTcp(addr))
            //                 .await {
            //                     Ok(_) => {}
            //                     Err(_) => error!("tx_internal: send failed")
            //                 }
            //         },
            //     }
            // }
            ev = from_event_loop.select_next_some() => {
                // serialize_and_commit(ev, &mut tx_prod);
                // consume_and_write(&mut tx_cons, &mut tcp_tx).await;
                serialize_and_send(ev, &mut frames_sink).await;
            },
        }
    }
}

async fn process_incoming_slice(bytes: Vec<u8>, to_event_loop: &mut Sender<Event>) {
    trace!("rx: {} bytes: {:2x?}", bytes.len(), bytes);
    let mut nrd = NibbleBuf::new_all(&bytes);
    let ev: Result<xwfd::Event, _> = nrd.des_vlu4();
    match ev {
        Ok(ev) => {
            trace!("rx {}B: {}", bytes.len(), ev);
            let ev_owned: Event = ev.into();
            if to_event_loop.send(ev_owned).await.is_err() {
                error!("mpsc fail");
            }
        }
        Err(e) => {
            error!("xwfd deserialize error: {:?} bytes: {:02x?}", e, bytes);
        }
    }
}

async fn serialize_and_send(
    ev: Event,
    frames_sink: &mut SplitSink<Framed<TcpStream, MvlbCrc32Codec>, Vec<u8>>,
) {
    let mut buf = Vec::new();
    buf.resize(10_000, 0);
    let mut nwr = NibbleBufMut::new_all(&mut buf);
    match ev.ser_xwfd(&mut nwr) {
        Ok(()) => {
            let (_, len, _) = nwr.finish();
            trace!("serialize_and_send: ser_xwfd ok, len: {:?}", len);
            buf.resize(len, 0);
            match frames_sink.send(buf).await {
                Ok(_) => {}
                Err(e) => error!("Encoder for tcp error: {e:?}")
            }
        }
        Err(e) => {
            error!("convert to xwfd failed: {:?}", e);
        }
    }
}
//
// fn serialize_and_commit<'tx>(ev: Event, tx_prod: &mut Producer<TX_BBBUFFER_LEN>) {
//     match tx_prod.grant_max_remaining(TX_BBBUFFER_MTU) {
//         Ok(mut wgr) => {
//             let mut nwr = NibbleBufMut::new_all(&mut wgr);
//             match ev.ser_xwfd(&mut nwr) {
//                 Ok(()) => {
//                     let (_, len, _) = nwr.finish();
//                     trace!("serialize_and_commit: ser_xwfd ok, len: {:?}", len);
//                     wgr.commit(len);
//                 }
//                 Err(e) => {
//                     error!("convert to xwfd failed: {:?}", e);
//                 }
//             }
//         }
//         Err(e) => {
//             error!("BBBuffer error: {e:?}");
//         }
//     }
// }
//
// async fn consume_and_write<'a, 'b>(tx_cons: &mut Consumer<'a, TX_BBBUFFER_LEN>, tcp_tx: &mut WriteHalf<'b>) {
//     match tx_cons.read() {
//         Ok(rgr) => {
//             match tcp_tx.write(&rgr).await {
//                 Ok(actually_written) => {
//                     rgr.release(actually_written);
//                 }
//                 Err(e) => {
//                     error!("consume_and_write: {e:?}");
//                 }
//             }
//         }
//         Err(e) => {
//             error!("consume_and_write: {e:?}");
//         }
//     }
//
// }