//! WireWeaver device side, `no_std` and allocation-free.
//!
//! Mirrors the host side (`wire_weaver_client`) in layers, while keeping the event loop in user code:
//!
//! - [link::DeviceLink] — sans-IO link state machine: link setup and version check, frame
//!   accumulation window, pings, peer timeout. Fed messages and time, hands back messages to send.
//! - [transport] — async [MessageTx] / [MessageRx]
//!   traits, with [FramedTx] / [FramedRx] implementing them
//!   over packets for frame based media (USB, CAN, ...) and [StreamTx] /
//!   [StreamRx] over bytes for stream media (RTT, UART, ...).
//! - `rtt` (feature `rtt`) — RTT up / down channels from `rtt-target` as a stream medium.
//! - `ws` (feature `ws`) — WebSocket as a medium over any TCP-like socket, `embassy-net` sockets with the
//!   `embassy-net` feature.
//! - `udp` (feature `udp`) — UDP as a medium over any datagram socket, one frame per datagram, several hosts on one
//!   port; `embassy-net` sockets with the `embassy-net` feature, the standard library's with `std`.
//! - [Server] — async glue: `wait()` (cancel-safe, `select` it with anything else) and `handle()`
//!   (link logic + backend), plus a [Sink] to send stream updates from anywhere in the loop.
//! - [blocking::Server] — the same for devices without async: packets are pushed in, time is polled.
//!
//! Medium specific crates (e.g., `wire_weaver_usb_embassy`) only provide packet or byte IO and descriptors.
#![no_std]

#[cfg(feature = "std")]
extern crate std;

mod fmt;

pub mod blocking;
mod buffer;
pub mod link;
pub mod mount;
#[cfg(feature = "rtt")]
pub mod rtt;
pub mod server;
mod time;
pub mod transport;
#[cfg(feature = "udp")]
pub mod udp;
#[cfg(feature = "ws")]
pub mod ws;

#[cfg(test)]
mod tests;

use fmt::error;

pub use buffer::RxBuffer;
pub use link::{DeviceLink, DownReason, LinkConfig, LinkEvent, PeerMeta, PeerVersion, SendError};
#[cfg(feature = "embassy-time")]
pub use server::EmbassyClock;
pub use server::{Clock, Ready, Server, Sink};
pub use time::Instant;
pub use transport::{
    FramedRx, FramedTx, MessageRx, MessageTx, PacketSink, PacketSource, StreamRx, StreamSink,
    StreamSource, StreamTx,
};
#[cfg(feature = "std")]
pub use transport::{OwnedFramedRx, OwnedFramedTx};
pub use ww_link::{self, DisconnectReason};

/// `err_seq` used in [generic_error_reply], outside of the ranges used by generated code
/// (`ww_client_server::err_seq::GENERIC`).
pub const GENERIC_ERROR_SEQ: u32 = ww_client_server::err_seq::GENERIC;

/// When the backend fails to process a request completely (it could not even serialize an error),
/// reply with a generic error, so that the host does not wait for a timeout.
/// None if the request has no seq (no reply expected) or `scratch` is too small.
pub fn generic_error_reply<'s>(request: &[u8], scratch: &'s mut [u8]) -> Option<&'s [u8]> {
    let seq = ww_client_server::Request::peek_seq(request).ok()?;
    if seq == 0 {
        return None;
    }
    let error = ww_client_server::Error::response_ser_failed(GENERIC_ERROR_SEQ);
    ww_client_server::util::ser_err_event(scratch, seq, error).ok()
}

/// The answer to a request whose reply is longer than the link's message limit (CLT-55,
/// [DeviceLink::agreed_max_message_len]): `ResponseSerFailed` at
/// [REPLY_OVER_LIMIT](ww_client_server::err_seq::REPLY_OVER_LIMIT), which a host shows with both sides' limits.
/// None if the request has no seq (no reply expected) or `scratch` is too small.
pub fn over_limit_reply<'s>(request: &[u8], scratch: &'s mut [u8]) -> Option<&'s [u8]> {
    let seq = ww_client_server::Request::peek_seq(request).ok()?;
    if seq == 0 {
        return None;
    }
    let error =
        ww_client_server::Error::response_ser_failed(ww_client_server::err_seq::REPLY_OVER_LIMIT);
    ww_client_server::util::ser_err_event(scratch, seq, error).ok()
}

/// The answer of a host server that refuses a request for its load (SRV-12): `Overloaded` at
/// [REQUEST_OVERLOADED](ww_client_server::err_seq::REQUEST_OVERLOADED). None if the request has no seq (no reply
/// expected) or `scratch` is too small.
pub fn overloaded_reply<'s>(request: &[u8], scratch: &'s mut [u8]) -> Option<&'s [u8]> {
    let seq = ww_client_server::Request::peek_seq(request).ok()?;
    if seq == 0 {
        return None;
    }
    ww_client_server::util::ser_err_event(scratch, seq, ww_client_server::Error::overloaded()).ok()
}

/// `request`, or the request `rewrite` wrote into `scratch` in its place (a global path into a mount slot,
/// [WireWeaverAsyncApiBackend::global_into_mount](wire_weaver::WireWeaverAsyncApiBackend::global_into_mount)), moved
/// to the end of `scratch`; and the rest of `scratch` for the reply. A rewritten request answers in events from the
/// slot, so its reply is short.
pub(crate) fn into_mount<'r>(
    request: &'r [u8],
    scratch: &'r mut [u8],
    rewrite: impl for<'b> FnOnce(&[u8], &'b mut [u8]) -> Option<&'b [u8]>,
) -> (&'r [u8], &'r mut [u8]) {
    let (base, size) = (scratch.as_ptr() as usize, scratch.len());
    let found = rewrite(request, scratch).and_then(|rewritten| sub_range(base, size, rewritten));
    let Some((start, len)) = found.filter(|(_, len)| *len > 0) else {
        return (request, scratch);
    };
    let Some(at) = scratch.len().checked_sub(len) else {
        return (request, scratch);
    };
    if start.checked_add(len).is_none_or(|end| end > scratch.len()) {
        return (request, scratch);
    }
    scratch.copy_within(start..start + len, at);
    let (reply, rewritten) = scratch.split_at_mut(at);
    (rewritten, reply)
}

/// Where `part`, a slice of the buffer of `len` bytes at the address `base`, starts in it, and its length; `None`
/// when it lies outside of the buffer.
pub(crate) fn sub_range(base: usize, len: usize, part: &[u8]) -> Option<(usize, usize)> {
    if part.is_empty() {
        return Some((0, 0));
    }
    let start = (part.as_ptr() as usize).checked_sub(base)?;
    let end = start.checked_add(part.len())?;
    (end <= len).then_some((start, part.len()))
}

/// Priority of a serialized request, [Priority::NORMAL](wire_weaver::Priority::NORMAL) when it can't be read
/// (the backend answers such a request with an error anyway).
pub(crate) fn request_priority(request: &[u8]) -> wire_weaver::Priority {
    ww_client_server::Request::peek_priority(request).unwrap_or_default()
}

/// Seq numbers for each of `repeat` loopback replies: one echo keeps the host's seq, several are numbered from 0.
pub fn loopback_seqs(repeat: u32, seq: u32) -> impl Iterator<Item = u32> {
    let first = if repeat == 1 { seq } else { 0 };
    (0..repeat).map(move |i| first.wrapping_add(i))
}

/// Encode one loopback reply into `scratch`.
pub fn loopback_reply<'s>(seq: u32, data: &[u8], scratch: &'s mut [u8]) -> Option<(u8, &'s [u8])> {
    // Message::encode would return data as is, but it's borrowed from rx and must outlive scratch,
    // so copy it in: | repeat: u32 LE | seq: u32 LE | data |, repeat = 0 for replies
    let Some(out) = scratch.get_mut(..8 + data.len()) else {
        error!("scratch buffer is too small for loopback");
        return None;
    };
    out[..4].copy_from_slice(&0u32.to_le_bytes());
    out[4..8].copy_from_slice(&seq.to_le_bytes());
    out[8..].copy_from_slice(data);
    Some((ww_link::Kind::Loopback as u8, out))
}
