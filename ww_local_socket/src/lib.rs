//! WireWeaver over local sockets (TR-16): how a program reaches a device served by another process on the same PC,
//! most of all the ww hub (SRV-13), which holds the devices and serves every program of the user.
//!
//! - [runtime_dir]: the per-user directory sockets and lock files live in, user-only.
//! - [listen] / [connect]: a Unix domain socket on Linux and macOS, a named pipe on Windows (through `interprocess`),
//!   reachable by the same user only. Both ends check who is on the other side: a socket or pipe someone else made
//!   (squatting a name) is refused, not talked to.
//! - [Preamble] / [Answer]: the first bytes of every connection, they pick the hub's own API or one of its devices.
//!   Frozen: a client and a hub of any two versions can always tell each other what they are.
//! - [MessageReader] / [MessageWriter]: after the preamble, `ww_link` messages as `| len: u32 LE | kind: u8 |
//!   payload |`, `len` counting the kind byte and the payload. Link setup, versions, pings and timeouts are
//!   `ww_link`'s, the same as over WebSocket.

mod dir;
mod framing;
mod preamble;
mod socket;

pub use dir::{RUNTIME_DIR_ENV, runtime_dir};
pub use framing::{MessageReader, MessageWriter};
pub use preamble::{
    Answer, AnswerError, MAGIC, MAX_KEY_LEN, PREAMBLE_VERSION, Preamble, PreambleError, Status,
    Target,
};
pub use socket::{Listener, Peer, Stream, connect, listen};
