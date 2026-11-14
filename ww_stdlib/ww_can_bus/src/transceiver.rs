use wire_weaver::prelude::*;
use ww_si::Ohm;

/// Transceiver mode, see `Can::set_transceiver`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TransceiverMode {
    Normal,
    /// Receiver on at low power (wake-up detection), driver off: the standby pin.
    Standby,
    /// Receiver on, driver off: the silent pin of transceivers that have one.
    Silent,
    /// Lowest power; woken by the bus (wake-up pattern) or the device.
    Sleep,
}

/// Transceiver mode and faults, see `Can::transceiver_state`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct TransceiverState {
    pub mode: TransceiverMode,
    /// A bus wiring fault the transceiver reports, if any (and if it can).
    pub fault: Option<TransceiverFault>,
}

/// A bus wiring fault (as SocketCAN's `CAN_ERR_TRX_*`).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TransceiverFault {
    CanHighNoWire,
    CanHighShortToBattery,
    CanHighShortToSupply,
    CanHighShortToGround,
    CanLowNoWire,
    CanLowShortToBattery,
    CanLowShortToSupply,
    CanLowShortToGround,
    CanLowShortToCanHigh,
    /// TX held dominant too long, the driver was switched off.
    DominantTimeout,
    /// Over temperature.
    Thermal,
}

/// A terminator setting, see `Can::set_termination`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq),
    ww_repr = nib
)]
pub enum Termination {
    Off,
    On(Ohm),
}

/// What `Can::test_pins` drives the TX pin with.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TxPin {
    /// The controller drives it.
    Controller,
    /// The controller's sample point signal (for measuring the bit timing).
    SamplePoint,
    Dominant,
    Recessive,
}

/// Level on the bus.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1,
    no_future_version
)]
pub enum BusLevel {
    Dominant,
    Recessive,
}

/// Which [TransceiverMode]s besides `Normal` a transceiver has.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct TransceiverModes {
    pub standby: bool,
    pub silent: bool,
    pub sleep: bool,
}
