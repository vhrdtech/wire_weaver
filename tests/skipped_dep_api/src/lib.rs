//! Stands in for a standard library crate (R7) in `wire_weaver_core`'s snapshot tests: an API using these traits
//! leaves their definitions out against this crate's snapshot. `Port` refers to `ww_si` quantities (which refer to
//! `ww_numeric` and `exact_units`) and to `ww_error`'s `CommonError`, as `ww_uart::Uart` does; `Bank` refers to
//! `Pin`, as `ww_gpio::Bank` does.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;
pub use ww_si::{Second, Volt};

/// A port with a configurable line level.
#[ww_trait]
pub trait Port {
    /// Applies `config`, answers what was set.
    #[errors(CommonError(Unsupported, InUse))]
    fn configure(config: PortConfig) -> PortConfig;
    /// Holds the line low for `duration`.
    fn send_break(duration: Second);
}

/// A bank of pins.
#[ww_trait]
pub trait Bank {
    /// The pins of the bank.
    ww_impl!(pin[]: Pin);
    /// Name of the bank.
    fn name() -> &'i str;
}

/// One pin of a [Bank].
#[ww_trait]
pub trait Pin {
    /// Drives the pin.
    fn set_level(level: Level);
    /// Level the pin reads.
    #[errors(CommonError(Unsupported))]
    fn level() -> Level;
}

/// Settings of a [Port].
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct PortConfig {
    /// Line voltage; `None` leaves it as it is.
    pub io_level: Option<Volt>,
}

/// Level of a [Pin].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = u1
)]
pub enum Level {
    /// Low.
    Low,
    /// High.
    High,
}
