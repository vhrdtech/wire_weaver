use futures::channel::mpsc::{channel, Receiver, Sender};
use xpi::client_server_owned::{AddressableEvent, Nrl, Protocol};

#[derive(Clone, Debug)]
pub struct ClientSpecificDispatcherHandle {
    pub protocol: Protocol,
    pub nrl: Nrl,
    pub tx: Sender<AddressableEvent>,
}

impl PartialEq<ClientSpecificDispatcherHandle> for ClientSpecificDispatcherHandle {
    fn eq(&self, other: &ClientSpecificDispatcherHandle) -> bool {
        self.protocol == other.protocol && self.nrl == other.nrl
    }
}

impl ClientSpecificDispatcherHandle {
    pub fn new(protocol: Protocol, nrl: Nrl) -> (Self, Receiver<AddressableEvent>) {
        let (tx, rx) = channel(64);
        (Self { protocol, nrl, tx }, rx)
    }
}

#[derive(Clone, Debug)]
pub struct NrlSpecificDispatcherHandle {
    pub nrl: Nrl,
    pub tx: Sender<AddressableEvent>,
}

impl NrlSpecificDispatcherHandle {
    pub fn new(nrl: Nrl) -> (Self, Receiver<AddressableEvent>) {
        let (tx, rx) = channel(64);
        (Self { nrl, tx }, rx)
    }

    pub fn matches(&self, nrl: &Nrl) -> bool {
        if nrl.0.len() < self.nrl.0.len() {
            return false;
        }
        if self.nrl.0[..] != nrl.0[..self.nrl.0.len()] {
            return false;
        }
        true
    }
}

#[derive(Debug)]
pub enum ServerControlRequest {
    /// Handle incoming requests for specific Nrl and specific Client via separate dispatcher instance
    RegisterClientSpecificDispatcher(ClientSpecificDispatcherHandle),
    /// Handle incoming requests for specific Nrl and all Client's via specified dispatcher
    RegisterNrlBasedDispatcher(NrlSpecificDispatcherHandle),
}
