use std::net::SocketAddr;
use futures::channel::mpsc::{Receiver, Sender};
use futures::{FutureExt, SinkExt, StreamExt};
use futures::channel::mpsc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::WriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, instrument, trace, warn};
use vhl_stdlib::serdes::{NibbleBuf, NibbleBufMut};
use xpi::owned::{Event, NodeId};
use xpi::xwfd;
use crate::node::addressing::RemoteNodeAddr;
use crate::node::async_std::internal_event::InternalEvent;
use crate::remote::remote_descriptor::RemoteDescriptor;

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
                tokio::spawn(async move {
                    tcp_event_loop(self_id, remote_addr, tcp_stream, to_event_loop.clone(), to_event_loop_internal, rx).await
                });
                let remote_descriptor = RemoteDescriptor {
                    reachable: vec![NodeId(1)], // TODO: do not hardcode this
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

#[instrument(skip(stream, to_event_loop, to_event_loop_internal, from_event_loop))]
pub async fn tcp_event_loop(
    _self_id: NodeId,
    addr: SocketAddr,
    mut stream: TcpStream,
    mut to_event_loop: Sender<Event>,
    mut to_event_loop_internal: Sender<InternalEvent>,
    mut from_event_loop: Receiver<Event>,
) {
    info!("Entering tcp event loop on {addr}");
    let (mut tcp_rx, mut tcp_tx) = stream.split();
    let mut buf = [0u8; 10_000];
    loop {
        futures::select! {
            read_result = tcp_rx.read(&mut buf).fuse() => {
                match read_result {
                    Ok(len) => if len > 0 {
                        process_incoming_slice(&buf[..len], &mut to_event_loop).await;
                    },
                    Err(e) => {
                        error!("Failed to read from tcp {:?}", e);
                         match to_event_loop_internal
                            .send(InternalEvent::DropRemoteTcp(addr))
                            .await {
                                Ok(_) => {}
                                Err(_) => error!("tx_internal: send failed")
                            }
                    },
                }
            }
            ev = from_event_loop.select_next_some() => {
                serialize_and_send(ev, &mut buf, &mut tcp_tx).await
            },
        }
    }
}

async fn process_incoming_slice(bytes: &[u8], to_event_loop: &mut Sender<Event>) {
    trace!("rx: {} bytes: {:2x?}", bytes.len(), bytes);
    let mut nrd = NibbleBuf::new_all(bytes);
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

async fn serialize_and_send<'tx>(ev: Event, scratchpad: &mut [u8], tcp_tx: &mut WriteHalf<'tx>) {
    //trace!("serialize_and_send: {}", ev);
    let mut nwr = NibbleBufMut::new_all(scratchpad);
    match ev.ser_xwfd(&mut nwr) {
        Ok(()) => {
            //trace!("sending xwfd: {}", nwr);
            let (_, len, _) = nwr.finish();
            let r = tcp_tx.write_all(&scratchpad[..len]).await;
            // TODO: fix to use write or smth else compatible with select!
            trace!("sent: {:?} {:2x?} {}", r, &scratchpad[..len], ev);
        }
        Err(e) => {
            error!("convert to xwfd failed: {:?}", e);
        }
    }
}
