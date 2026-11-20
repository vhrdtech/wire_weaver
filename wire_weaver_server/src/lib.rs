//! WireWeaver host server: one API served to many clients at once (SRV-12).
//!
//! [Server::serve] takes connections from a [Listener], runs link setup on each, makes it a backend of its own with
//! a factory and serves it until its link goes down. The crate has no async runtime in it: `serve` is a future for
//! the program's own executor, and time comes from a [ww_device::Clock].
//!
//! ```ignore
//! let server = Server::new(TokioClock::new());
//! server
//!     .serve(link_config, listener, |conn: &Connection| MyBackend::new(conn, shared.clone()), shutdown)
//!     .await?;
//! ```
//!
//! A connection's requests run one at a time, in the order they came. While a handler runs, the connection keeps
//! reading, answers pings and sends the events handlers queue, so a slow handler holds up its own connection's
//! requests only.

mod connection;
#[cfg(feature = "in_process")]
pub mod in_process;
mod lifecycle;
mod listener;
mod outbox;
mod server;

pub use connection::Connection;
pub use listener::{Accepted, Listener};
pub use server::{Limits, MakeBackend, Server};
