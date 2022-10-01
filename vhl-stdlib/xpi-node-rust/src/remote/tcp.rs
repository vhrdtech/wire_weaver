use futures::channel::mpsc::{Receiver, Sender};
use tokio::net::TcpStream;
use xpi::owned::{Event, NodeId};
use tokio::io::AsyncReadExt;
use futures::{StreamExt, FutureExt};
use vhl_stdlib::serdes::NibbleBufMut;

pub async fn tcp_event_loop(
    _self_id: NodeId,
    mut stream: TcpStream,
    _to_event_loop: Sender<Event>,
    mut from_event_loop: Receiver<Event>,
) {
    let (mut tcp_rx, _tcp_tx) = stream.split();
    let mut buf = [0u8; 10_000];
    loop {
        futures::select! {
            read_result = tcp_rx.read(&mut buf).fuse() => {
                match read_result {
                    Ok(len) => process_incoming_slice(&buf[..len]).await,
                    Err(e) => println!("{:?}", e),
                }
            }
            ev = from_event_loop.select_next_some() => serialize_and_send(ev, &mut buf).await,
        }
    }
}

async fn process_incoming_slice(bytes: &[u8]) {
    println!("rx: {} bytes from tcp", bytes.len());
}

async fn serialize_and_send(ev: Event, scratchpad: &mut [u8]) {
    println!("event to be serialized to tcp: {:?}", ev);
    let mut nwr = NibbleBufMut::new_all(scratchpad);
    match ev.ser_xwfd(&mut nwr) {
        Ok(()) => {
            println!("xwfd: {}", nwr);
        }
        Err(e) => {
            println!("convert to xwfd failed: {:?}", e);
        }
    }
}