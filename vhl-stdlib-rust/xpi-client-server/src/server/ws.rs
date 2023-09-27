use std::io::Cursor;
use std::sync::{Arc, RwLock};

use crate::server::internal_event::{InternalEvent, InternalEventToEventLoop};
use crate::server::remote_descriptor::RemoteDescriptor;
use futures::channel::mpsc;
use futures::channel::mpsc::{Receiver, Sender};
use futures_util::{Sink, SinkExt, Stream, StreamExt, TryStreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, instrument, warn};
use xpi::client_server_owned::{AddressableEvent, Event, Protocol};

#[instrument(skip(listener, tx_to_bridge, routes, tx_internal))]
pub(crate) async fn ws_server_acceptor(
    listener: TcpListener,
    routes: Arc<RwLock<Vec<super::NrlSpecificDispatcherHandle>>>,
    tx_to_bridge: Sender<AddressableEvent>,
    mut tx_internal: Sender<InternalEvent>,
) {
    loop {
        match listener.accept().await {
            Ok((tcp_stream, remote_addr)) => {
                info!("Got new connection from: {remote_addr}");
                let ws_stream = match tokio_tungstenite::accept_async(tcp_stream).await {
                    Ok(ws_stream) => ws_stream,
                    Err(e) => {
                        warn!("Error during the websocket handshake occurred {e:?}");
                        continue;
                    }
                };

                let (ws_sink, ws_source) = StreamExt::split(ws_stream);

                let (tx, rx) = mpsc::channel(64);
                let (st_tx, st_rx) = mpsc::channel(16);
                let tx_to_bridge = tx_to_bridge.clone();
                let routes = routes.read().map(|h| h.clone()).unwrap_or_default();
                let to_event_loop_internal = tx_internal.clone();
                let protocol = Protocol::Ws {
                    ip_addr: remote_addr.ip(),
                    port: remote_addr.port(),
                };

                tokio::spawn(async move {
                    ws_event_loop(
                        protocol,
                        ws_sink,
                        ws_source,
                        routes,
                        tx_to_bridge.clone(),
                        to_event_loop_internal,
                        st_rx,
                        rx,
                    )
                    .await
                });
                let remote_descriptor = RemoteDescriptor {
                    protocol,
                    to_event_loop: tx,
                    to_event_loop_internal: st_tx,
                };
                match tx_internal
                    .send(InternalEvent::ConnectRemote(remote_descriptor))
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
    tx_to_bridge,
    ws_sink,
    ws_source,
    to_event_loop_internal,
    from_event_loop,
    from_event_loop_internal,
    routes
))]
pub async fn ws_event_loop(
    protocol: Protocol,
    ws_sink: impl Sink<Message>,
    ws_source: impl Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>,
    mut routes: Vec<super::NrlSpecificDispatcherHandle>,
    mut tx_to_bridge: Sender<AddressableEvent>,
    mut to_event_loop_internal: Sender<InternalEvent>,
    mut from_event_loop_internal: Receiver<InternalEventToEventLoop>,
    mut from_event_loop: Receiver<AddressableEvent>,
) {
    info!("Event loop started");
    debug!("{:?}", routes);
    tokio::pin!(ws_sink);
    tokio::pin!(ws_source);
    let (tx_local, mut rx_direct) = futures::channel::mpsc::channel(64); // TODO: config
    loop {
        tokio::select! {
            frame = ws_source.try_next() => {
                match frame {
                    Ok(Some(frame)) => {
                        let should_terminate = process_incoming_frame(protocol, frame, &mut routes, &mut tx_to_bridge, &tx_local).await;
                        if should_terminate {
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
                    break;
                }
            },
            ev = rx_direct.select_next_some() => {
                let should_terminate = serialize_and_send(ev, &mut ws_sink).await;
                if should_terminate {
                    let _ = to_event_loop_internal.send(InternalEvent::DropRemote(protocol)).await;
                    break;
                }
            },
            ev = from_event_loop_internal.select_next_some() => {
                match ev {
                    InternalEventToEventLoop::RegisterDispatcherForNrl(handler) => {
                        if routes.iter().any(|h| h.nrl == handler.nrl) {
                            warn!("Tried registering the same Nrl dispatcher twice {}", handler.nrl);
                        } else {
                            info!("Registered Nrl specific dispatcher: {}", handler.nrl);
                            routes.push(handler);
                            routes.sort_by(|a, b| b.nrl.0.len().cmp(&a.nrl.0.len()));
                            debug!("{routes:?}");
                        }
                    }
                    InternalEventToEventLoop::DropDispatcherForNrl(nrl) => {
                        routes.retain(|r| r.nrl != nrl);
                        info!("Dropped handler for {nrl}");
                    }
                    // InternalEventToEventLoop::DropAllRelatedTo(protocol) => {
                    //     info!("Dropping all stateful dispatcher related to {} due to request", protocol);
                    //     tx_to_stateful.retain(|(p, _), _| *p != protocol);
                    // }
                }
            }
        }
    }

    let _ = to_event_loop_internal
        .send(InternalEvent::DropRemote(protocol))
        .await;
    info!("Event loop: exiting");
}

async fn process_incoming_frame(
    protocol: Protocol,
    ws_message: Message,
    nrl_specific_handlers: &mut Vec<super::NrlSpecificDispatcherHandle>,
    tx_to_bridge: &mut Sender<AddressableEvent>,
    tx_local: &Sender<AddressableEvent>,
) -> bool {
    match ws_message {
        Message::Binary(bytes) => {
            let cur = Cursor::new(bytes);
            let mut de = rmp_serde::Deserializer::new(cur);
            let event: Result<Event, _> = serde::Deserialize::deserialize(&mut de);
            match event {
                Ok(event) => {
                    let mut event = AddressableEvent {
                        protocol,
                        is_inbound: true,
                        event,
                        response_tx: tx_local.clone(),
                    };

                    // let mut possible_entry = event.event.nrl.clone();
                    // let mut sent_to_stateful = false;
                    // let mut should_drop = false;
                    // while possible_entry.0.len() > 2 {
                    //     // debug!("trying {}", possible_entry);
                    //     if let Some(tx) =
                    //         tx_to_stateful.get_mut(&(protocol, possible_entry.clone()))
                    //     {
                    //         if tx.send(event.clone()).await.is_ok() {
                    //             trace!("sent to stateful {} {}", protocol, possible_entry);
                    //             sent_to_stateful = true;
                    //         } else {
                    //             debug!("direct channel to stateful handler is closed, dropping it and routing through other channel(s)");
                    //             should_drop = true;
                    //         }
                    //         break;
                    //     }
                    //     possible_entry.0.remove(possible_entry.0.len() - 1);
                    // }
                    // if should_drop {
                    //     tx_to_stateful.remove(&(protocol, possible_entry));
                    // }
                    // if sent_to_stateful {
                    //     return false;
                    // }

                    let mut drop_idx = None;
                    for (idx, h) in nrl_specific_handlers.iter_mut().enumerate() {
                        if h.matches(&event.event.nrl) {
                            if h.tx.is_closed() {
                                warn!("mpsc to subgroup handler failed");
                                drop_idx = Some(idx);
                                continue;
                            }
                            event.event.nrl.0.drain(0..h.nrl.0.len());
                            if h.tx.send(event).await.is_err() {
                                warn!("mpsc failed even though it wasn't closed?");
                            }
                            return false;
                        }
                    }
                    if let Some(idx) = drop_idx {
                        nrl_specific_handlers.remove(idx);
                        debug!("{nrl_specific_handlers:?}");
                    }

                    // trace!("{event}");
                    if tx_to_bridge.send(event).await.is_err() {
                        error!("mpsc fail, main event loop must have crashed?");
                        return true;
                    }
                }
                Err(e) => {
                    // error!("xwfd deserialize error: {:?} bytes: {:02x?}", e, bytes);
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

pub(crate) async fn serialize_and_send(ev: AddressableEvent, ws_sink: impl Sink<Message>) -> bool {
    tokio::pin!(ws_sink);
    // trace!("sending: {ev}");

    let mut buf = Vec::new();
    match serde::Serialize::serialize(&ev.event, &mut rmp_serde::Serializer::new(&mut buf)) {
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
