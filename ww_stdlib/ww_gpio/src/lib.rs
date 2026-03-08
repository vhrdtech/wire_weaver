#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::Volt;

/// A bank of related IO pins.
/// Each pin in a bank is using the same reference voltage, that can be adjusted if supported by hardware.
#[ww_trait]
trait Bank {
    /// Array of individual pins.
    /// Suggested indexing scheme:
    /// * Range - useful when a device has sequentially marked IOs (e.g., IO expander)
    /// * List - when exposing some pins of an MCU, (e.g., PB1 and PB5, having indices 1 and 5 is less confusing than 0 and 1)
    ww_impl!(pin[]: Pin);

    // 1-7. Reserved
    reserved!();
    reserved!();
    reserved!();
    reserved!();
    reserved!();
    reserved!();
    reserved!();

    /// Capabilities that each pin of the bank supports.
    fn capabilities() -> BankCapabilities<'i>;

    /// Reference voltage currently in use.
    fn reference_voltage() -> Volt;
    /// Set reference voltage to the requested value.
    fn set_reference_voltage(quantity: Volt) -> Result<(), Error>;

    /// Mode configuration for all pins of the bank.
    /// If GpioBankCapabilities::individually_configurable_pins is false, this is the only way to reconfigure bank mode.
    /// If true, then setting mode here must change all pins mode?
    fn set_mode(mode: Mode, initial: Option<Level>) -> Result<(), Error>;
    /// Get current mode of all pins, if individually_configurable_pins is false.
    /// Returns DifferentModes error if individually_configurable_pins is true and pins have different modes configured.
    fn mode() -> Result<Mode, Error>;
    /// Drive strength selection for all pins of the bank.
    fn set_speed(speed: Speed) -> Result<(), Error>;
    /// Get current speed of all pins, if individually_configurable_pins is false.
    fn speed() -> Result<Speed, Error>;

    /// User-friendly bank name.
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

    // 4, 5, 6 Reserved
    reserved!();
    reserved!();
    reserved!();

    /// 7. Asynchronous stream of events (rising / falling edge), if enabled by [configure_events]
    stream!(event: IoPinEvent);

    /// Read analog voltage at the pin, if supported by hardware.
    fn voltage() -> Option<Volt>;

    /// Mode configuration, input, high-z, push-pull, open-drain or custom.
    /// Optionally set the initial level before changing pin mode.
    /// This method may fail with [UnsupportedMode](Error::UnsupportedMode) error.
    fn set_mode(mode: Mode, initial: Option<Level>) -> Result<(), Error>;
    /// Get current mode.
    fn mode() -> Mode;

    /// Pull resistors selection. [Error::UnsupportedPull] may be returned on incorrect set call.
    property!(rw pull: Pull, Error);

    /// Drive strength selection.  [Error::UnsupportedSpeed] may be returned on incorrect set call.
    property!(rw speed: Speed, Error);

    /// Enable or disable asynchronous events sent through the stream.
    fn configure_events(enabled: IoPinEnabledEvents<'i>) -> Result<(), Error>;

    // fn pulse() -> Result<(), GpioError>;
    // fn set_duty(duty: ?) -> Result<(), GpioError>;
    // fn set_frequency(frequency: ?) -> Result<(), GpioError>;
    // fn set_pwm(frequency, duty)?;
}

/// Digital output level - High and Low.
#[derive_shrink_wrap]
#[ww_repr(u1)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[defmt = "defmt"]
pub enum Level {
    Low,
    High,
}

/// IO pin mode (Push-Pull, Open-Drain, Input, etc.)
#[derive_shrink_wrap]
#[ww_repr(nib)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[defmt = "defmt"]
pub enum Mode {
    PushPullOutput,
    OpenDrainOutput,
    Input,
    /// Electrically the same as Input, but could lower power consumption by disabling input buffer
    HighZ,
    Analog,
    Custom(u8),
}

/// GPIO error
#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[derive(Clone, Debug)]
#[defmt = "defmt"]
pub enum Error {
    UnsupportedMode,
    UnsupportedPull,
    UnsupportedSpeed,
    UnsupportedEventType,
    UnsupportedReferenceVoltage,
    DifferentModes,
    NotImplemented,
    CustomU8(u8),
    CustomU32(u32),
}

/// IO pin pull configuration (pull-up, pull-down, etc.)
#[derive_shrink_wrap]
#[ww_repr(u2)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[defmt = "defmt"]
pub enum Pull {
    None,
    Up,
    Down,
    Custom(u8),
}

/// IO pin drive strength configuration
#[derive_shrink_wrap]
#[ww_repr(nib)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[defmt = "defmt"]
pub enum Speed {
    Slow,
    Medium,
    Fast,
    VeryFast,
    Custom(u8),
}

/// IO pin asynchronous event (interrupt reason), sent via the [Gpio] `event` stream if enabled.
#[derive_shrink_wrap]
#[ww_repr(u2)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[defmt = "defmt"]
pub enum IoPinEvent {
    RisingEdge,
    FallingEdge,
    Custom(u8),
}

/// List of enabled event sources for an IO pin (interrupts) that generate [IoPinEvent] stream.
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[defmt = "defmt"]
pub struct IoPinEnabledEvents<'i> {
    pub rising: bool,
    pub falling: bool,
    pub custom: RefVec<'i, u8>,
}

/// GPIO bank capabilities: supported voltages, modes, custom modes, etc.
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[defmt = "defmt"]
pub struct BankCapabilities<'i> {
    pub voltage: RefVec<'i, Volt>,

    pub push_pull: bool,
    pub open_drain: bool,
    pub input: bool,
    pub individually_configurable_pins: bool,

    pub custom_mode: RefVec<'i, &'i str>,
    pub custom_pull: RefVec<'i, &'i str>,
    pub custom_speed: RefVec<'i, &'i str>,
}

impl Level {
    pub fn is_high(&self) -> bool {
        matches!(*self, Level::High)
    }

    pub fn is_low(&self) -> bool {
        matches!(*self, Level::Low)
    }
}

// PWMOutput(PWMConfig?),
// PWMInput(PWMConfig?),
// PulseOutput,
// PulseInput,
// StepperOutput,
// UartTx,
// UartRx,
// UartRts,
// UartCts,
// I2cSda,
// I2cScl,
// SpiMosi,
// SpiMiso,
// SpiSck,
// SpiCs,
// OneWire,

// AnalogOutput,
// AnalogInput,
// analog modes? encoder?
