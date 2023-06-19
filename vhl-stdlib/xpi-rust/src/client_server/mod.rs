//! Simplified xPI data structures for client-server use.

use smallvec::SmallVec;

pub mod address;
pub mod event;
pub mod reply;
pub mod request;

pub use address::Address;
pub use event::{Event, EventKind};
pub use reply::{Reply, ReplyKind};
pub use request::{Request, RequestKind};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RequestId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NodeId(pub u32);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TraitDescriptor {
    pub trait_id: u64,
}

pub type Nrl = SmallVec<[u32; 3]>;

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
