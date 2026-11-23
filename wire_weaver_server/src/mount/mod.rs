//! Mount slots on a host server (SRV-76): what a forwarder needs to serve a mounted backend's API as a branch of the
//! server's own.
//!
//! - [MountedTraits]: global trait paths into the mounted APIs, resolved from each backend's learned introspection
//!   data (SRV-77).
//! - The prefix rewrite of requests and events crossing a slot is `ww_client_server::mount`'s
//!   ([MountPrefix](ww_client_server::mount::MountPrefix), [request_at](ww_client_server::mount::request_at)).

mod global_paths;

pub use global_paths::MountedTraits;
