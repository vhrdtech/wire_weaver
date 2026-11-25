//! The next firmware of the sensor node: its reading has a field more, and its messages a variant more. Old and new
//! nodes and gateways keep reading each other's bytes.

use crate::{Humidity, NodeId, Pressure, Temperature};
use shrink_wrap::prelude::*;

// --8<-- [start:reading]
/// One reading of a weather station, with the air pressure the next firmware adds.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq))]
pub struct Reading<'i> {
    pub station: &'i str,
    pub temperature: Temperature,
    pub humidity: Option<Humidity>,
    pub battery_low: bool,
    /// `None` from a node that has no pressure sensor, or runs the first firmware.
    #[default = None]
    pub pressure: Option<Pressure>,
}
// --8<-- [end:reading]

// --8<-- [start:message]
/// Everything a sensor node of the next firmware sends: `Rain` is new.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq), ww_repr = u4)]
pub enum Message<'i> {
    Hello {
        node: NodeId,
        firmware: &'i str,
    },
    Reading(Reading<'i>),
    Goodbye,
    /// The rain sensor got wet.
    Rain,
}
// --8<-- [end:message]

/// The roof reading of the next firmware: 21.5 °C, 48 %, 1013.2 hPa.
pub const ROOF: Reading<'static> = Reading {
    station: "roof",
    temperature: Temperature::from_raw(2150),
    humidity: Some(Humidity::bounded(48)),
    battery_low: false,
    pressure: Some(Pressure::bounded(10132)),
};
