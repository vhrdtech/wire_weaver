//! UART over WireWeaver: a device bridges a serial port to the host, or watches the two lines of a serial link.
//!
//! `Uart` is one port: configure it, receive on `rx`, send on the `tx` credit sink, and, on ports with the hardware
//! for it, watch both lines of a link between two other devices on the optional `monitor` stream. Being one trait,
//! a device with several ports is one array, `ww_impl!(uart[]: ww_uart::Uart)`, monitoring included.
//!
//! Received bytes come in chunks ([RxChunk]): a chunk ends after a short silence on the line, when it is full, or on
//! a line error. The receive streams are counted, in bytes: the event head carries each chunk's position and exactly
//! how many bytes the device dropped before it.
//!
//! # Time
//!
//! Every chunk, on `rx` and on `monitor`, says when it was received, on the device's one clock (`ww_timebase::Clock`)
//! that timestamps everything else the device captures:
//!
//! - when its first byte arrived (the end of that byte's stop bit): the time in the event head, always set;
//! - when its last byte arrived: [RxChunk::end].
//!
//! Both are [Second]s since the device's clock started, in the prefix and number type the device picks (usually a
//! `u32` of microseconds, which wraps every 71.6 minutes; the host unwraps it around `Clock::now()`). So a host lines up
//! the bytes of a serial link with the device's other data (a GPIO edge, an ADC sample, an I2C transaction), tells
//! the request and the answer of a `monitor` link apart in time, and sees the gaps between chunks: a chunk ends on a
//! silence, so the next chunk's first byte comes after a gap of at least that silence.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Baud, Second, Volt};

/// Largest item of the `tx` sink, in bytes.
pub const UART_TX_MAX: usize = 256;

/// Largest chunk of received bytes.
pub const UART_CHUNK_MAX: usize = 512;

/// Most discrete IO levels a device lists in [Capabilities::io_levels].
pub const MAX_IO_LEVELS: usize = 8;

/// A serial port: configure, receive on `rx`, send on the `tx` credit sink.
#[ww_trait]
pub trait Uart {
    /// Received bytes, chunked as they arrive (see [RxChunk]); a chunk ends on a short silence, when it is full, or
    /// on a line error. A counted stream, unit: byte; the head's time is when the chunk's first byte arrived (always
    /// set), [RxChunk::end] when its last one did.
    #[counted]
    stream!(rx: RxChunk<'i>);

    /// Bytes to send. Credit-based (API-28): the host never sends more than the device's TX buffer
    /// ([Capabilities::tx_buffer]) holds.
    #[credit]
    sink!(tx: MaxLen<RefVec<'i, u8>, UART_TX_MAX>);

    /// Supported baud rates, modes, IO levels and other features.
    fn capabilities() -> Capabilities<'i>;

    /// Configures the port in one step and returns what was actually set: the baud rate the divider settled on
    /// (the nearest it can do), and the IO level now in effect.
    ///
    /// # Errors
    ///
    /// [Error::Unsupported] (a baud rate outside [Capabilities::min_baud_rate]..=[Capabilities::max_baud_rate], or
    /// a mode, parity, bit count, back-feed detector or IO level this device doesn't have),
    /// [Error::UnsupportedMode], [Error::UnsupportedReferenceVoltage], [Error::InUse], [Error::NoIoLevel],
    /// [Error::Refused] (e.g. the line already sits at a higher voltage than `io_level`).
    fn configure(config: UartConfig) -> Result<UartConfig, Error>;

    /// Manually control pins, only in [Mode::Test].
    ///
    /// # Errors
    ///
    /// [Error::WrongMode] (not in [Mode::Test]), [Error::InUse], [Error::NoIoLevel].
    fn set_pin_level(pin: Pin, is_high: bool) -> Result<(), Error>;

    /// Holds TX low for `duration` (a break), after the bytes already queued.
    ///
    /// # Errors
    ///
    /// [Error::WrongMode] (e.g. currently in [Mode::HighZ] or [Mode::Test]), [Error::InUse], [Error::NoIoLevel],
    /// [Error::Refused] (longer than this device allows).
    fn send_break(duration: Second) -> Result<(), Error>;

    /// Bytes queued for sending and not sent yet.
    fn tx_pending() -> u32;

    /// Both lines of a serial link between two other devices, received without driving them: what both
    /// directions carry, in the order it arrived, decoded with `config`; each chunk says which line it came from.
    /// Opening starts receiving and answers the configuration actually set (the baud rate the receiver settled on,
    /// the IO level in effect), closing stops it. A counted stream, unit: byte (of both lines); the head's time is
    /// when the chunk's first byte arrived (always set), [RxChunk::end] when its last one did.
    ///
    /// Optional: a port without monitoring hardware leaves it out (`omit = ["uart.monitor"]`), and a host sees that
    /// in the device's introspection.
    ///
    /// # Errors
    ///
    /// [Error::Unsupported] (baud rate, mode or parity this monitor can't decode), [Error::InUse], [Error::NoIoLevel].
    #[optional]
    #[stream(MonitorChunk<'i>, counted)]
    fn monitor(config: MonitorConfig) -> Result<MonitorConfig, Error>;
}

/// Received bytes, see `Uart::rx` and `Uart::monitor`: when the first byte arrived is the time in the event head,
/// when the last one did is `end` (see the crate docs, "Time").
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct RxChunk<'i> {
    /// Device time when the chunk's last byte arrived (the end of its stop bit), on the same clock as the head's time
    /// of its first byte; the same as the head's for a one-byte chunk.
    pub end: Second,
    /// Line errors seen while receiving this chunk, if any.
    pub flags: Option<RxFlags>,
    /// The received bytes.
    pub bytes: MaxLen<RefVec<'i, u8>, UART_CHUNK_MAX>,
}

/// Bytes of one line of the watched link, see `Uart::monitor`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct MonitorChunk<'i> {
    pub line: MonitorLine,
    pub chunk: RxChunk<'i>,
}

/// Which line of the watched link a [MonitorChunk] came from.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1
)]
pub enum MonitorLine {
    /// One direction of the link (rePort: RX).
    A,
    /// The other direction (rePort: CTS / RX2).
    B,
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

/// Everything a port is configured with, see `Uart::configure`: what is asked for going in, what was actually set
/// coming back.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug))]
pub struct UartConfig {
    pub baud_rate: Baud,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    /// Flow control, high-Z or manual pin control.
    pub mode: Mode,
    /// Detect RX low for more than one byte interval and hold TX low, so a device asleep isn't powered through TX
    /// ([Capabilities::back_feed_detector]).
    pub prevent_back_feed: bool,
    /// Voltage of the lines on adapters with level translators ([Capabilities::io_levels]); `None` leaves it as it
    /// is going in (the only choice on a device without them), and is the answer of a device without them.
    pub io_level: Option<Volt>,
}

/// Line settings of the watched link, the argument and the answer of `Uart::monitor`.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug))]
pub struct MonitorConfig {
    /// Baud rate of the watched link.
    pub baud_rate: Baud,
    /// Data bits per character of the watched link.
    pub data_bits: DataBits,
    /// Parity of the watched link.
    pub parity: Parity,
    /// Stop bits of the watched link.
    pub stop_bits: StopBits,
    /// Input threshold level, as [UartConfig::io_level].
    pub io_level: Option<Volt>,
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

/// Errors returned by `Uart` methods; each method's doc lists the variants it returns.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug), ww_repr = u4)]
pub enum Error {
    /// This device doesn't have the feature asked for (e.g. a baud rate, mode, parity or bit count outside
    /// [Capabilities]).
    Unsupported,
    /// The [Mode] asked for isn't supported.
    UnsupportedMode,
    /// The IO level asked for isn't one of [Capabilities::io_levels] or in their range.
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
    /// Lowest baud rate this device can set.
    pub min_baud_rate: Baud,
    /// Highest baud rate this device can set.
    pub max_baud_rate: Baud,
    /// Discrete IO levels [UartConfig::io_level] can be set to (e.g. 1.8 V, 3.3 V, 5 V); empty if the adapter has
    /// no level control.
    pub io_levels: MaxLen<RefVec<'i, Volt>, MAX_IO_LEVELS>,
    /// Lowest IO level of an adjustable supply, if the adapter has one.
    pub io_level_min: Option<Volt>,
    /// Highest IO level of an adjustable supply, if the adapter has one.
    pub io_level_max: Option<Volt>,
    /// Whether [Mode::HardwareFlowControl] is supported.
    pub hw_flow_control: bool,
    /// Whether [Mode::SoftwareFlowControl] is supported.
    pub sw_flow_control: bool,
    /// Whether [Mode::HighZ] is supported.
    pub high_z_mode: bool,
    /// Whether [Mode::Test] and `Uart::set_pin_level` are supported.
    pub test_mode: bool,
    /// Whether [UartConfig::prevent_back_feed] is supported.
    pub back_feed_detector: bool,
    /// Device receive buffer, in bytes.
    pub rx_buffer: u32,
    /// Device transmit buffer, in bytes (the credit of the `tx` sink).
    pub tx_buffer: u32,
}
