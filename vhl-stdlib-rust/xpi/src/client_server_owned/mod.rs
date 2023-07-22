//! Simplified xPI data structures for client-server use.

pub mod address;
pub mod event;
pub mod nrl;
pub mod reply;
pub mod request;

pub use address::Protocol;
pub use event::{AddressableEvent, Event, EventKind};
pub use nrl::Nrl;
pub use reply::{ReplyKind, ReplyKindDiscriminants};
pub use request::{RequestKind};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RequestId(pub u32);

// #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
// pub struct NodeId(pub u32);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TraitDescriptor {
    pub trait_id: u64,
}

// #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
// pub enum Error {
//     Disconnected,
// }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ReplyAck {
    /// Always reply to requests
    Ack,
    /// Only reply if request resulted in an error
    Nack,
    /// Do not reply regardless of a result
    Ignore,
}

pub mod prelude {
    pub use super::event::{Event, EventKind};
    pub use super::reply::{ReplyKind};
    pub use super::request::{RequestKind};
    pub use super::Nrl;
    pub use super::Protocol;
    pub use super::RequestId;
    pub use smallvec::SmallVec;
}
