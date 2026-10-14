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

/// A serial port.
#[ww_trait]
pub trait Uart {
    /// Received bytes.
    stream!(rx: RxChunk<'i>);

    /// Bytes to send. Credit-based: the host never sends more than the device's TX buffer holds.
    #[credit]
    sink!(tx: MaxLen<RefVec<'i, u8>, UART_TX_MAX>);

    /// Supported baud rates, modes and other features.
    fn capabilities() -> Capabilities<'i>;

    /// Baud rate configuration; `Uart::baud_rate_actual` says what the divider made of it.
    property!(rw baud_rate: BaudRate, Error);

    /// Mode configuration.
    property!(rw mode: Mode, Error);

    /// Stop bits configuration.
    property!(rw stop_bits: StopBits, Error);

    /// Parity configuration.
    property!(rw parity: Parity, Error);

    /// Data bits per character.
    property!(rw data_bits: DataBits, Error);

    /// Detect RX low for more than 1 byte interval and set TX low to avoid powering device in sleep mode through it.
    /// Optional, check [Capabilities] if supported.
    property!(rw prevent_back_feed: bool, Error);

    /// Reference voltage configuration, if supported.
    property!(rw reference_voltage: Volt, Error);

    /// Manually control pins, only in [Mode::Test].
    fn set_pin_level(pin: Pin, is_high: bool) -> Result<(), Error>;

    /// The baud rate actually set, in baud (the nearest the divider can do).
    fn baud_rate_actual() -> u32;

    /// Holds TX low for `duration` (a break), after the bytes already queued.
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
    /// One direction of the link (rePort: RX).
    stream!(line_a: RxChunk<'i>);

    /// The other direction (rePort: CTS / RX2).
    stream!(line_b: RxChunk<'i>);

    /// Line settings of the watched link; answers the baud rate actually set.
    fn configure_monitor(config: MonitorConfig) -> Result<u32, Error>;
}

/// Received bytes, see [Uart] `rx` and [UartMonitor].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct RxChunk<'i> {
    pub flags: Option<RxFlags>,
    pub timestamp: Option<Second>,
    pub bytes: MaxLen<RefVec<'i, u8>, UART_CHUNK_MAX>,
}

/// Line errors seen in a chunk.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug))]
pub struct RxFlags {
    pub parity_error: bool,
    pub framing_error: bool,
    pub overrun: bool,
    pub break_detected: bool,
}

/// Line settings of `UartMonitor::configure_monitor`.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug))]
pub struct MonitorConfig {
    pub baud_rate: BaudRate,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug), ww_repr = u4)]
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
    BaudOther(u32),
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug), ww_repr = u4)]
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

/// Pin selector used in test mode.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug), ww_repr = u2)]
pub enum Pin {
    Tx,
    Rx,
    Rts,
    Cts,
}

/// Number of stop bits transmitted after every character.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug), ww_repr = u1)]
pub enum StopBits {
    Stop1,
    Stop2,
}

/// Parity checking mode.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug), ww_repr = u2)]
pub enum Parity {
    None,
    Odd,
    Even,
}

/// Data bits per character.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug), ww_repr = u3)]
pub enum DataBits {
    Eight,
    Seven,
    Nine,
    Six,
    Five,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug), ww_repr = u4)]
pub enum Error {
    Unsupported,
    UnsupportedMode,
    UnsupportedReferenceVoltage,
    WrongMode,
    CustomU8(u8),
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

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct Capabilities<'i> {
    pub min_baud_rate: u32,
    pub max_baud_rate: u32,
    pub voltages: MaxLen<RefVec<'i, Volt>, 8>,
    pub rx_timestamps: bool,
    pub hw_flow_control: bool,
    pub sw_flow_control: bool,
    pub high_z_mode: bool,
    pub test_mode: bool,
    pub back_feed_detector: bool,
    /// Device receive buffer, in bytes.
    pub rx_buffer: u32,
    /// Device transmit buffer, in bytes (the credit of the `tx` sink).
    pub tx_buffer: u32,
}
