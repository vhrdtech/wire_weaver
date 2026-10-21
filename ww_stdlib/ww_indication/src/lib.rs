#![cfg_attr(not(feature = "std"), no_std)]
//! Device indication control (STD-28): one global mode every indicator (LED, buzzer, ...) follows.
//!
//! [Indication::mode] is the only item a device must implement. Per-unit [brightness](Indication::brightness),
//! [identify](Indication::identify) (blink everything to find the unit on a bench) and
//! [NAMES](Indication::NAMES), and [set_mode_with_epoch](Indication::set_mode_with_epoch) to animate in phase with
//! other devices, are [optional](../api/optional_items.md): a device leaves out what its hardware or
//! flash budget doesn't support.

use wire_weaver::prelude::*;
pub use ww_si::Second;

/// Device indication control: a global mode every indicator follows, with optional brightness, identify and
/// names.
#[ww_trait]
pub trait Indication {
    /// Global operation mode every indicator follows.
    property!(rw mode: Mode);

    /// Brightness of all indicators; one setting covers every one of them. Left out by a device whose
    /// indicators have no adjustable brightness.
    #[optional]
    property!(rw brightness: Brightness);

    /// Blinks every indicator for `duration` so the unit can be found on a bench or in a rack, then returns to
    /// `mode`. `Unsupported` on a device with no way to blink its indicators, `Busy` while already identifying.
    #[optional]
    #[errors(Unsupported, Busy)]
    fn identify(duration: Second) -> Result<(), CommonError<'i>>;

    /// Indicator names, in the device's own order. Left out (empty on devices that implement it with nothing to
    /// name) to save flash.
    #[optional]
    const NAMES: RefVec<'i, &'i str>;

    /// Writes `mode` as the property does, with `epoch` as the start of every animation's cycle (STD-50): a
    /// breath or blink of period `P` shows `(t - epoch) mod P` at shared time `t` (see `ww_timebase::Clock`,
    /// `SyncFormat::Host`), so devices given the same mode and epoch breathe and blink in phase. `epoch` is in
    /// the prefix and storage of the device's `Clock::now()`, and may lie in the past. Calling it again with the
    /// mode unchanged only moves the phase. A device whose clock runs free (no shared time yet) starts its
    /// animations now, as writing `mode` does. Left out by a device without a shared clock.
    #[optional]
    fn set_mode_with_epoch(mode: Mode, epoch: Second);
}

/// Global operation mode, see [Indication::mode].
///
/// `Unsized` (default): a later version may add a mode (e.g. a "Demo" blink pattern) without breaking a host
/// built against this one, which reads it as `Unknown(u32)` (SW-47).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Mode {
    /// Every indicator off, including alerts.
    AllOff,
    /// Only indicators that must stay visible (faults, power) are on.
    OnlyEssential,
    /// Every indicator behaves as designed.
    Normal,
}

/// Indicator brightness, see [Indication::brightness].
///
/// `Unsized` (default), for the same reason as [Mode]: room for a level between two of these without breaking
/// older hosts.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Brightness {
    /// Night mode, non-distracting.
    Lowest,
    /// Day mode, non-distracting.
    Low,
    /// Good visibility inside.
    Normal,
    /// For alerts inside.
    High,
    /// Visible outside.
    Highest,
}
