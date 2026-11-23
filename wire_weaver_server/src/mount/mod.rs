//! Mount slots on a host server (SRV-76): what a forwarder needs to serve a mounted backend's API as a branch of the
//! server's own.
//!
//! - [MountedTraits]: global trait paths into the mounted APIs, resolved from each backend's learned introspection
//!   data (SRV-77).
//! - The prefix rewrite of requests and events crossing a slot is `ww_client_server::mount`'s
//!   ([MountPrefix](ww_client_server::mount::MountPrefix), [request_at](ww_client_server::mount::request_at)).

mod connect;
mod forward;
mod global_paths;
mod link;
mod table;

pub use connect::Connect;
pub use forward::Forwarder;
#[cfg(feature = "local_socket")]
pub use connect::LocalSocket;
pub use link::LinkDown;
pub use table::{Learn, MountEvent, MountOptions, MountRoutes, Mounts};

pub use global_paths::MountedTraits;
