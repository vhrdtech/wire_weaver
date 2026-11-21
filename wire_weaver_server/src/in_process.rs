//! A listener for clients of the same process, which connect by path (`ClientConfig::in_process_path`): for tests
//! and simulators. Every connection gets the peer the listener was given, so a policy can be tested with any peer.

use std::convert::Infallible;

use wire_weaver::Peer;
use wire_weaver_client::in_process::{DeviceRx, DeviceTx};

use crate::listener::{Accept, Accepted, Listener};

/// Connections of clients in the same process.
pub struct InProcessListener {
    inner: wire_weaver_client::in_process::Listener,
    peer: Peer,
}

impl InProcessListener {
    /// Listen on `path`; `message_len_max` is the longest message the server takes. Clients are [Peer::Local]
    /// unless [with_peer](Self::with_peer) says otherwise.
    pub fn new(path: &str, message_len_max: usize) -> Self {
        InProcessListener {
            inner: wire_weaver_client::in_process::listener(path, message_len_max),
            peer: Peer::Local,
        }
    }

    /// Give every connection `peer`, as if a transport had seen it.
    #[must_use]
    pub fn with_peer(mut self, peer: Peer) -> Self {
        self.peer = peer;
        self
    }
}

impl Listener for InProcessListener {
    type Tx = DeviceTx;
    type Rx = DeviceRx;
    type Error = Infallible;

    async fn accept(&mut self) -> Accept<DeviceTx, DeviceRx, Infallible> {
        let (tx, rx) = self.inner.accept().await;
        Ok(Some(Accepted {
            tx,
            rx,
            peer: self.peer,
        }))
    }
}
