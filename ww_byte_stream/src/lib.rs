//! WireWeaver over a byte stream (TR-19): a local socket, TCP, a process's stdio or any stream a program accepted.
//!
//! - [Preamble] / [Answer]: the first bytes of every connection, they pick the hub's own API or a device.
//!   Frozen: a client and a server of any two versions can always tell each other what they are.
//! - [MessageReader] / [MessageWriter]: after the preamble, `ww_link` messages as `| len: u32 LE | kind: u8 |
//!   payload |`, `len` counting the kind byte and the payload. Link setup, versions, pings and timeouts are
//!   `ww_link`'s, the same as over WebSocket.

mod framing;
mod preamble;

pub use framing::{MessageReader, MessageWriter};
pub use preamble::{
    Answer, AnswerError, DISCONNECT_LENT, Generations, MAGIC, MAX_KEY_LEN, MAX_TRAILER_LEN,
    PREAMBLE_VERSION, Preamble, PreambleError, Status, TAG_ADOPT, TAG_API_MODEL_VERSION,
    TAG_GENERATIONS, TAG_LINK_VERSION, Target, Version,
};
