use wire_weaver::prelude::*;
use ww_si::prefix::One;
use ww_si::units;
use ww_si::{Fixed, Hertz, Ohm, Range, Second};

use crate::Prescaler;
use crate::group::Performance;
use crate::transceiver::TransceiverModes;

/// Most terminator values a device lists in [Capabilities::termination].
pub const MAX_TERMINATION_VALUES: usize = 4;

/// Limits of one phase's [BitTiming] (as SocketCAN's `can_bittiming_const`).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct TimingLimits {
    pub prescaler_min: Prescaler,
    pub prescaler_max: Prescaler,
    pub seg1_min: Fixed<units::TimeQuantum, One, u16>,
    pub seg1_max: Fixed<units::TimeQuantum, One, u16>,
    pub seg2_min: Fixed<units::TimeQuantum, One, u8>,
    pub seg2_max: Fixed<units::TimeQuantum, One, u8>,
    pub sjw_max: Fixed<units::TimeQuantum, One, u8>,
}

/// How the device timestamps frames, see [Capabilities::timestamps].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct TimestampInfo {
    /// Smallest step.
    pub resolution: Second,
    pub point: TimestampPoint,
}

/// Where in a frame its time is taken.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TimestampPoint {
    /// Start of frame, by the controller.
    StartOfFrame,
    /// End of frame, by the controller.
    EndOfFrame,
    /// By software when the frame was read out of the controller (jitter of the interrupt latency).
    Software,
}

/// How a controller's message memory is divided, see `Can::set_layout`; counts in elements.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct MemoryLayout {
    pub standard_filters: Fixed<units::Filter, One, u16>,
    pub extended_filters: Fixed<units::Filter, One, u16>,
    /// Frames each receive FIFO holds, FIFO 0 first (0: not used).
    pub rx_fifos: [Fixed<units::Frame, One, u16>; 2],
    pub rx_buffers: Fixed<units::Buffer, One, u16>,
    pub tx_buffers: Fixed<units::Buffer, One, u16>,
    /// Frames the transmit FIFO / queue holds.
    pub tx_queue: Fixed<units::Frame, One, u16>,
    /// Events the TX event FIFO holds.
    pub tx_events: Fixed<units::Event, One, u16>,
    /// Data an element holds (8, 12, 16, 20, 24, 32, 48 or 64 bytes); frames with more are cut (and flagged by the
    /// controller).
    pub element_data: Fixed<units::Byte, One, u8, Range<8, 64>>,
}

/// What a `Can` implementation supports, see `Can::capabilities`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct Capabilities<'i> {
    /// Controller (kernel) clock the time quanta are counted in.
    pub clock: Hertz,
    /// Limits of the nominal bit timing.
    pub nominal: TimingLimits,
    /// Limits of the data bit timing; `None`: no CAN FD.
    pub data: Option<TimingLimits>,
    /// [Tdc::Manual] and [Tdc::Auto] are supported.
    pub tdc: bool,
    /// [Mode::Restricted] is supported.
    pub restricted: bool,
    /// [Mode::ListenOnly] is supported.
    pub listen_only: bool,
    /// [Mode::InternalLoopback] is supported.
    pub internal_loopback: bool,
    /// [Mode::ExternalLoopback] is supported.
    pub external_loopback: bool,
    /// [Mode::Test] (and `Can::test_pins`) is supported.
    pub test_mode: bool,
    /// [Mode::Sleep] is supported.
    pub sleep: bool,
    /// [Options::one_shot] is supported.
    pub one_shot: bool,
    /// [Options::presume_ack] is supported.
    pub presume_ack: bool,
    /// [Options::triple_sample] is supported.
    pub triple_sample: bool,
    /// [Options::tx_pause] is supported.
    pub tx_pause: bool,
    /// Both [TxOrder]s are supported (otherwise only the one `configure` answers).
    pub tx_order: bool,
    /// Both [RxOverflow]s are supported (otherwise only the one `configure` answers).
    pub rx_overflow: bool,
    /// [Options::edge_filtering] is supported.
    pub edge_filtering: bool,
    /// [Options::protocol_exception] can be switched off.
    pub protocol_exception: bool,
    /// [Options::wake_on_bus] is supported.
    pub wake_on_bus: bool,
    /// [FrameKind::DataLongDlc] frames can be sent and are told apart when received.
    pub long_dlc: bool,
    /// Automatic bus-off recovery ([CanConfig::bus_off_recovery] `Some`) is supported.
    pub auto_recovery: bool,
    /// How frames are timestamped; `None`: the heads carry no time.
    pub timestamps: Option<TimestampInfo>,
    /// Receive FIFOs ([FilterAction::Fifo] indices).
    pub rx_fifos: Fixed<units::Fifo, One, u8>,
    /// Frames the receive FIFOs hold together.
    pub rx_fifo_depth: Fixed<units::Frame, One, u16>,
    /// Frames the transmit queue holds (the credit of `tx` is its size in bytes).
    pub tx_queue_depth: Fixed<units::Frame, One, u16>,
    /// Dedicated transmit buffers (`Can::tx_buffer`).
    pub tx_buffers: Fixed<units::Buffer, One, u16>,
    /// Dedicated receive buffers (`Can::rx_buffer`).
    pub rx_buffers: Fixed<units::Buffer, One, u16>,
    /// Filter elements for standard IDs.
    pub standard_filters: Fixed<units::Filter, One, u16>,
    /// Filter elements for extended IDs.
    pub extended_filters: Fixed<units::Filter, One, u16>,
    /// [FilterMatch::Mask] is supported.
    pub filter_mask: bool,
    /// [FilterMatch::Range] is supported.
    pub filter_range: bool,
    /// Message memory for `Can::set_layout`; `None`: fixed layout.
    pub memory: Option<Fixed<units::Word, One, u32>>,
    /// [TransceiverMode]s `Can::set_transceiver` can switch to.
    pub transceiver_modes: TransceiverModes,
    /// Terminator values `Can::set_termination` can switch in; empty without a switchable terminator.
    pub termination: MaxLen<RefVec<'i, Ohm>, MAX_TERMINATION_VALUES>,
    /// What this controller guarantees on its own; `None` when only its group's figure makes sense.
    pub performance: Option<Performance>,
}
