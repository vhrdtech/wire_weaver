//! GPIO control and remote bridging API: banks of pins (`Bank`), each with individually addressable pins (`Pin`)
//! that can be read, driven, reconfigured (mode / pull / drive strength) and watched for edge events through a
//! stream. See `docs/std_library/ww_gpio.md` for the full writeup with client and server examples.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::Volt;

/// A bank of related IO pins.
/// Each pin in a bank is using the same reference voltage, that can be adjusted if supported by hardware.
#[ww_trait]
pub trait Bank {
    /// Array of individual pins.
    /// Suggested indexing scheme:
    /// * Range - useful when a device has sequentially marked IOs (e.g., IO expander)
    /// * List - when exposing some pins of an MCU, (e.g., PB1 and PB5, having indices 1 and 5 is less confusing than 0 and 1)
    ww_impl!(pin[]: Pin);

    reserved!(1..=7);

    /// Capabilities that each pin of the bank supports.
    fn capabilities() -> BankCapabilities<'i>;

    /// Reference voltage for all pins of the bank.
    ///
    /// # Errors
    /// `Error::UnsupportedReferenceVoltage` if the bank's hardware can't drive the requested voltage (`set` only).
    property!(rw reference_voltage: Volt, Error);

    /// User-friendly bank name, e.g. `"PA"`.
    fn name() -> &'i str;
}

/// One pin from a GPIO bank.
///
/// Commonly used operations are defined first, to get more compact resource paths.
#[ww_trait]
pub trait Pin {
    /// 0. Set the output level.
    ///
    /// If the pin is currently configured as input, this level should only be written to control register, without changing pin mode.
    fn set_output_level(level: Level);

    /// 1. Toggle the output level.
    fn toggle();

    /// 2. Get output level.
    ///
    /// If the pin is currently configured as input, this level should be read from an output register, not input one.
    fn output_level() -> Level;

    /// 3. Get the input level.
    ///
    /// Note that when a pin is configured as output, input buffer might be disabled, resulting in incorrect input level reported.
    /// If this is the case, current output level must be returned.
    fn input_level() -> Level;

    reserved!(4..=6);

    /// 7. Asynchronous stream of events (rising / falling edge), if enabled by `configure_events`.
    stream!(event: IoPinEvent);

    /// Mode configuration, input, high-z, push-pull, open-drain or custom.
    /// Optionally set the initial level before changing pin mode.
    ///
    /// # Errors
    /// `Error::UnsupportedMode` if the pin can't be switched to the requested mode.
    fn set_mode(mode: Mode, initial: Option<Level>) -> Result<(), Error>;
    /// Get current mode.
    fn mode() -> Mode;

    /// Pull resistors selection.
    ///
    /// # Errors
    /// `Error::UnsupportedPull` if the pin doesn't support the requested pull (`set` only).
    property!(rw pull: Pull, Error);

    /// Drive strength selection.
    ///
    /// # Errors
    /// `Error::UnsupportedSpeed` if the pin doesn't support the requested drive strength (`set` only).
    property!(rw speed: Speed, Error);

    /// Enable or disable asynchronous events sent through the `event` stream.
    ///
    /// # Errors
    /// `Error::UnsupportedEventType` if a requested source (edge or custom) isn't available on this pin.
    fn configure_events(enabled: IoPinEnabledEvents<'i>) -> Result<(), Error>;

    /// What this pin can do: modes, pulls, speeds, whether it sends events, and its name.
    fn capabilities() -> PinCapabilities<'i>;
}

/// What one pin can do, see `Pin::capabilities`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug)
)]
pub struct PinCapabilities<'i> {
    /// Modes the pin can be switched to with `Pin::set_mode`.
    pub modes: MaxLen<RefVec<'i, Mode>, 8>,
    /// Pull configurations the pin's `pull` property accepts.
    pub pulls: MaxLen<RefVec<'i, Pull>, 4>,
    /// Drive strengths the pin's `speed` property accepts.
    pub speeds: MaxLen<RefVec<'i, Speed>, 5>,
    /// The `event` stream works on this pin.
    pub events: bool,
    /// Name printed on the board or in the datasheet: "IO4", "PB5".
    pub name: MaxLen<&'i str, 16>,
}

/// Digital output level - High and Low.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1
)]
pub enum Level {
    Low,
    High,
}

/// IO pin mode (Push-Pull, Open-Drain, Input, etc.)
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Mode {
    /// Driven output, both high and low levels actively sourced/sunk.
    PushPullOutput,
    /// Driven output that only actively sinks low; a pull-up (internal or external) is needed for a high level.
    OpenDrainOutput,
    /// High-impedance input, level read back through `Pin::input_level`.
    Input,
    /// Electrically the same as Input, but could lower power consumption by disabling input buffer
    HighZ,
    /// Analog mode, e.g. for an ADC or DAC channel sharing the pin.
    Analog,
    /// Device-specific mode not covered above, e.g. an alternate function.
    Custom(u8),
}

/// GPIO error
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug),
    ww_repr = unib32
)]
pub enum Error {
    /// `Pin::set_mode` was called with a `Mode` this pin doesn't support.
    UnsupportedMode,
    /// `Pin::pull` was set to a `Pull` this pin doesn't support.
    UnsupportedPull,
    /// `Pin::speed` was set to a `Speed` this pin doesn't support.
    UnsupportedSpeed,
    /// `Pin::configure_events` requested an edge or custom source this pin can't generate.
    UnsupportedEventType,
    /// `Bank::reference_voltage` was set to a voltage this bank can't drive.
    UnsupportedReferenceVoltage,
    /// Pins of a bank were requested to use different modes when only one mode at a time is supported.
    DifferentModes,
    /// The method or property exists on the device but isn't implemented yet.
    NotImplemented,
    /// Device-specific error code, 8-bit.
    CustomU8(u8),
    /// Device-specific error code, 32-bit.
    CustomU32(u32),
    /// The pin serves another function.
    InUse,
    /// The level translators have no supply (IO level off).
    NoIoLevel,
    /// A safety rule of the device doesn't allow it.
    Refused,
}

/// IO pin pull configuration (pull-up, pull-down, etc.)
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = u2
)]
pub enum Pull {
    /// No pull resistor, floating when not driven.
    None,
    /// Internal pull-up resistor enabled.
    Up,
    /// Internal pull-down resistor enabled.
    Down,
    /// Device-specific pull configuration not covered above.
    Custom(u8),
}

/// IO pin drive strength configuration
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Speed {
    /// Slowest slew rate, least EMI, lowest achievable toggle frequency.
    Slow,
    Medium,
    Fast,
    /// Fastest slew rate, most EMI, highest achievable toggle frequency.
    VeryFast,
    /// Device-specific drive strength not covered above.
    Custom(u8),
}

/// IO pin asynchronous event (interrupt reason), sent via the `Pin::event` stream if enabled.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = u2
)]
// TODO: Add optional timestamp?
pub enum IoPinEvent {
    /// The pin's input went from low to high.
    RisingEdge,
    /// The pin's input went from high to low.
    FallingEdge,
    /// Device-specific event source not covered above.
    Custom(u8),
}

/// List of enabled event sources for an IO pin (interrupts) that generate the `Pin::event` stream.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug)
)]
pub struct IoPinEnabledEvents<'i> {
    /// Report `IoPinEvent::RisingEdge`.
    pub rising: bool,
    /// Report `IoPinEvent::FallingEdge`.
    pub falling: bool,
    /// Indices of device-specific sources to report as `IoPinEvent::Custom`.
    pub custom: RefVec<'i, u8>,
    // TODO: add GPIO filter configuration
}

/// GPIO bank capabilities: supported voltages, modes, custom modes, etc.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug)
)]
pub struct BankCapabilities<'i> {
    /// Reference voltages the bank can be set to with `Bank::reference_voltage`.
    pub voltage: RefVec<'i, Volt>,

    /// At least one pin of the bank supports `Mode::PushPullOutput`.
    pub push_pull: bool,
    /// At least one pin of the bank supports `Mode::OpenDrainOutput`.
    pub open_drain: bool,
    /// At least one pin of the bank supports `Mode::Input`.
    pub input: bool,
    /// Pins of the bank can be put in different modes independently; otherwise every pin shares one mode.
    pub individually_configurable_pins: bool,

    /// Descriptions of the bank's `Mode::Custom` variants, indexed by their `u8` payload.
    pub custom_mode: RefVec<'i, &'i str>,
    /// Descriptions of the bank's `Pull::Custom` variants, indexed by their `u8` payload.
    pub custom_pull: RefVec<'i, &'i str>,
    /// Descriptions of the bank's `Speed::Custom` variants, indexed by their `u8` payload.
    pub custom_speed: RefVec<'i, &'i str>,
}

impl Mode {
    /// `true` for `PushPullOutput` and `OpenDrainOutput`, the modes that actively drive the output register.
    pub fn is_driving(&self) -> bool {
        matches!(self, Mode::PushPullOutput | Mode::OpenDrainOutput)
    }
}

impl Level {
    /// `true` for `Level::High`.
    pub fn is_high(&self) -> bool {
        matches!(*self, Level::High)
    }

    /// `true` for `Level::Low`.
    pub fn is_low(&self) -> bool {
        matches!(*self, Level::Low)
    }
}
