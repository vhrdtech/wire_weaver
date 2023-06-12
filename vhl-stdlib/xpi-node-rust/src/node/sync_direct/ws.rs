use std::collections::HashMap;
use std::io::Cursor;

use futures_util::{
    stream::{SplitSink, SplitStream},
    StreamExt, TryStreamExt,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, instrument, trace, warn};
use xpi::owned::{Event, NodeId, NodeSet};

use crate::node::addressing::RemoteNodeAddr;

use super::node::{InternalReq, InternalResp};

#[instrument(skip(events_rx, internal_rx, internal_tx))]
pub async fn ws_event_loop(
    // ws_tx: impl Sink<Message>,
    // ws_rx: impl Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>,
    mut events_rx: UnboundedReceiver<Event>,
    mut internal_rx: UnboundedReceiver<InternalReq>,
    internal_tx: UnboundedSender<InternalResp>,
) {
    info!("Entering sync client event loop");
    // tokio::pin!(ws_rx);
    // tokio::pin!(ws_tx);
    let mut instances: HashMap<NodeId, (UnboundedSender<Event>, String)> = HashMap::new();
    let mut next_id = 1u32; // TODO: this is a hack, use (ip, port, id)?
    let mut ws_txrx: Option<(SplitSink<_, _>, SplitStream<_>)> = None;
    let mut close_connection = false;

    loop {
        match &mut ws_txrx {
            Some((ws_tx, ws_rx)) => {
                tokio::select! {
                    frame = ws_rx.try_next() => {
                        match frame {
                            Ok(Some(frame)) => {
                                close_connection = process_incoming_frame(frame, &mut instances).await;
                            }
                            Ok(None) => {
                                trace!("break A");
                                break;
                            }
                            Err(e) => {
                                // Connection was dropped by server
                                warn!("{e}");
                                close_connection = true;
                            }
                        }
                    }

                    event = events_rx.recv() => {
                        match event {
                            Some(event) => {
                                close_connection = crate::remote::ws::serialize_and_send(event, ws_tx).await;
                            }
                            None => {
                                trace!("break C");
                                break;
                            }
                        }
                    }

                    internal_req = internal_rx.recv() => {
                        match internal_req {
                            Some(InternalReq::AddInstance { tx, name }) => {
                                let id = NodeId(next_id);
                                next_id += 1; // TODO: recycle
                                instances.insert(id, (tx, name));
                                if internal_tx.send(InternalResp::InstanceCreated { id }).is_err() {
                                    trace!("break D");
                                    break;
                                }
                            }
                            Some(InternalReq::Connect(addr)) => {
                                warn!("Cannot connect to {addr:?}, while being already connected");
                            }
                            Some(InternalReq::Disconnect) => {
                                info!("Disconnecting due to user request");
                                close_connection = true;
                            }
                            Some(InternalReq::Stop) => {
                                info!("Stopping due to user request");
                                break;
                            }
                            _ => {
                                trace!("break E");
                                break;
                            }
                        }
                    }
                }
            }
            None => {
                tokio::select! {
                    event = events_rx.recv() => {
                        match event {
                            Some(event) => {
                                warn!("Replying with error to {event} because of disconnected state");
                                reply_with_error(event, &mut instances).await;
                            }
                            None => {
                                trace!("break F");
                                break;
                            }
                        }
                    }
                    internal_req = internal_rx.recv() => {
                        match internal_req {
                            Some(InternalReq::AddInstance { tx, name }) => {
                                let id = NodeId(next_id);
                                next_id += 1; // TODO: recycle
                                instances.insert(id, (tx, name));
                                if internal_tx.send(InternalResp::InstanceCreated { id }).is_err() {
                                    trace!("break G");
                                    break;
                                }
                            }
                            Some(InternalReq::Connect(addr)) => {
                                match addr {
                                    RemoteNodeAddr::Tcp(_) => unimplemented!(),
                                    RemoteNodeAddr::Ws(ip_addr) => {
                                        let url = format!("ws://{ip_addr}");
                                        info!("ws: Connecting to remote {url}");
                                        let ws_stream = match tokio_tungstenite::connect_async(url).await {
                                            Ok((ws_stream, _)) => ws_stream,
                                            Err(e) => {
                                                warn!("{e:?}");
                                                continue;
                                            }
                                        };
                                        let (ws_tx, ws_rx) = ws_stream.split();
                                        ws_txrx = Some((ws_tx, ws_rx));
                                    }
                                }
                            }
                            Some(InternalReq::Stop) => {
                                info!("Stopping due to user request");
                                break;
                            }
                            ignore => {
                                trace!("ignoring due to disconnected state: {ignore:?}");
                            }
                        }
                    }
                }
            }
        }

        if close_connection {
            ws_txrx = match ws_txrx.take() {
                Some((ws_tx, ws_rx)) => {
                    let r = ws_tx.reunite(ws_rx).unwrap().close(None).await;
                    info!("Closing connection {r:?}");
                    None
                }
                None => None,
            };
            close_connection = false;
        }
    }
    info!("Exiting websocket event loop");
}

async fn process_incoming_frame(
    ws_message: Message,
    instances: &mut HashMap<NodeId, (UnboundedSender<Event>, String)>,
) -> bool {
    // trace!("rx: {} bytes: {:2x?}", bytes.len(), &bytes);
    match ws_message {
        Message::Binary(bytes) => {
            let cur = Cursor::new(bytes);
            let mut de = rmp_serde::Deserializer::new(cur);
            let ev: Result<Event, _> = serde::Deserialize::deserialize(&mut de);
            match ev {
                Ok(ev) => {
                    // trace!("rx {}B: {}", bytes.len(), ev);
                    trace!("received: {ev}");
                    if let NodeSet::Unicast(id) = ev.destination {
                        if let Some((tx, name)) = instances.get_mut(&id) {
                            if tx.send(ev).is_err() {
                                debug!("dropping instance with id: {id} ({name})");
                                instances.remove(&id);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("rmp deserialize error {e:?}");
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

/// TODO: many cases not handled
async fn reply_with_error(
    event: Event,
    instances: &mut HashMap<NodeId, (UnboundedSender<Event>, String)>,
) {
    let mut reply = event.clone();
    reply.kind = event.kind.flip_with_error();
    if let NodeSet::Unicast(id) = event.destination {
        reply.source = id;
    }
    reply.destination = NodeSet::Unicast(event.source);
    if let Some((tx, _)) = instances.get(&event.source) {
        tx.send(reply).unwrap();
    }
}
