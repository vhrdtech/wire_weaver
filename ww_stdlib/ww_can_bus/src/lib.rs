//! CAN bus over WireWeaver: a device (a USB-CAN adapter, a gateway, a node with a CAN port, a bus analyzer) lets a
//! host send and receive frames on its CAN bus, configure it and watch its state.
//!
//! # One trait, optional items
//!
//! [Can] is one controller with its transceiver (and terminator, where switchable). A device with several is an
//! array, `ww_impl!(can[]: ww_can_bus::Can)`. Six items are required, and they are all a simple node needs:
//! `capabilities`, `configure`, `config`, `status`, the `rx` stream and the `tx` sink. Everything only some
//! controllers have is an [optional item](https://vhrdtech.github.io/wire_weaver/api/optional_items/) the device
//! leaves out (`omit = ["can.filters", "can.tx_buffers", ..]`): bus-off recovery on request, aborting queued
//! frames, TX events, bus error events, acceptance filters, dedicated RX and TX buffers, a switchable terminator and
//! test mode pins. A host finds out what is there through introspection, and what the present items support
//! through [Can::capabilities].
//!
//! Watching a bus is not another trait: it is the same controller in [Mode::ListenOnly] (or [Mode::Restricted]),
//! with no filters, plus the optional `bus_errors` stream.
//!
//! # Frames
//!
//! [Frame] is a classic data frame, a remote frame or a CAN FD frame (with its BRS and ESI bits), with an 11-bit or
//! 29-bit [CanId] and up to 64 bytes. With the `embedded-can` feature, [CanId] converts from and to
//! `embedded_can::Id`, and on hosts `FrameOwned` implements `embedded_can::Frame` (classic frames). CAN XL frames are
//! not part of [Frame]: a frame of up to 2048 bytes would make every device size its `tx` buffer for one.
//!
//! # Time
//!
//! Every received frame, TX event and bus error says when it happened, as the time in its counted stream's event
//! head: a [Second] of the device's one clock (`ww_timebase::Clock`) that timestamps everything else the device
//! captures, in the prefix and number type the device picks (usually a `u32` of microseconds, which wraps every 71.6
//! minutes; the host unwraps it around `Clock::now()`). Where in the frame the time was taken (start of frame, end of
//! frame or by software on reception) and how fine it is: [Capabilities::timestamps].
//!
//! # Errors
//!
//! Every method returns [CanError]: the framework's `CommonError` (unsupported, in use, wrong state, ...) as
//! [CanError::Common], plus what is specific to CAN (`OutOfFilters`, `BusOff`). Each method lists the variants it
//! returns (`#[errors]`). Errors on the bus are not errors of a call: they are counted in [Status] and reported one
//! by one on the optional `bus_errors` stream.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Baud, Hertz, Ohm, Second};

/// Largest data of a [Frame], in bytes (CAN FD).
pub const MAX_DATA_LEN: usize = 64;

/// Most filter elements of one `Can::set_filters`.
pub const MAX_FILTERS: usize = 256;

/// Most terminator values a device lists in [Capabilities::termination].
pub const MAX_TERMINATION_VALUES: usize = 4;

/// One CAN controller with its transceiver: configure it, receive on `rx`, send on the `tx` credit sink, watch its
/// [Status].
#[ww_trait]
pub trait Can {
    /// What this controller supports: clock, bit timing limits, CAN FD, modes, options, buffer and filter counts,
    /// timestamps, terminator values.
    fn capabilities() -> Capabilities<'i>;

    /// Configures the controller in one step and returns what was actually set: the bit timing as segments (also
    /// when a bit rate was asked for), the transceiver delay compensation in effect, and the options as the
    /// controller applies them. The device takes the controller out of the bus and back as it needs to (frames
    /// queued for sending stay queued). [Mode::Off] stops it.
    ///
    /// A mode, option or FD setting this controller doesn't have is `Unsupported`, bit timing outside
    /// [Capabilities::nominal] / [Capabilities::data] or a bit rate it can't reach within 1 % `InvalidArgument`,
    /// `InUse` when another instance sharing the clock keeps it from changing.
    #[errors(Common(Unsupported, InvalidArgument, InUse, NoPower))]
    fn configure(config: CanConfig) -> Result<CanConfig, CanError<'i>>;

    /// The configuration in effect, as `configure` last answered it: for a host attaching to a running node.
    fn config() -> CanConfig;

    /// Error state and counters of the controller. The device sends it to observers when the error state changes
    /// (and may on counter changes), so a bus-off reaches the host without polling.
    property!(ro status: Status);

    /// Received frames, in the order they were received. A counted stream, unit: frame; the head's time is when
    /// the frame was received (see [Capabilities::timestamps]), and the head counts the frames the device lost (a
    /// full receive FIFO, a full link buffer).
    #[counted]
    stream!(rx: RxFrame<'i>);

    /// Frames to send, in order, or by ID priority with [TxOrder::Priority]. Credit-based (API-28): the host never
    /// sends more than the device's transmit queue holds. A frame the controller can't send as configured (an FD
    /// frame with FD off, any frame in [Mode::ListenOnly]) is dropped, and reported on `tx_events` as
    /// [TxOutcome::Rejected] where present.
    #[credit]
    sink!(tx: TxFrame<'i>);

    /// Starts the bus-off recovery now (128 occurrences of 11 recessive bits, then error active). Left out by a
    /// device that only recovers automatically ([CanConfig::bus_off_recovery]). `WrongState` when not bus-off.
    #[optional]
    #[errors(Common(WrongState))]
    fn recover() -> Result<(), CanError<'i>>;

    /// Drops every frame queued for sending, including those already handed to the controller and not yet on the
    /// bus; answers how many were dropped. Each one is reported on `tx_events` as [TxOutcome::Aborted].
    #[optional]
    fn tx_abort() -> u32;

    /// When each sent frame went on the bus, or why it didn't: one event per frame of `tx` (and of `tx_buffer`),
    /// with its [TxFrame::marker]. A counted stream, unit: frame; the head's time is when the frame was sent.
    #[optional]
    #[counted]
    stream!(tx_events: TxEvent);

    /// Every error the controller detected on the bus: kind, phase and whether it was sending or receiving. A
    /// counted stream, unit: error; the head's time is when the error was detected. For analyzers and bus
    /// diagnostics; [Status] has the counters and the last error without it.
    #[optional]
    #[counted]
    stream!(bus_errors: BusError);

    /// Replaces the whole acceptance filter list and what happens to frames no filter matches. Without this item,
    /// a device receives every frame.
    ///
    /// More filters of a kind than the controller has is `OutOfFilters`, a match kind it can't do (a range, a
    /// mask) `Unsupported`, a FIFO or buffer index it doesn't have `InvalidArgument`.
    #[optional(filters)]
    #[errors(Common(Unsupported, InvalidArgument), OutOfFilters)]
    fn set_filters(filters: FilterConfig<'i>) -> Result<(), CanError<'i>>;

    /// The last frame a filter routed into dedicated receive buffer `index` ([FilterAction::Buffer]), and when it
    /// came; `None` when none came since the buffer was last read. For "the latest value of this ID" without
    /// streaming every frame. `InvalidArgument` for an index past [Capabilities::rx_buffers].
    #[optional(rx_buffers)]
    #[errors(Common(InvalidArgument))]
    fn rx_buffer(index: u16) -> Result<Option<BufferedFrame<'i>>, CanError<'i>>;

    /// Dedicated transmit buffer `index`: `frame` with no `period` is sent once now, with a `period` every
    /// `period` by the device until the buffer is written again; `None` clears the buffer (stops a cyclic frame,
    /// aborts a pending one). Its frames go out ahead of or behind the `tx` queue by ID priority, and are reported
    /// on `tx_events` with `marker`.
    ///
    /// `InvalidArgument` for an index past [Capabilities::tx_buffers], `Unsupported` for a period on a device
    /// without cyclic sending, `WrongState` in [Mode::Off] or a listen-only mode, `BusOff` while bus-off.
    #[optional(tx_buffers)]
    #[errors(Common(Unsupported, InvalidArgument, WrongState), BusOff)]
    fn tx_buffer(
        index: u16,
        frame: Option<TxFrame<'i>>,
        period: Option<Second>,
    ) -> Result<(), CanError<'i>>;

    /// Switches the terminator and answers the one in effect. `Unsupported` for a value not in
    /// [Capabilities::termination].
    #[optional(terminator)]
    #[errors(Common(Unsupported, NoPower))]
    fn set_termination(termination: Termination) -> Result<Termination, CanError<'i>>;

    /// The terminator in effect.
    #[optional(terminator)]
    property!(ro termination: Termination);

    /// In [Mode::Test]: drives the TX pin as `tx` says and answers the level on the RX pin. `WrongState` in any
    /// other mode.
    #[optional]
    #[errors(Common(WrongState))]
    fn test_pins(tx: TxPin) -> Result<BusLevel, CanError<'i>>;
}

/// An 11-bit (standard) or 29-bit (extended) CAN identifier.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash),
    self_describing,
    ww_repr = u1,
    no_future_version
)]
pub enum CanId {
    Standard(U11),
    Extended(U29),
}

impl CanId {
    /// A standard ID, `None` above `0x7FF`.
    pub const fn standard(id: u16) -> Option<Self> {
        match U11::new(id) {
            Some(id) => Some(CanId::Standard(id)),
            None => None,
        }
    }

    /// An extended ID, `None` above `0x1FFF_FFFF`.
    pub const fn extended(id: u32) -> Option<Self> {
        match U29::new(id) {
            Some(id) => Some(CanId::Extended(id)),
            None => None,
        }
    }

    /// The ID as a number, 11 or 29 bits.
    pub const fn raw(&self) -> u32 {
        match self {
            CanId::Standard(id) => id.value() as u32,
            CanId::Extended(id) => id.value(),
        }
    }

    pub const fn is_extended(&self) -> bool {
        matches!(self, CanId::Extended(_))
    }
}

/// A CAN frame: classic data, remote or CAN FD.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Clone, Debug, PartialEq, Eq),
    final_structure
)]
pub struct Frame<'i> {
    pub id: CanId,
    pub kind: FrameKind,
    /// 0-8 bytes for a classic frame, empty for a remote frame, 0-8, 12, 16, 20, 24, 32, 48 or 64 bytes for an FD
    /// frame (a device pads other lengths up to the next one with zeroes).
    pub data: MaxLen<RefVec<'i, u8>, MAX_DATA_LEN>,
}

/// Kind of a [Frame].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = u2,
    no_future_version
)]
pub enum FrameKind {
    /// Classic CAN data frame.
    Data,
    /// Classic CAN remote frame (RTR), asking for `len` bytes (its DLC, 0-8).
    Remote { len: U4 },
    /// CAN FD frame.
    Fd {
        /// Bit rate switch: the data phase runs at the data bit rate.
        brs: bool,
        /// Error state indicator: the sender was error passive.
        esi: bool,
    },
}

impl Frame<'_> {
    /// Data length of the frame (a remote frame's requested length).
    pub fn len(&self) -> usize {
        match self.kind {
            FrameKind::Remote { len } => len.value() as usize,
            _ => self.data.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_fd(&self) -> bool {
        matches!(self.kind, FrameKind::Fd { .. })
    }
}

/// A received frame, see `Can::rx`; when it was received is the time in the event head.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct RxFrame<'i> {
    pub frame: Frame<'i>,
    /// Index of the filter element that accepted it (into [FilterConfig::filters]); `None` when it was accepted as
    /// non-matching, or the device has no filters.
    pub filter: Option<UNib32>,
}

/// A frame to send, see `Can::tx` and `Can::tx_buffer`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct TxFrame<'i> {
    pub frame: Frame<'i>,
    /// Comes back in the frame's [TxEvent], so a host matches events to the frames it sent; `None` when it doesn't
    /// need to (a device may then skip the event).
    pub marker: Option<UNib32>,
}

/// A frame of a dedicated receive buffer, see `Can::rx_buffer`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct BufferedFrame<'i> {
    pub frame: Frame<'i>,
    /// When it was received, on the device's clock; `None` from a device without timestamps.
    pub time: Option<Second>,
}

/// What happened to a sent frame, see `Can::tx_events`; when is the time in the event head.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct TxEvent {
    /// [TxFrame::marker] of the frame.
    pub marker: Option<UNib32>,
    pub id: CanId,
    pub outcome: TxOutcome,
}

/// Outcome of sending one frame.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TxOutcome {
    /// Sent and acknowledged; the head's time is when it went on the bus.
    Sent,
    /// Dropped by `Can::tx_abort`, or by clearing its `Can::tx_buffer`.
    Aborted,
    /// One attempt failed with [Options::one_shot] (lost arbitration or an error).
    Failed,
    /// Not sendable as configured (an FD frame with FD off, a listen-only mode, bus-off with a full queue).
    Rejected,
}

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
    pub tec: u8,
    /// Receive error counter (some controllers stop at 127 and only flag error passive above).
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
    /// Stopped: [Mode::Off], or not configured yet.
    Stopped,
    /// Both counters below 96.
    Active,
    /// A counter at 96 or above (error warning).
    Warning,
    /// A counter above 127: sends passive error flags only.
    Passive,
    /// TEC above 255: off the bus until recovered.
    BusOff,
}

/// Everything a controller is configured with, see `Can::configure`: what is asked for going in, what was set
/// coming back.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct CanConfig {
    pub mode: Mode,
    /// Bit timing of the arbitration phase (all of a classic frame).
    pub nominal: Timing,
    /// CAN FD: `None` sends and accepts classic frames only.
    pub fd: Option<FdConfig>,
    pub options: Options,
    /// Recover from bus-off by itself this long after entering it (zero: at once); `None`: stay bus-off until
    /// `Can::recover`. A device that can't do one or the other answers `Unsupported`.
    pub bus_off_recovery: Option<Second>,
}

/// Operating mode of a controller.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Mode {
    /// Off the bus, TX recessive; the lowest power state the device keeps the controller in.
    Off,
    /// Sends, receives and acknowledges.
    Normal,
    /// Receives and acknowledges, but doesn't send frames or active error / overload flags (ISO 11898-1
    /// restricted operation); error counters are frozen.
    Restricted,
    /// Receives only, sends nothing at all, not even an acknowledge (bus monitoring, silent, listen-only).
    ListenOnly,
    /// Sent frames are received back internally; the bus is not driven and not read.
    InternalLoopback,
    /// Sent frames go on the bus and are received back, acknowledged or not.
    ExternalLoopback,
    /// Production test: `Can::test_pins` drives TX and reads RX.
    Test,
}

/// Bit timing of one phase: asked for as a bit rate or as segments, answered as segments.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug, PartialEq), ww_repr = nib)]
pub enum Timing {
    /// A bit rate and sample point, the device picks the segments.
    Rate {
        rate: Baud,
        /// Sample point in tenths of a percent of the bit (875 = 87.5 %).
        sample_point: u16,
    },
    /// Segments in time quanta of `prescaler / Capabilities::clock`.
    Segments(BitTiming),
}

impl Timing {
    /// The segments, `None` for a `Rate`.
    pub fn segments(&self) -> Option<BitTiming> {
        match self {
            Timing::Segments(bt) => Some(*bt),
            _ => None,
        }
    }
}

/// Bit timing in time quanta: a bit is `1 + seg1 + seg2` quanta of `prescaler` clock periods each, sampled after
/// `1 + seg1` (seg1 = propagation + phase segment 1, without the sync quantum, as SocketCAN and the M_CAN `NBTP`
/// count).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct BitTiming {
    pub prescaler: u16,
    pub seg1: u16,
    pub seg2: u8,
    /// Synchronization jump width.
    pub sjw: u8,
}

impl BitTiming {
    /// Time quanta per bit.
    pub fn quanta(&self) -> u32 {
        1 + self.seg1 as u32 + self.seg2 as u32
    }

    /// Bit rate at controller clock `clock_hz` ([Capabilities::clock]), in bit/s; `None` for a zero prescaler.
    pub fn bit_rate(&self, clock_hz: u32) -> Option<u32> {
        let div = self.prescaler as u32 * self.quanta();
        if div == 0 {
            return None;
        }
        Some(clock_hz / div)
    }

    /// Sample point in tenths of a percent of the bit.
    pub fn sample_point(&self) -> u16 {
        ((1 + self.seg1 as u32) * 1000 / self.quanta()) as u16
    }
}

/// CAN FD settings, see [CanConfig::fd].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct FdConfig {
    /// Bit timing of the data phase of frames with bit rate switch; the same as the nominal one when no frame
    /// switches.
    pub data: Timing,
    /// Transceiver delay compensation, needed from about 2 Mbit/s in the data phase.
    pub tdc: Tdc,
    /// ISO 11898-1:2015 CAN FD (with the stuff count); `false`: the original Bosch (non-ISO) CAN FD.
    pub iso: bool,
}

/// Transceiver delay compensation: where the controller checks a bit it sent in the data phase.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Tdc {
    Off,
    /// The device measures the delay and sets the offset at the data sample point.
    Auto,
    /// Secondary sample point at the measured delay plus `offset`, in minimum time quanta (clock periods);
    /// `filter`: shortest delay accepted as measured.
    Manual {
        offset: u8,
        filter: Option<u8>,
    },
}

/// Options of a controller, see [CanConfig::options]. A device answers `Unsupported` for one it doesn't have
/// ([Capabilities]), except where `false` / the first variant is what it always does.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Options {
    /// Send each frame once, no automatic retransmission after an error or lost arbitration.
    pub one_shot: bool,
    /// Treat a missing acknowledge as received (self test on a bus without other nodes).
    pub presume_ack: bool,
    /// Sample each bit three times (classic, nominal bit rate).
    pub triple_sample: bool,
    /// Wait two bit times after each sent frame, so other nodes of the same priority get the bus.
    pub tx_pause: bool,
    /// Order of the `tx` queue.
    pub tx_order: TxOrder,
    /// Which frames a full receive FIFO keeps.
    pub rx_overflow: RxOverflow,
}

/// Order the frames of `Can::tx` go on the bus.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1,
    no_future_version
)]
pub enum TxOrder {
    /// As queued.
    Fifo,
    /// Lowest ID first (as the bus would arbitrate them).
    Priority,
}

/// Which frames a full receive FIFO keeps; the lost ones are counted in the `rx` head either way.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1,
    no_future_version
)]
pub enum RxOverflow {
    /// Keeps the frames in the FIFO, drops the new one.
    KeepOldest,
    /// Overwrites the oldest frame with the new one.
    KeepNewest,
}

/// Acceptance filters, see `Can::set_filters`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct FilterConfig<'i> {
    /// Filter elements, first match wins.
    pub filters: MaxLen<RefVec<'i, Filter>, MAX_FILTERS>,
    /// What happens to a standard frame no filter matches.
    pub non_matching_standard: NonMatching,
    /// What happens to an extended frame no filter matches.
    pub non_matching_extended: NonMatching,
    /// Drop every remote frame.
    pub reject_remote: bool,
}

/// One filter element: which IDs it matches and what it does with them.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Filter {
    pub matching: FilterMatch,
    pub action: FilterAction,
}

/// IDs a [Filter] matches. Standard and extended IDs never match each other.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum FilterMatch {
    /// One ID.
    Exact(CanId),
    /// IDs equal to `id` in every bit set in `mask`.
    Mask { id: CanId, mask: U29 },
    /// IDs from `from` to `to`, both included, both of the same kind.
    Range { from: CanId, to: CanId },
}

/// What a [Filter] does with a frame it matches.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum FilterAction {
    /// Into receive FIFO `fifo` (0 when the controller has one), and from there on `rx`.
    Fifo(u8),
    /// Into dedicated receive buffer `index`, read with `Can::rx_buffer`; not sent on `rx`.
    Buffer(u16),
    /// Dropped.
    Reject,
}

/// What happens to a frame no [Filter] matches.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum NonMatching {
    /// Into receive FIFO `fifo`.
    Accept {
        fifo: u8,
    },
    Reject,
}

/// A terminator setting, see `Can::set_termination`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq),
    ww_repr = nib
)]
pub enum Termination {
    Off,
    On(Ohm),
}

/// What `Can::test_pins` drives the TX pin with.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TxPin {
    /// The controller drives it.
    Controller,
    /// The controller's sample point signal (for measuring the bit timing).
    SamplePoint,
    Dominant,
    Recessive,
}

/// Level on the bus.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1,
    no_future_version
)]
pub enum BusLevel {
    Dominant,
    Recessive,
}

/// Errors of `Can`: the common ones plus what is specific to CAN; each method lists the variants it returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum CanError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
    /// More filters than the controller has ({max} of this kind).
    OutOfFilters { max: u16 },
    /// Not possible while the controller is bus-off.
    BusOff,
}

/// Limits of one phase's [BitTiming] (as SocketCAN's `can_bittiming_const`).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct TimingLimits {
    pub prescaler_min: u16,
    pub prescaler_max: u16,
    pub seg1_min: u16,
    pub seg1_max: u16,
    pub seg2_min: u8,
    pub seg2_max: u8,
    pub sjw_max: u8,
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
    /// Automatic bus-off recovery ([CanConfig::bus_off_recovery] `Some`) is supported.
    pub auto_recovery: bool,
    /// How frames are timestamped; `None`: the heads carry no time.
    pub timestamps: Option<TimestampInfo>,
    /// Receive FIFOs ([FilterAction::Fifo] indices).
    pub rx_fifos: u8,
    /// Frames the receive FIFOs hold together.
    pub rx_fifo_depth: u16,
    /// Frames the transmit queue holds (the credit of `tx` is its size in bytes).
    pub tx_queue_depth: u16,
    /// Dedicated transmit buffers (`Can::tx_buffer`).
    pub tx_buffers: u16,
    /// Dedicated receive buffers (`Can::rx_buffer`).
    pub rx_buffers: u16,
    /// Filter elements for standard IDs.
    pub standard_filters: u16,
    /// Filter elements for extended IDs.
    pub extended_filters: u16,
    /// [FilterMatch::Mask] is supported.
    pub filter_mask: bool,
    /// [FilterMatch::Range] is supported.
    pub filter_range: bool,
    /// Terminator values `Can::set_termination` can switch in; empty without a switchable terminator.
    pub termination: MaxLen<RefVec<'i, Ohm>, MAX_TERMINATION_VALUES>,
}

#[cfg(feature = "embedded-can")]
mod embedded_can_impl {
    use super::*;

    impl From<embedded_can::Id> for CanId {
        fn from(id: embedded_can::Id) -> Self {
            match id {
                embedded_can::Id::Standard(id) => {
                    CanId::Standard(U11::new(id.as_raw()).unwrap_or(U11::new(0).unwrap()))
                }
                embedded_can::Id::Extended(id) => {
                    CanId::Extended(U29::new(id.as_raw()).unwrap_or(U29::new(0).unwrap()))
                }
            }
        }
    }

    impl From<CanId> for embedded_can::Id {
        fn from(id: CanId) -> Self {
            match id {
                // both are range checked by U11 / U29
                CanId::Standard(id) => embedded_can::StandardId::new(id.value())
                    .unwrap_or(embedded_can::StandardId::ZERO)
                    .into(),
                CanId::Extended(id) => embedded_can::ExtendedId::new(id.value())
                    .unwrap_or(embedded_can::ExtendedId::ZERO)
                    .into(),
            }
        }
    }

    #[cfg(feature = "std")]
    impl embedded_can::Frame for FrameOwned {
        fn new(id: impl Into<embedded_can::Id>, data: &[u8]) -> Option<Self> {
            if data.len() > 8 {
                return None;
            }
            Some(FrameOwned {
                id: id.into().into(),
                kind: FrameKind::Data,
                data: MaxLen::new(data.to_vec()).ok()?,
            })
        }

        fn new_remote(id: impl Into<embedded_can::Id>, dlc: usize) -> Option<Self> {
            if dlc > 8 {
                return None;
            }
            Some(FrameOwned {
                id: id.into().into(),
                kind: FrameKind::Remote {
                    len: U4::new(dlc as u8)?,
                },
                data: MaxLen::new(Vec::new()).ok()?,
            })
        }

        fn is_extended(&self) -> bool {
            self.id.is_extended()
        }

        fn is_remote_frame(&self) -> bool {
            matches!(self.kind, FrameKind::Remote { .. })
        }

        fn id(&self) -> embedded_can::Id {
            self.id.into()
        }

        /// The DLC of a classic frame; the data length of an FD frame (embedded-can has no FD frames).
        fn dlc(&self) -> usize {
            match self.kind {
                FrameKind::Remote { len } => len.value() as usize,
                _ => self.data.len(),
            }
        }

        fn data(&self) -> &[u8] {
            self.data.as_slice()
        }
    }
}
