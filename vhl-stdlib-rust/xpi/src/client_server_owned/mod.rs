//! Simplified xPI data structures for client-server use.

use smallvec::SmallVec;

pub mod address;
pub mod event;
pub mod reply;
pub mod request;

pub use address::{Address, Protocol};
pub use event::{Event, EventKind};
pub use reply::{Reply, ReplyKind, ReplyKindDiscriminants};
pub use request::{Request, RequestKind};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RequestId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NodeId(pub u32);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TraitDescriptor {
    pub trait_id: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Nrl(pub SmallVec<[u32; 3]>);
impl Default for Nrl {
    fn default() -> Self {
        Nrl(SmallVec::new())
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Error {
    Disconnected,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ReplyAck {
    /// Always reply to requests
    Ack,
    /// Only reply if request resulted in an error
    Nack,
    /// Do not reply regardless of a result
    Ignore,
}
