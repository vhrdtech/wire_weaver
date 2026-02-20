#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Second, Volt};

#[ww_trait]
pub trait Uart {
    /// Receive stream
    stream!(rx: RxChunk<'i>);
    /// Transmit sink
    sink!(tx: TxChunk<'i>);
    /// TX observe stream
    stream!(tx_mon: RxChunk<'i>);

    /// Supported baud rates, modes and other features.
    fn capabilities() -> Capabilities<'i>;

    /// Baud rate configuration
    property!(baud_rate: BaudRate/*, Error*/);

    /// Mode configuration
    property!(mode: Mode/*, Error*/);

    /// Stop bits configuration
    property!(stop_bits: StopBits/*, Error*/);

    /// Parity configuration
    property!(parity: Parity/*, Error*/);

    /// Detect RX low for more than 1 byte interval and set TX low to avoid powering device in sleep mode through it.
    /// Optional, check [Capabilities] if supported.
    property!(prevent_back_feed: bool/*, Error*/);

    /// Reference voltage configuration, if supported.
    property!(reference_voltage: Volt/*, Error*/);

    /// Manually control pins, only in [Mode::Test]
    fn set_pin_level(pin: Pin, is_high: bool) -> Result<(), Error>;
}

pub struct RxChunk<'i> {
    pub flags: Option<RxFlags>,
    pub timestamp: Option<Second>,
    pub bytes: &'i [u8],
}

pub struct RxFlags {
    pub parity_error: bool,
}

pub struct TxChunk<'i> {
    pub transmit_at: Option<Second>,
    pub bytes: &'i [u8],
}

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

pub enum Mode {
    /// No flow control.
    Asynchronous,
    /// Flow control using RTS/CTS signals.
    HardwareFlowControl,
    /// Flow control using XON/XOFF bytes.
    SoftwareFlowControl,
    /// Both TX and RX lines are receiving, useful for monitoring traffic between two devices.
    Monitor2,
    /// All lines are receiving (TX, RX, RTS, CTS), useful for monitoring traffic between two devices.
    Monitor4,
    /// All pins High-Z.
    HighZ,
    /// Manual control of pins state.
    Test,
}

/// Pin selector used in test mode.
pub enum Pin {
    Tx,
    Rx,
    Rts,
    Cts,
}

/// Number of stop bits transmitted after every character.
pub enum StopBits {
    Stop1,
    Stop2
}

/// Parity checking mode.
pub enum Parity {
    None,
    Odd,
    Even,
}

pub enum Error {
    UnsupportedMode,
    UnsupportedReferenceVoltage,
    WrongMode,
    CustomU8(u8),
    CustomU32(u32),
    // CustomBytes(&'i [u8])
}

pub struct Capabilities<'i> {
    pub min_baud_rate: u32,
    pub max_baud_rate: u32,
    pub voltages: RefVec<'i, Volt>,
    pub rx_timestamps: bool,
    pub transmit_at: bool,
    pub hw_flow_control: bool,
    pub sw_flow_control: bool,
    pub high_z_mode: bool,
    pub test_mode: bool,
    pub back_feed_detector: bool,
}