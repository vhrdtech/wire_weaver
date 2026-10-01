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
//! - [Server] — async glue: `wait()` (cancel-safe, `select` it with anything else) and `handle()`
//!   (link logic + backend), plus a [Sink] to send stream updates from anywhere in the loop.
//! - [blocking::Server] — the same for devices without async: packets are pushed in, time is polled.
//!
//! Medium specific crates (e.g., `wire_weaver_usb_embassy`) only provide packet or byte IO and descriptors.
#![no_std]

mod fmt;

pub mod blocking;
mod buffer;
pub mod link;
#[cfg(feature = "rtt")]
pub mod rtt;
pub mod server;
mod time;
pub mod transport;

#[cfg(test)]
mod tests;

use fmt::error;

pub use buffer::RxBuffer;
pub use link::{DeviceLink, DownReason, LinkConfig, LinkEvent, SendError};
#[cfg(feature = "embassy-time")]
pub use server::EmbassyClock;
pub use server::{Clock, Ready, Server, Sink};
pub use time::Instant;
pub use transport::{
    FramedRx, FramedTx, MessageRx, MessageTx, PacketSink, PacketSource, StreamRx, StreamSink,
    StreamSource, StreamTx,
};
pub use ww_link::{self, DisconnectReason};

/// `err_seq` used in [generic_error_reply], outside of the range used by generated code.
pub const GENERIC_ERROR_SEQ: u32 = u32::MAX;

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
