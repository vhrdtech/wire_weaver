//! WireWeaver over local sockets (TR-16): how a program reaches a device served by another process on the same PC,
//! most of all the ww hub (SRV-13), which holds the devices and serves every program of the user.
//!
//! - [runtime_dir]: the per-user directory sockets and lock files live in, user-only.
//! - [listen] / [connect]: a Unix domain socket on Linux and macOS, a named pipe on Windows (through `interprocess`),
//!   reachable by the same user only. Both ends check who is on the other side: a socket or pipe someone else made
//!   (squatting a name) is refused, not talked to.
//! - [inherited]: on Unix, the listening socket a service manager passed (systemd socket activation, `LISTEN_FDS`).
//! - [HandoverRequest] / [HandoverAnswer]: what a program asks a direct holder of a device on the holder's socket
//!   (CLT-43): hand it to the hub. Frozen like the preamble.
//!
//! The preamble and the message framing on these sockets are `ww_byte_stream`'s, as on any byte stream.

#[cfg(all(unix, feature = "socket"))]
mod activation;
mod dir;
pub mod files;
#[cfg(feature = "socket")]
mod handover;
#[cfg(feature = "socket")]
mod socket;

#[cfg(all(unix, feature = "socket"))]
pub use activation::{ActivationError, inherited};
pub use dir::{RUNTIME_DIR_ENV, runtime_dir};
#[cfg(feature = "socket")]
pub use handover::{
    HANDOVER_MAGIC, HANDOVER_VERSION, HandoverAnswer, HandoverError, HandoverRequest,
    HandoverStatus, MAX_HANDOVER_TRAILER_LEN, MAX_META_LEN, Op, TAG_META,
};
#[cfg(feature = "socket")]
pub use socket::{Listener, Peer, ReadHalf, Stream, WriteHalf, connect, listen};
