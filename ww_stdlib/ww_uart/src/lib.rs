//! UART over WireWeaver: a device bridges a serial port to the host, or watches the two lines of a serial link.
//!
//! | Trait          | What it is for                                                    | Who implements it        |
//! |----------------|-------------------------------------------------------------------|--------------------------|
//! | `Uart`         | Configure, receive on `rx`, send on the `tx` credit sink.         | Every UART bridge        |
//! | `UartMonitor`  | Both lines of a serial link as two streams, without driving them. | Sniffers, debug adapters |
//!
//! Received bytes come in chunks ([RxChunk]): a chunk ends after a short silence on the line, when it is full, or on
//! a line error. How a host learns about chunks the device had to drop is the subject of the stream reliability
//! work (`ww-stream-reliability`, see `docs/std_library/io_family.md`).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Second, Volt};

/// Largest item of the `tx` sink, in bytes.
pub const UART_TX_MAX: usize = 256;

/// Largest chunk of received bytes.
pub const UART_CHUNK_MAX: usize = 512;

/// A serial port: configure, receive on `rx`, send on the `tx` credit sink.
#[ww_trait]
pub trait Uart {
    /// Received bytes, chunked as they arrive (see [RxChunk]); a chunk ends on a short silence, when it is full, or
    /// on a line error.
    stream!(rx: RxChunk<'i>);

    /// Bytes to send. Credit-based (API-28): the host never sends more than the device's TX buffer
    /// ([Capabilities::tx_buffer]) holds.
    #[credit]
    sink!(tx: MaxLen<RefVec<'i, u8>, UART_TX_MAX>);

    /// Supported baud rates, modes and other features.
    fn capabilities() -> Capabilities<'i>;

    /// Baud rate configuration; `Uart::baud_rate_actual` says what the divider made of it.
    ///
    /// # Errors
    ///
    /// [Error::Unsupported] (not in [Capabilities::min_baud_rate]..=[Capabilities::max_baud_rate] or this device
    /// only offers a fixed set), [Error::InUse], [Error::NoIoLevel].
    property!(rw baud_rate: BaudRate, Error);

    /// Mode configuration.
    ///
    /// # Errors
    ///
    /// [Error::UnsupportedMode], [Error::InUse], [Error::NoIoLevel].
    property!(rw mode: Mode, Error);

    /// Stop bits configuration.
    ///
    /// # Errors
    ///
    /// [Error::Unsupported], [Error::InUse].
    property!(rw stop_bits: StopBits, Error);

    /// Parity configuration.
    ///
    /// # Errors
    ///
    /// [Error::Unsupported], [Error::InUse].
    property!(rw parity: Parity, Error);

    /// Data bits per character.
    ///
    /// # Errors
    ///
    /// [Error::Unsupported], [Error::InUse].
    property!(rw data_bits: DataBits, Error);

    /// Detect RX low for more than 1 byte interval and set TX low to avoid powering device in sleep mode through it.
    /// Optional, check [Capabilities] if supported.
    ///
    /// # Errors
    ///
    /// [Error::Unsupported] (this device has no back-feed detector).
    property!(rw prevent_back_feed: bool, Error);

    /// Reference voltage configuration, if supported.
    ///
    /// # Errors
    ///
    /// [Error::UnsupportedReferenceVoltage], [Error::NoIoLevel].
    property!(rw reference_voltage: Volt, Error);

    /// Manually control pins, only in [Mode::Test].
    ///
    /// # Errors
    ///
    /// [Error::WrongMode] (not in [Mode::Test]), [Error::InUse], [Error::NoIoLevel].
    fn set_pin_level(pin: Pin, is_high: bool) -> Result<(), Error>;

    /// The baud rate actually set, in baud (the nearest the divider can do).
    fn baud_rate_actual() -> u32;

    /// Holds TX low for `duration` (a break), after the bytes already queued.
    ///
    /// # Errors
    ///
    /// [Error::WrongMode] (e.g. currently in [Mode::HighZ] or [Mode::Test]), [Error::InUse], [Error::NoIoLevel],
    /// [Error::Refused] (longer than this device allows).
    fn send_break(duration: Second) -> Result<(), Error>;

    /// Bytes queued for sending and not sent yet.
    fn tx_pending() -> u32;
}

/// Both lines of a serial link between two other devices, received without driving them.
///
/// Configured with `UartMonitor::configure_monitor`; each stream's sideband `Open` starts receiving its line,
/// `Close` stops it.
#[ww_trait]
pub trait UartMonitor {
    /// One direction of the link (rePort: RX). Receiving starts on sideband `Open`, stops on `Close`.
    stream!(line_a: RxChunk<'i>);

    /// The other direction (rePort: CTS / RX2). Receiving starts on sideband `Open`, stops on `Close`.
    stream!(line_b: RxChunk<'i>);

    /// Line settings of the watched link; answers the baud rate actually set.
    ///
    /// # Errors
    ///
    /// [Error::Unsupported] (baud rate, mode or parity this monitor can't decode), [Error::InUse], [Error::NoIoLevel].
    fn configure_monitor(config: MonitorConfig) -> Result<u32, Error>;
}

/// Received bytes, see `Uart::rx` and `UartMonitor`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct RxChunk<'i> {
    /// Line errors seen while receiving this chunk, if any.
    pub flags: Option<RxFlags>,
    /// When this chunk was received, device time since the clock started.
    pub timestamp: Option<Second>,
    /// The received bytes.
    pub bytes: MaxLen<RefVec<'i, u8>, UART_CHUNK_MAX>,
}

/// Line errors seen in a chunk.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug))]
pub struct RxFlags {
    /// A character's parity bit didn't match the configured [Parity].
    pub parity_error: bool,
    /// A character's stop bit wasn't where expected.
    pub framing_error: bool,
    /// A byte arrived before the previous one was read out of the device's receive buffer and was lost.
    pub overrun: bool,
    /// A break condition (an extended low on the line) was detected.
    pub break_detected: bool,
}

/// Line settings of `UartMonitor::configure_monitor`.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug))]
pub struct MonitorConfig {
    /// Baud rate of the watched link.
    pub baud_rate: BaudRate,
    /// Data bits per character of the watched link.
    pub data_bits: DataBits,
    /// Parity of the watched link.
    pub parity: Parity,
    /// Stop bits of the watched link.
    pub stop_bits: StopBits,
}

/// A baud rate; the common ones encode in a nibble, anything else falls back to [BaudRate::BaudOther].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = u4
)]
pub enum BaudRate {
    Baud110,
    Baud300,
    Baud600,
    Baud1200,
    Baud2400,
    Baud4800,
    Baud9600,
    Baud19200,
    Baud38400,
    Baud57600,
    Baud115200,
    Baud921600,
    /// Any other rate, in baud.
    BaudOther(u32),
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = u4
)]
pub enum Mode {
    /// No flow control.
    Asynchronous,
    /// Flow control using RTS/CTS signals.
    HardwareFlowControl,
    /// Flow control using XON/XOFF bytes.
    SoftwareFlowControl,
    /// All pins High-Z.
    HighZ,
    /// Manual control of pins state.
    Test,
}

/// Pin selector used in test mode (`Uart::set_pin_level`).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = u2
)]
pub enum Pin {
    /// Transmit pin.
    Tx,
    /// Receive pin.
    Rx,
    /// Request-to-send pin.
    Rts,
    /// Clear-to-send pin.
    Cts,
}

/// Number of stop bits transmitted after every character.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = u1
)]
pub enum StopBits {
    /// One stop bit.
    Stop1,
    /// Two stop bits.
    Stop2,
}

/// Parity checking mode.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = u2
)]
pub enum Parity {
    /// No parity bit.
    None,
    /// Odd parity.
    Odd,
    /// Even parity.
    Even,
}

/// Data bits per character.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = u3
)]
pub enum DataBits {
    Eight,
    Seven,
    Nine,
    Six,
    Five,
}

/// Errors returned by `Uart` and `UartMonitor` methods; each method's doc lists the variants it returns.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug), ww_repr = u4)]
pub enum Error {
    /// This device doesn't have the feature asked for (e.g. a baud rate, mode, parity or bit count outside
    /// [Capabilities]).
    Unsupported,
    /// The [Mode] asked for isn't supported.
    UnsupportedMode,
    /// The [Volt] asked for isn't in [Capabilities::voltages].
    UnsupportedReferenceVoltage,
    /// The call needs a different [Mode] than the one currently set (e.g. `Uart::set_pin_level` outside
    /// [Mode::Test]).
    WrongMode,
    /// A device-specific error, carried as a single byte.
    CustomU8(u8),
    /// A device-specific error, carried as four bytes.
    CustomU32(u32),
    /// The pins serve another function.
    InUse,
    /// The level translators have no supply (IO level off).
    NoIoLevel,
    /// A safety rule of the device doesn't allow it.
    Refused,
    /// The line stayed busy (e.g. CTS held off too long).
    Timeout,
}

/// What a `Uart` implementation supports, see `Uart::capabilities`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct Capabilities<'i> {
    /// Lowest baud rate this device can set, in baud.
    pub min_baud_rate: u32,
    /// Highest baud rate this device can set, in baud.
    pub max_baud_rate: u32,
    /// Reference voltages this device's `reference_voltage` property can be set to; empty if it isn't adjustable.
    pub voltages: MaxLen<RefVec<'i, Volt>, 8>,
    /// Whether `rx` chunks carry a [RxChunk::timestamp].
    pub rx_timestamps: bool,
    /// Whether [Mode::HardwareFlowControl] is supported.
    pub hw_flow_control: bool,
    /// Whether [Mode::SoftwareFlowControl] is supported.
    pub sw_flow_control: bool,
    /// Whether [Mode::HighZ] is supported.
    pub high_z_mode: bool,
    /// Whether [Mode::Test] and `Uart::set_pin_level` are supported.
    pub test_mode: bool,
    /// Whether the `prevent_back_feed` property is supported.
    pub back_feed_detector: bool,
    /// Device receive buffer, in bytes.
    pub rx_buffer: u32,
    /// Device transmit buffer, in bytes (the credit of the `tx` sink).
    pub tx_buffer: u32,
}
