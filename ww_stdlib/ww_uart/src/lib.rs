//! UART over WireWeaver: a device bridges a serial port to the host, or watches the two lines of a serial link.
//!
//! `Uart` is one port: configure it, receive on `rx`, send on the `tx` credit sink, and, on ports with the hardware
//! for it, watch both lines of a link between two other devices on the optional `monitor` stream. Being one trait,
//! a device with several ports is one array, `ww_impl!(uart[]: ww_uart::Uart)`, monitoring included.
//!
//! Received bytes come in chunks ([RxChunk]): a chunk ends when the line stays idle for more than
//! [UartConfig::idle_chars] character times, when it is full, or on a line error. So a chunk is one burst of
//! back-to-back characters (a frame of most protocols), or the part of a longer one that fit. The receive streams are
//! counted, in bytes: the event head carries each chunk's position and exactly how many bytes the device dropped
//! before it.
//!
//! # Time
//!
//! Every chunk, on `rx` and on `monitor`, says when it was received, on the device's one clock (`ww_timebase::Clock`)
//! that timestamps everything else the device captures:
//!
//! - when its first byte arrived (the end of that byte's stop bit): the time in the event head, always set;
//! - when its last byte arrived: [RxChunk::end], set by devices that measure it, `None` from those that don't.
//!
//! The bytes of a chunk came back to back, so the time of each one follows from the first: byte `i` (from 0) of a
//! chunk of `n` arrived at
//!
//! - `first + i * (end - first) / (n - 1)` when `end` is set (this takes up the sender's baud rate error: 2-3 % off
//!   the nominal rate, which UARTs accept, adds up to milliseconds over a long burst);
//! - `first + i * frame_bits / baud` when it isn't, `frame_bits` being start + data + parity + stop bits
//!   ([UartConfig::frame_bits]) and `baud` the rate `configure` answered. The chunk's last byte is then at
//!   `first + (n - 1) * frame_bits / baud`.
//!
//! All are [Second]s since the device's clock started, in the prefix and number type the device picks (usually a
//! `u32` of microseconds, which wraps every 71.6 minutes; the host unwraps it around `Clock::now()`). So a host lines up
//! the bytes of a serial link with the device's other data (a GPIO edge, an ADC sample, an I2C transaction), tells
//! the request and the answer of a `monitor` link apart in time, and sees the gaps between chunks: a chunk ends on a
//! silence of more than [UartConfig::idle_chars] character times, so the next chunk's first byte comes at least that
//! much after the previous chunk's last one.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Baud, Second, Volt};

/// Largest item of the `tx` sink, in bytes.
pub const UART_TX_MAX: usize = 256;

/// Largest chunk of received bytes.
pub const UART_CHUNK_MAX: usize = 512;

/// Suggested [UartConfig::idle_chars]: a gap of more than two character times ends a chunk. One would split the
/// output of senders that leave a short pause between characters (a CPU feeding its UART byte by byte, a USB-serial
/// bridge between its USB packets), more would merge frames of protocols that keep only a short gap between them
/// (Modbus RTU separates frames by 3.5 character times, this still splits them).
pub const IDLE_CHARS_DEFAULT: u8 = 2;

/// Most discrete IO levels a device lists in [Capabilities::io_levels].
pub const MAX_IO_LEVELS: usize = 8;

/// A serial port: configure, receive on `rx`, send on the `tx` credit sink.
#[ww_trait]
#[claim(exclusive)]
pub trait Uart {
    /// Received bytes, chunked as they arrive (see [RxChunk]); a chunk ends when the line is idle for more than
    /// [UartConfig::idle_chars] character times, when it is full, or on a line error. A counted stream, unit: byte;
    /// the head's time is when the chunk's first byte arrived (always set), [RxChunk::end] when its last one did.
    #[counted]
    stream!(rx: RxChunk<'i>);

    /// Bytes to send. Credit-based (API-28): the host never sends more than the device's TX buffer
    /// ([Capabilities::tx_buffer]) holds.
    #[credit]
    sink!(tx: MaxLen<RefVec<'i, u8>, UART_TX_MAX>);

    /// Supported baud rates, modes, IO levels and other features.
    fn capabilities() -> Capabilities<'i>;

    /// Configures the port in one step and returns what was actually set: the baud rate the divider settled on
    /// (the nearest it can do), the idle time that ends a chunk (the nearest the receiver can time), and the IO
    /// level now in effect. A baud rate outside
    /// [Capabilities::min_baud_rate]..=[Capabilities::max_baud_rate], or a mode, parity, bit count, back-feed detector
    /// or IO level this device doesn't have is `Unsupported`; `Refused` when e.g. the line already sits at a higher
    /// voltage than `io_level`.
    #[errors(Common(Unsupported, InUse, NoPower, Refused))]
    fn configure(config: UartConfig) -> Result<UartConfig, UartError<'i>>;

    /// Manually control pins, only in [Mode::Test].
    ///
    /// `WrongState` in any other mode.
    #[errors(Common(WrongState, InUse, NoPower))]
    fn set_pin_level(pin: Pin, is_high: bool) -> Result<(), UartError<'i>>;

    /// Holds TX low for `duration` (a break), after the bytes already queued.
    /// `WrongState` in [Mode::HighZ] or [Mode::Test], `Refused` when longer than this device allows.
    #[errors(Common(WrongState, InUse, NoPower, Refused))]
    fn send_break(duration: Second) -> Result<(), UartError<'i>>;

    /// Bytes queued for sending and not sent yet.
    fn tx_pending() -> u32;

    /// Both lines of a serial link between two other devices, received without driving them: what both
    /// directions carry, in the order it arrived, decoded with `config`; each chunk says which line it came from.
    /// Opening starts receiving and answers the configuration actually set (the baud rate the receiver settled on,
    /// the IO level in effect), closing stops it. A counted stream, unit: byte (of both lines); the head's time is
    /// when the chunk's first byte arrived (always set), [RxChunk::end] when its last one did.
    ///
    /// Optional: a port without monitoring hardware leaves it out (`omit = ["uart.monitor"]`), and a host sees that
    /// in the device's introspection. `Unsupported` for a baud rate, mode or parity this monitor can't decode.
    #[optional]
    #[errors(Common(Unsupported, InUse, NoPower))]
    #[stream(MonitorChunk<'i>, counted)]
    fn monitor(config: MonitorConfig) -> Result<MonitorConfig, UartError<'i>>;
}

/// Received bytes that came back to back, see `Uart::rx` and `Uart::monitor`: when the first byte arrived is the
/// time in the event head, when the last one did is `end` (see the crate docs, "Time").
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct RxChunk<'i> {
    /// Device time when the chunk's last byte arrived (the end of its stop bit), on the same clock as the head's time
    /// of its first byte; the same as the head's for a one-byte chunk. `None` from a device that doesn't measure it:
    /// the last byte is then `(n - 1) * frame_bits / baud` after the first (see the crate docs, "Time").
    pub end: Option<Second>,
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
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq))]
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
    /// Idle time on RX that ends a chunk, in character times (`frame_bits / baud` each): a gap longer than this ends
    /// the chunk, a shorter one doesn't. [IDLE_CHARS_DEFAULT] (2) unless a protocol asks for another; 0 is taken as 1.
    pub idle_chars: u8,
}

/// Line settings of the watched link, the argument and the answer of `Uart::monitor`.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq))]
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
    /// Idle time on either line that ends a chunk of that line, as [UartConfig::idle_chars].
    pub idle_chars: u8,
}

impl UartConfig {
    /// Bits of one character on the line: start + data + parity + stop; `None` for a setting from a newer version of
    /// this crate. A character takes `frame_bits / baud_rate` seconds.
    pub fn frame_bits(&self) -> Option<u32> {
        frame_bits(self.data_bits, self.parity, self.stop_bits)
    }
}

impl MonitorConfig {
    /// Bits of one character of the watched link, as [UartConfig::frame_bits].
    pub fn frame_bits(&self) -> Option<u32> {
        frame_bits(self.data_bits, self.parity, self.stop_bits)
    }
}

fn frame_bits(data_bits: DataBits, parity: Parity, stop_bits: StopBits) -> Option<u32> {
    let data = match data_bits {
        DataBits::Five => 5,
        DataBits::Six => 6,
        DataBits::Seven => 7,
        DataBits::Eight => 8,
        DataBits::Nine => 9,
        DataBits::Unknown(_) => return None,
    };
    let parity = match parity {
        Parity::None => 0,
        Parity::Odd | Parity::Even => 1,
        Parity::Unknown(_) => return None,
    };
    let stop = match stop_bits {
        StopBits::Stop1 => 1,
        StopBits::Stop2 => 2,
    };
    Some(1 + data + parity + stop)
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

/// Errors of `Uart`: the common ones (unsupported, in use, no power, refused, ...); each method lists the variants it
/// returns. Line errors of received data are not errors of a call, they are [RxFlags] of the chunk.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum UartError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
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
