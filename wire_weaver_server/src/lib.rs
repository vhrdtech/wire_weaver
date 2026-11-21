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
//!
//! Between requests a [Backend] does its own work: it sends stream items and observed properties when its
//! [wait](Backend::wait) completes or its [next_wake](Backend::next_wake) comes. A [Fanout] carries what many
//! connections follow at once.

mod backend;
mod connection;
mod fanout;
#[cfg(feature = "in_process")]
pub mod in_process;
mod lifecycle;
mod listener;
#[cfg(feature = "local_socket")]
pub mod local_socket;
mod outbox;
mod pump;
mod server;
mod sync;

pub use backend::Backend;
pub use connection::Connection;
pub use fanout::{Fanout, Lagged, Subscription};
pub use listener::{Accepted, Listener};
pub use pump::{PumpId, Pumps};
pub use server::{Accepting, Limits, MakeBackend, Server};
