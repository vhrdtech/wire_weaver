pub mod codec;
pub mod node;
pub mod remote;

pub mod prelude {
    pub use xpi::owned::{Event, EventKind, NodeId, NodeSet, Priority, ResourceSet, UriOwned};
}
