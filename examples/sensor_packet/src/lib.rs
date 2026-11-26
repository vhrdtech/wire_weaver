//! A weather station's sensor node and its gateway, talking over a radio link with `shrink_wrap` alone: no API
//! trait and no framework. The book's chapters "Your data on the wire" use it.
//!
//! The node is a microcontroller without an allocator: it writes a [Reading] into a buffer of its own and hands the
//! bytes to its radio. The gateway is a PC: it reads the bytes back and keeps the readings as [ReadingOwned].
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
pub mod radio;
pub mod v2;

use shrink_wrap::prelude::*;
use ww_si::origins::Celsius;
use ww_si::prefix::{Centi, Deca};
use ww_si::units::{Kelvin, Pascal, Ratio};
use ww_si::{Fixed, Point, Range};

// --8<-- [start:units]
/// A temperature in steps of 0.01 °C: an `i16` on the wire.
pub type Temperature = Point<Celsius, Fixed<Kelvin, Centi, i16>>;

/// A relative humidity in percent, from 0 to 100: a `u8` on the wire.
pub type Humidity = Fixed<Ratio, Centi, u8, Range<0, 100>>;
// --8<-- [end:units]

/// An air pressure in steps of 10 Pa: a `u16` on the wire.
pub type Pressure = Fixed<Pascal, Deca, u16>;

// --8<-- [start:reading]
/// One reading of a weather station, as its sensor node sends it over the radio.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq))]
pub struct Reading<'i> {
    /// Where the station stands, such as "roof".
    pub station: &'i str,
    pub temperature: Temperature,
    /// `None` when the humidity sensor did not answer.
    pub humidity: Option<Humidity>,
    pub battery_low: bool,
}
// --8<-- [end:reading]

// --8<-- [start:node_id]
/// The number of a sensor node on the radio link.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct NodeId(pub u8);
// --8<-- [end:node_id]

// --8<-- [start:message]
/// Everything a sensor node sends over the radio.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq), ww_repr = u4)]
pub enum Message<'i> {
    /// Sent once after power-up.
    Hello { node: NodeId, firmware: &'i str },
    /// A new reading.
    Reading(Reading<'i>),
    /// The battery is about to run out; the node stops sending.
    Goodbye,
}
// --8<-- [end:message]

/// The reading the chapters send: 21.5 °C and 48 % on the roof, the battery fine.
pub const ROOF: Reading<'static> = Reading {
    station: "roof",
    temperature: Temperature::from_raw(2150),
    humidity: Some(Humidity::bounded(48)),
    battery_low: false,
};

/// The longest message a node sends, in bytes: the radio's frame size.
pub const FRAME_BYTES_MAX: usize = 32; // spirit-level: allow(exact-units-quantity) a buffer length in the code

// --8<-- [start:send]
/// Writes `reading` into `frame` as the node sends it: no allocator, the bytes stay in `frame`.
pub fn node_sends<'f>(
    reading: &Reading<'_>,
    frame: &'f mut [u8],
) -> Result<&'f [u8], ShrinkWrapError> {
    reading.to_ww_bytes(frame)
}
// --8<-- [end:send]

/// Lowercase hex bytes separated by spaces, as the book prints them.
#[cfg(feature = "std")]
pub fn hex(data: &[u8]) -> String {
    let hex: Vec<String> = data.iter().map(|b| format!("{b:02x}")).collect();
    hex.join(" ")
}
