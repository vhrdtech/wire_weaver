use futures::channel::mpsc::{Receiver, Sender};
use futures::{FutureExt, SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::WriteHalf;
use tokio::net::TcpStream;
use tracing::{error, info, instrument, trace};
use vhl_stdlib::serdes::{NibbleBuf, NibbleBufMut};
use xpi::owned::{Event, NodeId};
use xpi::xwfd;

#[instrument(skip(stream, to_event_loop, from_event_loop))]
pub async fn tcp_event_loop(
    _self_id: NodeId,
    mut stream: TcpStream,
    mut to_event_loop: Sender<Event>,
    mut from_event_loop: Receiver<Event>,
) {
    info!("Entering tcp event loop");
    let (mut tcp_rx, mut tcp_tx) = stream.split();
    let mut buf = [0u8; 10_000];
    loop {
        futures::select! {
            read_result = tcp_rx.read(&mut buf).fuse() => {
                match read_result {
                    Ok(len) => process_incoming_slice(&buf[..len], &mut to_event_loop).await,
                    Err(e) => error!("Failed to read from tcp {:?}", e),
                }
            }
            ev = from_event_loop.select_next_some() => {
                serialize_and_send(ev, &mut buf, &mut tcp_tx).await
            },
        }
    }
}

async fn process_incoming_slice(bytes: &[u8], to_event_loop: &mut Sender<Event>) {
    // trace!("rx: {} bytes: {:2x?}", bytes.len(), bytes);
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
            error!("xwfd deserialize error: {:?}", e);
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
