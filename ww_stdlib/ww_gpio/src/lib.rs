//! GPIO control and remote bridging API: banks of pins (`Bank`), each with individually addressable pins (`Pin`)
//! that can be read, driven, reconfigured (mode / pull / drive strength) and watched for edge events through a
//! stream. See `docs/std_library/ww_gpio.md` for the full writeup with client and server examples.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::Volt;

/// Most discrete IO levels a bank lists in [BankCapabilities::io_levels].
pub const MAX_IO_LEVELS: usize = 8;

/// A bank of related IO pins.
/// Every pin of a bank uses the same IO level, adjustable on hardware with level translators ([BankConfig]).
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

    /// Configures what the pins of the bank share and returns what was actually set: the IO level in effect.
    /// `Unsupported` for an IO level the bank's hardware can't drive, `Refused` when a safety rule of the device
    /// doesn't allow it.
    #[errors(Common(Unsupported, Refused))]
    fn configure(config: BankConfig) -> Result<BankConfig, GpioError<'i>>;

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

    // TODO(STD-31): `property!(ro+observe level: Level)` here, once levels under a resource array can be observed
    // (API-29).
    reserved!(4..=6);

    /// 7. Edges (and device-specific sources) of this pin, as they happen: opening the stream enables the sources
    /// in `enabled`, closing it disables them. A counted stream, unit: event; each event may carry the device time
    /// it happened at, and the host learns exactly how many events the device dropped.
    /// `Unsupported` if a requested source (edge or custom) isn't available on this pin.
    #[errors(Common(Unsupported))]
    #[stream(IoPinEvent, counted)]
    fn event(enabled: IoPinEnabledEvents<'i>) -> Result<(), GpioError<'i>>;

    /// Mode configuration, input, high-z, push-pull, open-drain or custom.
    /// Optionally set the initial level before changing pin mode.
    /// `Unsupported` if the pin can't be switched to the requested mode, `DifferentModes` if the bank's pins share one
    /// mode and this one differs.
    #[errors(Common(Unsupported, InUse, NoPower, Refused), DifferentModes)]
    fn set_mode(mode: Mode, initial: Option<Level>) -> Result<(), GpioError<'i>>;
    /// Get current mode.
    fn mode() -> Mode;

    /// Pull resistors selection.
    /// `Unsupported` if the pin doesn't support the requested pull (`set` only).
    #[errors(Common(Unsupported))]
    property!(rw pull: Pull, GpioError<'i>);

    /// Drive strength selection.
    /// `Unsupported` if the pin doesn't support the requested drive strength (`set` only).
    #[errors(Common(Unsupported))]
    property!(rw speed: Speed, GpioError<'i>);

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

/// Errors of `Bank` and `Pin`: the common ones first (unsupported mode, pull, speed, event or IO level; in use; no
/// power; refused), then what is specific to GPIO. Each method lists the variants it returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum GpioError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
    /// The pins of this bank share one mode, and another pin is in a different one.
    DifferentModes,
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

/// IO pin asynchronous event (interrupt reason), sent on the `Pin::event` stream; when it happened is in the event's
/// head.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = u2
)]
pub enum IoPinEvent {
    /// The pin's input went from low to high.
    RisingEdge,
    /// The pin's input went from high to low.
    FallingEdge,
    /// Device-specific event source not covered above.
    Custom(u8),
}

/// Event sources of an IO pin (interrupts) to report on the `Pin::event` stream, its argument.
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

/// Everything a bank is configured with, see `Bank::configure`: what is asked for going in, what was actually set
/// coming back.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug)
)]
pub struct BankConfig {
    /// IO level of every pin of the bank, on hardware with level translators ([BankCapabilities::io_levels]); `None`
    /// leaves it as it is going in (the only choice on a bank without them), and is the answer of such a bank.
    pub io_level: Option<Volt>,
}

/// GPIO bank capabilities: supported IO levels, modes, custom modes, etc.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug)
)]
pub struct BankCapabilities<'i> {
    /// Discrete IO levels [BankConfig::io_level] can be set to (e.g. 1.8 V, 3.3 V, 5 V); empty if the bank has no
    /// level control.
    pub io_levels: MaxLen<RefVec<'i, Volt>, MAX_IO_LEVELS>,
    /// Lowest IO level of an adjustable supply, if the bank has one.
    pub io_level_min: Option<Volt>,
    /// Highest IO level of an adjustable supply, if the bank has one.
    pub io_level_max: Option<Volt>,

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
