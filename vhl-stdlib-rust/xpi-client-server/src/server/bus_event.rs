use postage::broadcast::{Receiver, Sender};
use postage::sink::Sink;
use tracing::error;
use xpi::client_server_owned::Protocol;

pub type ServerBusTx = Sender<ServerInfoEvent>;
pub type ServerBusRx = Receiver<ServerInfoEvent>;

#[derive(Clone, Debug)]
pub enum ServerInfoEvent {
    ClientConnected(Protocol),
    ClientDisconnected(Protocol),
}

pub async fn send_server_info(info: ServerInfoEvent, tx: &mut ServerBusTx) {
    if let Err(e) = tx.send(info).await {
        error!("ServerBusTx: {e:?}");
    }
}
