//! Servers on byte streams (TR-19): TCP, a process's stdio, a local socket, or any stream the program accepted
//! itself, each opened by its [Framing] and routed to the listener of the API it asks for.
//!
//! - [listen] makes a [StreamListener] on any [Incoming] source; [StreamListener::service] adds one per further API
//!   on the same source.
//! - Sources: [handed_in] for streams the program accepts itself, and with feature `tokio` [TcpIncoming] and
//!   [stdio]; `local_socket::LocalSocketIncoming` with feature `local_socket`.
//! - [PreambleFraming] opens connections as the local socket does: the `ww_byte_stream` preamble, then
//!   length-prefixed messages.

mod framing;
mod handed_in;
mod listener;
#[cfg(feature = "tokio")]
mod stdio;
#[cfg(feature = "tokio")]
mod tcp;

pub use framing::{Framing, PreambleFraming};
pub use handed_in::{Gone, HandedIn, StreamSender, handed_in};
pub use listener::{HANDSHAKES_MAX, Incoming, OPENING_TIMEOUT, QUEUED_MAX, StreamListener, listen};
#[cfg(feature = "tokio")]
pub use stdio::{Stdio, stdio};
#[cfg(feature = "tokio")]
pub use tcp::TcpIncoming;
#[cfg(feature = "tokio")]
pub(crate) use tcp::{ACCEPT_RETRY_DELAY, is_transient};
