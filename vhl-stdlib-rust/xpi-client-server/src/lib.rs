pub mod server;
pub mod client;
pub mod remote;
pub mod filter;

pub mod prelude {
    pub use xpi::client_server_owned::{Event, EventKind, NodeId};
}
