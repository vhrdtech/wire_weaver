//! A level of `serve_trait_levels_api` defined in a crate of its own, with its local trait `Gauge` (CLT-58): the
//! parent's local trait names it next to the `#[ww_trait]`.
use ww_api::prelude::*;

#[ww_trait(local = "Gauge")]
#[effect(edit)]
pub trait GaugeApi {
    /// The gauge's reading minus its zero
    #[effect(read)]
    fn reading() -> Reading;
    /// What `reading` subtracts
    property!(rw zero: Reading);
    /// The readings 0, 1 and 2
    stream!(readings: Reading);
}

/// A gauge reading.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct Reading(pub u16);

/// `Gauge`, the level as an async trait.
#[cfg(feature = "local")]
mod local {
    wire_weaver::ww_codegen!(crate::GaugeApi, local_trait = "Gauge",);
}
#[cfg(feature = "local")]
pub use local::{Gauge, GaugeDyn};
