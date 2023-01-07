use crate::codec::rmvlb_codec::RmvlbCodec;
use crate::node::addressing::RemoteNodeAddr;
use crate::node::async_std::internal_event::InternalEvent;
use crate::remote::remote_descriptor::RemoteDescriptor;
use futures::channel::mpsc;
use futures::channel::mpsc::{Receiver, Sender};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use std::net::SocketAddr;
use bytes::Bytes;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::Framed;
use tracing::{error, info, instrument, trace, warn};
use vhl_stdlib::serdes::{NibbleBuf, NibbleBufMut};
use xpi::owned::{Event, NodeId};
use xpi::xwfd;

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
                let (frames_sink, frames_source) =
                    Framed::new(tcp_stream, RmvlbCodec::new_with_max_length(512)).split(); // TODO: do no hardcode
                tokio::spawn(async move {
                    tcp_event_loop(
                        self_id,
                        remote_addr,
                        frames_sink,
                        frames_source,
                        to_event_loop.clone(),
                        to_event_loop_internal,
                        rx,
                    )
                        .await
                });
                let remote_descriptor = RemoteDescriptor {
                    reachable: vec![],
                    addr: RemoteNodeAddr::Tcp(remote_addr),
                    to_event_loop: tx,
                };
                match tx_internal
                    .send(InternalEvent::ConnectRemoteTcp(remote_descriptor))
                    .await
                {
                    Ok(_) => {}
                    Err(_) => error!("tx_internal: send failed"),
                }
            }
            Err(e) => {
                warn!("{e:?}");
            }
        }
    }
}

#[instrument(skip(to_event_loop, frames_sink, frames_source, _to_event_loop_internal, from_event_loop))]
pub async fn tcp_event_loop(
    _self_id: NodeId,
    addr: SocketAddr,
    mut frames_sink: SplitSink<Framed<TcpStream, RmvlbCodec>, Bytes>,
    frames_source: SplitStream<Framed<TcpStream, RmvlbCodec>>,
    mut to_event_loop: Sender<Event>,
    _to_event_loop_internal: Sender<InternalEvent>,
    mut from_event_loop: Receiver<Event>,
) {
    info!("Entering tcp event loop on {addr}");
    let mut frames_source = frames_source.fuse();

    loop {
        futures::select! {
            frame = frames_source.next() => {
                match frame {
                    Some(Ok(frame)) => {
                        process_incoming_frame(frame, &mut to_event_loop).await;
                    }
                    Some(Err(e)) => {
                       error!("Decoder from tcp error: {:?}", e);
                    }
                    None => {

                    }
                }
            }
            ev = from_event_loop.select_next_some() => {
                serialize_and_send(ev, &mut frames_sink).await;
            },
        }
    }
}

async fn process_incoming_frame(bytes: Bytes, to_event_loop: &mut Sender<Event>) {
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
    frames_sink: &mut SplitSink<Framed<TcpStream, RmvlbCodec>, Bytes>,
) {
    let mut buf = Vec::new();
    buf.resize(10_000, 0);
    let mut nwr = NibbleBufMut::new_all(&mut buf);
    match ev.ser_xwfd(&mut nwr) {
        Ok(()) => {
            let (_, len, _) = nwr.finish();
            trace!("serialize_and_send: ser_xwfd ok, len: {:?}", len);
            buf.resize(len, 0);
            match frames_sink.send(Bytes::from(buf)).await {
                Ok(_) => {}
                Err(e) => error!("Encoder for tcp error: {e:?}"),
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
