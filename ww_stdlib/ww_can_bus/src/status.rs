use ww_api::prelude::*;
use ww_si::prefix::One;
use ww_si::units;
use ww_si::{Fixed, Second};

use crate::Share;

/// An error the controller detected on the bus, see `Can::bus_errors`; when is the time in the event head.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct BusError {
    pub kind: ProtocolError,
    pub phase: Phase,
    pub dir: Direction,
}

/// Kind of a protocol error (ISO 11898-1 error detection).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum ProtocolError {
    /// More than 5 equal bits in a row.
    Stuff,
    /// A fixed-format field had an illegal bit.
    Form,
    /// The frame wasn't acknowledged.
    Ack,
    /// Sent recessive, read dominant (outside arbitration).
    Bit1,
    /// Sent dominant, read recessive.
    Bit0,
    /// CRC mismatch.
    Crc,
    /// Protocol exception: a recessive reserved bit after FDF (a frame of a newer protocol); the controller went to
    /// bus integration instead of signalling an error ([Options::protocol_exception]).
    Exception,
}

/// Phase of a frame an error happened in.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1,
    no_future_version
)]
pub enum Phase {
    /// Arbitration phase, at the nominal bit rate (all of a classic frame).
    Arbitration,
    /// Data phase of an FD frame with bit rate switch.
    Data,
}

/// Whether the controller was sending or receiving.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1,
    no_future_version
)]
pub enum Direction {
    Tx,
    Rx,
}

/// Error state and counters, see `Can::status`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Status {
    pub state: ErrorState,
    /// Transmit error counter.
    #[no_unit_hint = "ISO 11898-1 fault confinement counter as the controller reads it: points (+8 per error, \
                       -1 per frame sent), not a count of errors"]
    pub tec: u8,
    /// Receive error counter (some controllers stop at 127 and only flag error passive above).
    #[no_unit_hint = "same ISO 11898-1 fault confinement points scale as tec, not a count of errors"]
    pub rec: u8,
    /// Last protocol error in the arbitration phase (or of a classic frame), if any since the last read.
    pub last_error: Option<ProtocolError>,
    /// Last protocol error in the data phase of an FD frame with bit rate switch, if any since the last read.
    pub last_data_error: Option<ProtocolError>,
}

/// Fault confinement state of a controller.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum ErrorState {
    /// Off the bus: [Mode::Off], [Mode::Config], or not configured yet.
    Stopped,
    /// Both counters below 96.
    Active,
    /// A counter at 96 or above (error warning).
    Warning,
    /// A counter above 127: sends passive error flags only.
    Passive,
    /// TEC above 255: off the bus until recovered.
    BusOff,
    /// [Mode::Sleep]: goes back to `Active` on a wake-up, with [Options::wake_on_bus] on bus activity.
    Sleeping,
}

/// The controller's protocol state, see `Can::protocol_status`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct ProtocolStatus {
    pub status: Status,
    pub activity: Activity,
    /// The controller's error logging counter (errors since the last read, saturating), if it has one.
    pub error_log: Option<Fixed<units::Event, One, u8>>,
    /// Flags of the last received FD frame, if one came since the last read.
    pub last_fd_frame: Option<FdFrameFlags>,
    /// A protocol exception happened since the last read.
    pub protocol_exception: bool,
    /// Transceiver delay the controller measured, in minimum time quanta (clock periods of [Capabilities::clock]),
    /// with TDC on.
    pub tdc_value: Option<Fixed<units::Cycle, One, u8>>,
    /// Corrected and uncorrected bit errors of the message memory (ECC / parity) since the last read, where the
    /// controller detects them.
    pub memory_errors: Option<Fixed<units::Event, One, u16>>,
}

/// What the controller is doing on the bus.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Activity {
    /// Waiting for 11 recessive bits to join the bus.
    Synchronizing,
    Idle,
    Receiving,
    Transmitting,
}

/// Flags of a received FD frame (M_CAN `PSR.RESI` / `RBRS` / `REDL`).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct FdFrameFlags {
    pub esi: bool,
    pub brs: bool,
}

/// Bus load measured by the device, see `Can::bus_load`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct BusLoad {
    /// Length of the measurement window.
    pub window: Second,
    /// Busy time of the bus in the window (frames with their stuff bits, error and overload frames, interframe space
    /// not counted).
    pub load: Share,
    /// Frames seen on the bus in the window, sent and received.
    pub frames: Fixed<units::Frame, One, u32>,
    /// Error frames in the window.
    pub errors: Fixed<units::Frame, One, u32>,
}
