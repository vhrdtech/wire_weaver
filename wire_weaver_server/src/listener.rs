//! Where connections come from.

use std::future::Future;

use wire_weaver::Peer;
use ww_device::{MessageRx, MessageTx};

/// A source of connections: a socket that accepts, an in-process path, a stream handed in by the program.
pub trait Listener {
    /// Sends messages to the client.
    type Tx: MessageTx;
    /// Receives messages from the client.
    type Rx: MessageRx;
    /// Why accepting failed.
    type Error;

    /// The next connection, `None` when no more will come (one stream served, its source closed): the server then
    /// ends once the connections it has ended. Must be cancel-safe: [Server::serve](crate::Server::serve) drops the
    /// future whenever something else is due, and a connection that arrived meanwhile must come out of the next call.
    fn accept(
        &mut self,
    ) -> impl Future<Output = Result<Option<Accepted<Self::Tx, Self::Rx>>, Self::Error>>;
}

/// One accepted connection.
pub struct Accepted<Tx, Rx> {
    /// Sends messages to the client.
    pub tx: Tx,
    /// Receives messages from the client.
    pub rx: Rx,
    /// Who connected, as the transport saw it: never what the client says about itself.
    pub peer: Peer,
}
