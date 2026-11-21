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
//! A connection's requests start in the order they came. Shared calls and reads (`receiver_model`) run side by
//! side, everything else runs alone behind them. While handlers run, the connection keeps reading, answers pings and
//! sends the events handlers queue, so a slow handler holds up its own connection's requests only.
//!
//! Listeners: `in_process` (feature `in_process`) for clients of the same process; [byte_stream] for any byte
//! stream, with several APIs on one source: TCP and stdio (feature `tokio`), streams the program accepted itself,
//! and `local_socket` (feature `local_socket`) for programs of the same user, also on a socket systemd socket
//! activation passed; `udp` (feature `udp`) for hosts on a datagram socket, a connection per host address.
//! [Server::accepting] hands out connections for the program to spawn on a multi-threaded runtime.
//!
//! [Server::authorize] puts an [Authorize] hook in front of every handler (SRV-46): who asked, as the transport
//! saw it, and what the request does where.
//!
//! Between requests a [Backend] does its own work: it sends stream items and observed properties when its
//! [wait](Backend::wait) completes or its [next_wake](Backend::next_wake) comes. A [Fanout] carries what many
//! connections follow at once.

mod authorize;
mod backend;
pub mod byte_stream;
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
mod serves;
mod sync;
#[cfg(feature = "udp")]
pub mod udp;

pub use authorize::{Authorize, Denied, Resource};
pub use backend::Backend;
pub use connection::Connection;
pub use fanout::{Fanout, Lagged, Subscription};
pub use listener::{Accept, Accepted, Listener};
pub use pump::{PumpId, Pumps};
pub use server::{Accepting, Limits, MakeBackend, Server};
pub use serves::Serves;
pub use ww_client_server::target::{Access, Place, TraitRef};
