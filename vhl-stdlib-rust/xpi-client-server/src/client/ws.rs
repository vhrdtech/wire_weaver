use std::collections::HashMap;
use std::io::Cursor;

use super::{InternalReq, InternalResp};
use futures_util::{
    stream::{SplitSink, SplitStream},
    Sink, SinkExt, StreamExt, TryStreamExt,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, instrument, trace, warn};
use xpi::client_server_owned::{Event, Protocol};

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
    let mut instances: HashMap<u8, (UnboundedSender<Event>, String)> = HashMap::new();
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
                                close_connection = serialize_and_send(event, ws_tx).await;
                            }
                            None => {
                                trace!("break C");
                                break;
                            }
                        }
                    }

                    internal_req = internal_rx.recv() => {
                        match internal_req {
                            Some(InternalReq::AddInstance { seq_subset, tx, name }) => {
                                instances.insert(seq_subset, (tx, name));
                                if internal_tx.send(InternalResp::InstanceCreated).is_err() {
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
                                warn!("Replying with error to {event:?} because of disconnected state");
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
                            Some(InternalReq::AddInstance { seq_subset, tx, name }) => {
                                instances.insert(seq_subset, (tx, name));
                                if internal_tx.send(InternalResp::InstanceCreated).is_err() {
                                    trace!("break G");
                                    break;
                                }
                            }
                            Some(InternalReq::Connect(protocol)) => {
                                match protocol {
                                    Protocol::Tcp { .. } => unimplemented!(),
                                    Protocol::Ws { ip_addr, port } => {
                                        let url = format!("ws://{ip_addr}:{port}");
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
    instances: &mut HashMap<u8, (UnboundedSender<Event>, String)>,
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
                    let destination_node = (ev.seq.0 >> 24) as u8;
                    if let Some((tx, name)) = instances.get_mut(&destination_node) {
                        if tx.send(ev).is_err() {
                            debug!("dropping instance with id: {destination_node:?} ({name})");
                            instances.remove(&destination_node);
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

async fn reply_with_error(
    event: Event,
    instances: &mut HashMap<u8, (UnboundedSender<Event>, String)>,
) {
    if let Some(reply) = event.flip_with_error(xpi::error::XpiError::Disconnected) {
        let seq_subset = (event.seq.0 >> 24) as u8;
        if let Some((tx, _)) = instances.get(&seq_subset) {
            tx.send(reply).unwrap();
        }
    }
}

async fn serialize_and_send(ev: Event, ws_sink: impl Sink<Message>) -> bool {
    tokio::pin!(ws_sink);
    // trace!("sending: {ev}");

    let mut buf = Vec::new();
    match serde::Serialize::serialize(&ev, &mut rmp_serde::Serializer::new(&mut buf)) {
        Ok(()) => match ws_sink.send(Message::Binary(buf)).await {
            Ok(_) => {}
            Err(_) => {
                error!("ws send error");
                return true;
            }
        },
        Err(e) => {
            error!("rmp serialize error {e:?}");
        }
    }

    false
}
