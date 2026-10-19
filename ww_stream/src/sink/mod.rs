//! Credit sinks on the stream protocol (API-34): the host sends items to the device, the device grants room in its
//! buffer and acknowledges what it accepted, with the messages reliable streams use the other way round
//! ([StreamSideband::Ack], [StreamSideband::Query], [StreamSideband::ResendFrom]). See `docs/api/credit_sinks.md`.
//!
//! | | |
//! |---|---|
//! | [SinkCredit] | device, the receiver: accepts writes in order, grants bytes of its buffer, asks for lost writes |
//! | [SinkTx] | host, the sender: keeps a copy of what isn't acknowledged, never sends beyond the grant, go-back-N (`alloc`) |
//!
//! **Positions count units**, which the sender names for each item: 1 per item by default, the bytes of its payload
//! for a byte sink (a UART's tx), so a [StreamCursor] means something to user code and a restarted host knows where
//! to continue in its own data. Every item has at least one unit. **Grants count bytes** of items as written (the
//! serialized value), since what runs out on the device is its buffer: [SinkCredit::sized_for] checks that the
//! largest item fits.
//!
//! **Positions on the wire** are taken modulo [POS_MODULUS] (2^28, at most 4 bytes as a `UVlq32`), and the receiver
//! gets the full `u64` position back from the one it expects ([extend_pos], serial number arithmetic as in TCP), so a
//! session has no size limit. That works while every position on the wire is less than 2^27 away from what the
//! receiver expects: the host never has more than [MAX_WINDOW] bytes nor [MAX_WINDOW] units in flight.
//!
//! **The sender names the session**: the host opens every session with a [StreamCursor], a new session at position 0,
//! or its own session again after a reconnect or a restart. The device keeps a session it is in (a repeated open,
//! also a late duplicate on UDP, changes nothing) and starts at 0 for any other.
//!
//! [StreamSideband::Ack]: ww_client_server::StreamSideband::Ack
//! [StreamSideband::Query]: ww_client_server::StreamSideband::Query
//! [StreamSideband::ResendFrom]: ww_client_server::StreamSideband::ResendFrom

mod credit;
#[cfg(any(feature = "alloc", test))]
mod tx;

pub use credit::{SinkCredit, SinkCreditError, SinkVerdict};
#[cfg(any(feature = "alloc", test))]
pub use tx::{SinkConfig, SinkDue, SinkOpenError, SinkSideband, SinkTx, SinkTxError, SinkWriteOut};

#[allow(unused_imports)] // doc links
use crate::StreamCursor;

/// Positions of credit sinks go on the wire modulo 2^28: the most a `UVlq32` holds in 4 bytes
/// ([RequestKind::SinkWrite](ww_client_server::RequestKind::SinkWrite)).
pub const POS_MODULUS: u32 = 1 << 28;

/// Largest credit window: the most bytes a sink lets the host have in flight (its capacity), and the most units the
/// host puts in flight: 64 MiB, a quarter of [POS_MODULUS].
///
/// A position on the wire is read right when it is less than `POS_MODULUS / 2` units away from the one the receiver
/// expects. Everything in flight lies within one window around it, so two windows take half of the modulus, the
/// other half is the margin for a late packet (a duplicate delayed on UDP): one that arrives more than 64 Mi units
/// late looks like a write from ahead (a gap, answered with a resend), only 256 Mi units late it would look like the
/// expected one. [SinkCredit] refuses a larger capacity; the host never puts more than this in flight, whatever the
/// device grants.
pub const MAX_WINDOW: u32 = POS_MODULUS / 4;

/// A position as sent on the wire: modulo [POS_MODULUS].
pub const fn wire_pos(pos: u64) -> u32 {
    (pos % POS_MODULUS as u64) as u32
}

/// Full position of the wire position `wire`, the one nearest to `near` (the position the receiver expects): from
/// `near - 2^27` to `near + 2^27 - 1`. None when `wire` isn't below [POS_MODULUS] (an invalid one), or when the
/// position falls before the start of the session (a late packet from a previous one).
pub const fn extend_pos(near: u64, wire: u64) -> Option<u64> {
    if wire >= POS_MODULUS as u64 {
        return None;
    }
    let delta = (wire as u32).wrapping_sub(near as u32) & (POS_MODULUS - 1);
    let delta = if delta >= POS_MODULUS / 2 {
        delta as i64 - POS_MODULUS as i64
    } else {
        delta as i64
    };
    near.checked_add_signed(delta)
}
