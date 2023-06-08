use std::net::SocketAddr;

use crate::node::addressing::RemoteNodeAddr;
use crate::node::async_std::internal_event::InternalEvent;
use crate::remote::remote_descriptor::RemoteDescriptor;
use futures::channel::mpsc;
use futures::channel::mpsc::{Receiver, Sender};
// use futures::{SinkExt, StreamExt};
use futures_util::{Sink, SinkExt, Stream, StreamExt, TryStreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, instrument, trace, warn};
use vhl_stdlib::serdes::{NibbleBuf, NibbleBufMut};
use xpi::owned::{Event, NodeId};
use xpi::xwfd;

#[instrument(skip(listener, tx_to_event_loop, tx_internal))]
pub(crate) async fn ws_server_acceptor(
    self_id: NodeId,
    listener: TcpListener,
    tx_to_event_loop: Sender<Event>,
    mut tx_internal: Sender<InternalEvent>,
) {
    loop {
        match listener.accept().await {
            Ok((tcp_stream, remote_addr)) => {
                info!("Got new connection from: {remote_addr}");
                let ws_stream = tokio_tungstenite::accept_async(tcp_stream)
                    .await
                    .expect("Error during the websocket handshake occured");

                let (ws_sink, ws_source) = futures_util::StreamExt::split(ws_stream);

                let (tx, rx) = mpsc::channel(64);
                let to_event_loop = tx_to_event_loop.clone();
                let to_event_loop_internal = tx_internal.clone();

                tokio::spawn(async move {
                    ws_event_loop(
                        self_id,
                        remote_addr,
                        ws_sink,
                        ws_source,
                        to_event_loop.clone(),
                        to_event_loop_internal,
                        rx,
                    )
                    .await
                });
                let remote_descriptor = RemoteDescriptor {
                    reachable: vec![NodeId(1)], // TODO: Do not hardcode
                    addr: RemoteNodeAddr::Ws(remote_addr),
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

#[instrument(skip(
    to_event_loop,
    ws_sink,
    ws_source,
    to_event_loop_internal,
    from_event_loop
))]
pub async fn ws_event_loop(
    _self_id: NodeId,
    addr: SocketAddr,
    ws_sink: impl Sink<Message>,
    ws_source: impl Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>,
    mut to_event_loop: Sender<Event>,
    mut to_event_loop_internal: Sender<InternalEvent>,
    mut from_event_loop: Receiver<Event>,
) {
    info!("Entering tcp event loop on {addr}");
    // let mut ws_source = ws_source.fuse();
    tokio::pin!(ws_sink);
    tokio::pin!(ws_source);
    loop {
        tokio::select! {
            frame = ws_source.try_next() => {
                match frame {
                    Ok(Some(frame)) => {
                        let should_terminate = process_incoming_frame(frame, &mut to_event_loop).await;
                        if should_terminate {
                            let _ = to_event_loop_internal.send(InternalEvent::DropRemoteTcp(addr)).await;
                            break;
                        }
                    }
                    Ok(None) => {
                        break;
                    }
                    Err(e) => {
                        warn!("{e}");
                        break;
                    }
                }
            }
            ev = from_event_loop.select_next_some() => {
                let should_terminate = serialize_and_send(ev, &mut ws_sink).await;
                if should_terminate {
                    let _ = to_event_loop_internal.send(InternalEvent::DropRemoteTcp(addr)).await;
                    break;
                }
            },
            // complete => {
            //     error!("Unexpected select! completion, exiting");
            //     let _ = to_event_loop_internal.send(InternalEvent::DropRemoteTcp(addr)).await;
            //     break;
            // }
        }
    }
}

async fn process_incoming_frame(ws_message: Message, to_event_loop: &mut Sender<Event>) -> bool {
    // trace!("rx: {} bytes: {:2x?}", bytes.len(), &bytes);
    match ws_message {
        Message::Binary(bytes) => {
            let mut nrd = NibbleBuf::new_all(&bytes);
            let ev: Result<xwfd::Event, _> = nrd.des_vlu4();
            match ev {
                Ok(ev) => {
                    trace!("rx {}B: {}", bytes.len(), ev);
                    let ev_owned: Event = ev.into();
                    if to_event_loop.send(ev_owned).await.is_err() {
                        error!("mpsc fail, main event loop must have crashed?");
                        return true;
                    }
                }
                Err(e) => {
                    error!("xwfd deserialize error: {:?} bytes: {:02x?}", e, bytes);
                }
            }
        }
        Message::Close(_) => {
            return true;
        }
        u => {
            warn!("Unsupported ws message: {u:?}");
        }
    }

    false
}

async fn serialize_and_send(ev: Event, ws_sink: impl Sink<Message>) -> bool {
    let mut buf = Vec::new();
    buf.resize(10_000, 0);
    let mut nwr = NibbleBufMut::new_all(&mut buf);
    tokio::pin!(ws_sink);
    match ev.ser_xwfd(&mut nwr) {
        Ok(()) => {
            let (_, len, _) = nwr.finish();
            // trace!("serialize_and_send: ser_xwfd ok, len: {:?}", len);
            buf.resize(len, 0);

            match ws_sink.send(Message::Binary(buf)).await {
                Ok(_) => {}
                Err(_) => {
                    error!("ws send error");
                }
            }
        }
        Err(e) => {
            error!("convert of event: {ev} to xwfd failed: {e:?}");
        }
    }
    false
}
