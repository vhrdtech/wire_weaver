use futures::channel::mpsc::{Receiver, Sender};
use tokio::net::TcpStream;
use xpi::owned::{Event, NodeId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use futures::{StreamExt, FutureExt, SinkExt};
use tokio::net::tcp::WriteHalf;
use vhl_stdlib::serdes::{NibbleBuf, NibbleBufMut};
use xpi::xwfd;

pub async fn tcp_event_loop(
    _self_id: NodeId,
    mut stream: TcpStream,
    mut to_event_loop: Sender<Event>,
    mut from_event_loop: Receiver<Event>,
) {
    let (mut tcp_rx, mut tcp_tx) = stream.split();
    let mut buf = [0u8; 10_000];
    loop {
        futures::select! {
            read_result = tcp_rx.read(&mut buf).fuse() => {
                match read_result {
                    Ok(len) => process_incoming_slice(&buf[..len], &mut to_event_loop).await,
                    Err(e) => println!("{:?}", e),
                }
            }
            ev = from_event_loop.select_next_some() => {
                serialize_and_send(ev, &mut buf, &mut tcp_tx).await
            },
        }
    }
}

async fn process_incoming_slice(bytes: &[u8], to_event_loop: &mut Sender<Event>) {
    println!("rx: {} bytes from tcp", bytes.len());
    let mut nrd = NibbleBuf::new_all(bytes);
    let ev: Result<xwfd::Event, _> = nrd.des_vlu4();
    match ev {
        Ok(ev) => {
            println!("des: {}", ev);
            let ev_owned: Event = ev.into();
            to_event_loop.send(ev_owned).await;
        }
        Err(e) => {
            println!("xwfd deserialize error: {:?}", e);
        }
    }
}

async fn serialize_and_send<'tx>(ev: Event, scratchpad: &mut [u8], tcp_tx: &mut WriteHalf<'tx>) {
    println!("event to be serialized to tcp: {:?}", ev);
    let mut nwr = NibbleBufMut::new_all(scratchpad);
    match ev.ser_xwfd(&mut nwr) {
        Ok(()) => {
            println!("sending xwfd: {}", nwr);
            let (_, len, _) = nwr.finish();
            let r = tcp_tx.write_all(&scratchpad[..len]).await;
            println!("Sent: {:?}", r);
        }
        Err(e) => {
            println!("convert to xwfd failed: {:?}", e);
        }
    }
}