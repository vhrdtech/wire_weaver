use std::io::Cursor;

use crate::server::internal_event::InternalEvent;
use crate::server::remote_descriptor::RemoteDescriptor;
use futures::channel::mpsc;
use futures::channel::mpsc::{Receiver, Sender};
use futures_util::{Sink, SinkExt, Stream, StreamExt, TryStreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, instrument, trace, warn};
use xpi::client_server_owned::{AddressableEvent, Event, Protocol};

#[instrument(skip(listener, tx_to_event_loop, tx_internal))]
pub(crate) async fn ws_server_acceptor(
    listener: TcpListener,
    tx_to_event_loop: Sender<AddressableEvent>,
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
                let protocol = Protocol::Ws {
                    ip_addr: remote_addr.ip(),
                    port: remote_addr.port(),
                };

                tokio::spawn(async move {
                    ws_event_loop(
                        protocol,
                        ws_sink,
                        ws_source,
                        to_event_loop.clone(),
                        to_event_loop_internal,
                        rx,
                    )
                    .await
                });
                let remote_descriptor = RemoteDescriptor {
                    protocol,
                    to_event_loop: tx,
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
    to_event_loop,
    ws_sink,
    ws_source,
    to_event_loop_internal,
    from_event_loop
))]
pub async fn ws_event_loop(
    protocol: Protocol,
    ws_sink: impl Sink<Message>,
    ws_source: impl Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>,
    mut to_event_loop: Sender<AddressableEvent>,
    mut to_event_loop_internal: Sender<InternalEvent>,
    mut from_event_loop: Receiver<AddressableEvent>,
) {
    info!("Entering websocket event loop on {protocol}");
    // let mut ws_source = ws_source.fuse();
    tokio::pin!(ws_sink);
    tokio::pin!(ws_source);
    loop {
        tokio::select! {
            frame = ws_source.try_next() => {
                match frame {
                    Ok(Some(frame)) => {
                        let should_terminate = process_incoming_frame(protocol, frame, &mut to_event_loop).await;
                        if should_terminate {
                            let _ = to_event_loop_internal.send(InternalEvent::DropRemote(protocol)).await;
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
                    let _ = to_event_loop_internal.send(InternalEvent::DropRemote(protocol)).await;
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

async fn process_incoming_frame(
    protocol: Protocol,
    ws_message: Message,
    to_event_loop: &mut Sender<AddressableEvent>,
) -> bool {
    // trace!("rx: {} bytes: {:2x?}", bytes.len(), &bytes);
    match ws_message {
        Message::Binary(bytes) => {
            // let mut nrd = NibbleBuf::new_all(&bytes);
            // let ev: Result<xwfd::Event, _> = nrd.des_vlu4();
            let cur = Cursor::new(bytes);
            let mut de = rmp_serde::Deserializer::new(cur);
            let event: Result<Event, _> = serde::Deserialize::deserialize(&mut de);
            match event {
                Ok(event) => {
                    // trace!("{event}");
                    if to_event_loop
                        .send(AddressableEvent {
                            protocol,
                            is_inbound: true,
                            event,
                        })
                        .await
                        .is_err()
                    {
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
    trace!("sending: {ev}");

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
    // let mut buf = Vec::new();
    // buf.resize(10_000, 0);
    // let mut nwr = NibbleBufMut::new_all(&mut buf);
    // match ev.ser_xwfd(&mut nwr) {
    //     Ok(()) => {
    //         let (_, len, _) = nwr.finish();
    //         // trace!("serialize_and_send: ser_xwfd ok, len: {:?}", len);
    //         buf.resize(len, 0);

    //         match ws_sink.send(Message::Binary(buf)).await {
    //             Ok(_) => {}
    //             Err(_) => {
    //                 error!("ws send error");
    //             }
    //         }
    //     }
    //     Err(e) => {
    //         error!("convert of event: {ev} to xwfd failed: {e:?}");
    //     }
    // }
    false
}