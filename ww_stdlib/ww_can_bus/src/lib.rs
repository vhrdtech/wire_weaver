//! CAN bus over WireWeaver: a device (a USB-CAN adapter, a gateway, a node with a CAN port, a bus analyzer or
//! debugger) lets a host send and receive frames on its CAN buses, configure them and watch their state.
//!
//! # Traits
//!
//! | Trait | What it is | Who implements it |
//! |---|---|---|
//! | [Can] | One controller with its transceiver: configure, receive, send, error state; optional items for everything only some controllers have | Every device |
//! | [CanGroup] | Controllers that share clock, power and message RAM (all FDCAN instances of an STM32): `interface[]` of [Can], group enable / disable, clock source and frequency, group-wide performance | Devices whose controllers can't be clocked or powered alone |
//! | [CanTimeTriggered] | Time-triggered CAN (ISO 11898-4) of one controller: schedule, reference message, cycle state | Controllers with TTCAN (FDCAN1 of an STM32H7) |
//! | `CanXl` | CAN XL frames of one controller, behind the `xl` cargo feature | Controllers with CAN XL |
//!
//! A simple bridge puts its controllers straight into its API, `ww_impl!(can[]: ww_can_bus::Can)`, and implements
//! the six required items of [Can]. A device that mirrors its hardware puts a [CanGroup] there instead,
//! `ww_impl!(can: ww_can_bus::CanGroup)`, the controllers being `can.interface[i]`. [CanTimeTriggered] and `CanXl`
//! go next to them as arrays indexed like the controllers (`ww_impl!(can_xl[]: ww_can_bus::CanXl)`, element `i`
//! being controller `i`), on devices that have them.
//!
//! Everything only some controllers have is an [optional item](https://vhrdtech.github.io/wire_weaver/api/optional_items/)
//! of [Can] the device leaves out (`omit = ["can.filters", "can.transceiver", ..]`). A host finds out what is there
//! through introspection, and what the present items support through [Can::capabilities].
//!
//! # Debuggers
//!
//! `Can::configure` takes the controller off the bus and back as its hardware needs, so a node is set up in one call.
//! A bus debugger keeps every step in hand: `Can::set_mode` switches modes on their own ([Mode::Config] to stay off
//! the bus for configuration-only steps, [Mode::Restricted], [Mode::ListenOnly], the loopbacks, [Mode::Test],
//! [Mode::Sleep]), items that need configuration mode stay in it when called there, `Can::protocol_status` reads the
//! controller's raw protocol state, `bus_errors` reports every error, and `Can::test_pins` drives the pins.
//!
//! # Measured on the device
//!
//! What the device sees better than the host is measured there: receive and send times (in the stream heads), the
//! time a frame waited in the transmit queue ([TxEvent::queued]), bus load (`Can::bus_load`), the transceiver loop
//! delay ([ProtocolStatus::tdc_value]) and the bit timing of an unknown bus (`Can::detect_timing`).
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
//! [CanError::Common], plus what is specific to CAN (`OutOfFilters`, `OutOfMemory`, `BusOff`). Each method lists the
//! variants it returns (`#[errors]`). Errors on the bus are not errors of a call: they are counted in [Status] and
//! reported one by one on the optional `bus_errors` stream.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Baud, Hertz, Ohm, Second};

/// Largest data of a [Frame], in bytes (CAN FD).
pub const MAX_DATA_LEN: usize = 64;

/// Most filter elements of one `Can::set_filters`.
pub const MAX_FILTERS: usize = 256;

/// Most terminator values a device lists in [Capabilities::termination].
pub const MAX_TERMINATION_VALUES: usize = 4;

/// Most bit timings one `Can::detect_timing` tries.
pub const MAX_TIMING_CANDIDATES: usize = 16;

/// Most clock sources a [CanGroup] lists.
pub const MAX_CLOCK_SOURCES: usize = 8;

/// Longest clock source name.
pub const MAX_NAME_LEN: usize = 32;

/// Most triggers of one `CanTimeTriggered::set_schedule`.
pub const MAX_TRIGGERS: usize = 64;

/// One CAN controller with its transceiver: configure it, receive on `rx`, send on the `tx` credit sink, watch its
/// [Status].
#[ww_trait]
#[claim(exclusive)]
pub trait Can {
    /// What this controller supports: clock, bit timing limits, CAN FD, modes, options, memory, filters,
    /// timestamps, transceiver and terminator.
    fn capabilities() -> Capabilities<'i>;

    /// Configures the controller in one step and returns what was actually set: the bit timing as segments (also
    /// when a bit rate was asked for), the transceiver delay compensation in effect, the options as the controller
    /// applies them. The device takes the controller off the bus and back as it needs to (frames queued for sending
    /// stay queued) and ends in [CanConfig::mode].
    ///
    /// A mode, option or FD setting this controller doesn't have is `Unsupported`, bit timing outside
    /// [Capabilities::nominal] / [Capabilities::data] or a bit rate it can't reach exactly `InvalidArgument`,
    /// `InUse` when another controller sharing the clock keeps it from changing, `WrongState` while its [CanGroup]
    /// is disabled.
    #[errors(Common(Unsupported, InvalidArgument, InUse, WrongState, NoPower))]
    fn configure(config: CanConfig) -> Result<CanConfig, CanError<'i>>;

    /// The configuration in effect, as `configure` last answered it (with the mode now in effect): for a host
    /// attaching to a running node.
    fn config() -> CanConfig;

    /// Error state and counters of the controller. The device sends it to observers when the error state changes
    /// (and may on counter changes), so a bus-off or a wake-up reaches the host without polling.
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

    /// Switches the mode alone, keeping the bit timing, options, filters and memory layout, and answers the mode
    /// now in effect: [Mode::Config] to stay off the bus for configuration-only steps, back to [Mode::Normal], into
    /// [Mode::Restricted], [Mode::ListenOnly], a loopback, [Mode::Test] or [Mode::Sleep]. `Unsupported` for a mode
    /// this controller doesn't have.
    #[optional]
    #[errors(Common(Unsupported, WrongState))]
    fn set_mode(mode: Mode) -> Result<Mode, CanError<'i>>;

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
    /// with its [TxFrame::marker] and the time it waited in the queue. A counted stream, unit: frame; the head's
    /// time is when the frame was sent.
    #[optional]
    #[counted]
    stream!(tx_events: TxEvent);

    /// Every error the controller detected on the bus: kind, phase and whether it was sending or receiving. A
    /// counted stream, unit: error; the head's time is when the error was detected. For analyzers and bus
    /// debuggers; [Status] has the counters and the last error without it.
    #[optional]
    #[counted]
    stream!(bus_errors: BusError);

    /// The controller's protocol state as it reads it: activity, counters, the error logging counter, last error
    /// codes of both phases, flags of the last received FD frame, protocol exception, measured transceiver delay.
    /// Reading resets what the controller resets on a read (last error codes, error logging counter).
    #[optional]
    fn protocol_status() -> ProtocolStatus;

    /// Bus load the device measures over [BusLoad::window], sent to observers once per window.
    #[optional]
    property!(ro bus_load: BusLoad);

    /// Finds the bit timing of a running bus: listens without acknowledging ([Mode::ListenOnly]) at each of
    /// `candidates` in turn for up to `per_candidate`, and answers the first one a frame was received at without
    /// errors (the controller then stays configured with it, in listen-only), or `None` (the configuration is left
    /// as it was). `InvalidArgument` for an empty list or a timing outside the limits.
    #[optional]
    #[errors(Common(InvalidArgument, Unsupported, InUse))]
    fn detect_timing(
        candidates: MaxLen<RefVec<'i, Timing>, MAX_TIMING_CANDIDATES>,
        per_candidate: Second,
    ) -> Result<Option<CanConfig>, CanError<'i>>;

    /// Replaces the whole acceptance filter list and what happens to frames no filter matches. Without this item,
    /// a device receives every frame.
    ///
    /// More filters of a kind than the controller has (or than its memory layout holds) is `OutOfFilters`, a match
    /// kind it can't do (a range, a mask) `Unsupported`, a FIFO or buffer index it doesn't have `InvalidArgument`.
    #[optional(filters)]
    #[errors(Common(Unsupported, InvalidArgument), OutOfFilters)]
    fn set_filters(filters: FilterConfig<'i>) -> Result<(), CanError<'i>>;

    /// Divides the controller's message memory (M_CAN message RAM, MCP2518FD FIFO RAM) between filters, FIFOs,
    /// buffers and the TX event FIFO, and answers the layout in effect. Only on controllers with a configurable
    /// layout ([Capabilities::memory]). `OutOfMemory` when it doesn't fit (also next to the other controllers of its
    /// group sharing the memory), `WrongState` while frames are queued.
    #[optional]
    #[errors(Common(Unsupported, InvalidArgument, WrongState), OutOfMemory)]
    fn set_layout(layout: MemoryLayout) -> Result<MemoryLayout, CanError<'i>>;

    /// The last frame a filter routed into dedicated receive buffer `index` ([FilterAction::Buffer]), and when it
    /// came; `None` when none came since the buffer was last read. For "the latest value of this ID" without
    /// streaming every frame. `InvalidArgument` for an index past [Capabilities::rx_buffers].
    #[optional(rx_buffers)]
    #[errors(Common(InvalidArgument))]
    fn rx_buffer(index: u16) -> Result<Option<BufferedFrame<'i>>, CanError<'i>>;

    /// Dedicated transmit buffer `index`: `frame` with no `period` is sent once now, with a `period` every
    /// `period` by the device until the buffer is written again; `None` clears the buffer (stops a cyclic frame,
    /// aborts a pending one). Its frames go out ahead of or behind the `tx` queue by ID priority, and are reported
    /// on `tx_events` with their marker.
    ///
    /// `InvalidArgument` for an index past [Capabilities::tx_buffers], `Unsupported` for a period on a device
    /// without cyclic sending, `WrongState` in a mode that doesn't send, `BusOff` while bus-off.
    #[optional(tx_buffers)]
    #[errors(Common(Unsupported, InvalidArgument, WrongState), BusOff)]
    fn tx_buffer(
        index: u16,
        frame: Option<TxFrame<'i>>,
        period: Option<Second>,
    ) -> Result<(), CanError<'i>>;

    /// Switches the transceiver (standby pin, silent pin, sleep) and answers the mode in effect. `Unsupported` for
    /// a mode it doesn't have ([Capabilities::transceiver_modes]).
    #[optional(transceiver)]
    #[errors(Common(Unsupported, NoPower))]
    fn set_transceiver(mode: TransceiverMode) -> Result<TransceiverMode, CanError<'i>>;

    /// Transceiver mode and the bus wiring faults it detects; sent to observers when a fault appears or clears.
    #[optional(transceiver)]
    property!(ro transceiver_state: TransceiverState);

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

/// Controllers that share a clock, power and message memory, as all FDCAN instances of an STM32: none of them can be
/// clocked, powered down or re-laid out alone. The group is enabled (clocked) or disabled as a whole; each
/// controller is `interface[i]`, configured on its own once the group is enabled.
#[ww_trait]
#[claim(exclusive)]
pub trait CanGroup {
    /// The controllers of the group.
    ww_impl!(interface[]: Can);

    /// What the group has: number of controllers, clock sources and frequencies, shared message memory.
    fn group_capabilities() -> GroupCapabilities<'i>;

    /// Enables the group's clock (the source and frequency set last, or the device's default) and answers the clock
    /// in effect. Every controller is then in [Mode::Off] until configured. `WrongState` when already enabled.
    #[errors(Common(WrongState, NoPower))]
    fn enable() -> Result<GroupClock, CanError<'i>>;

    /// Disables the group's clock, its lowest power state. Every controller must be in [Mode::Off] first
    /// (`WrongState` names the one that isn't).
    #[errors(Common(WrongState))]
    fn disable() -> Result<(), CanError<'i>>;

    /// Enabled (with the clock in effect) or disabled; sent to observers on every change.
    property!(ro group_state: GroupState);

    /// Selects the group's clock source and frequency, and answers what was set (the nearest frequency the source's
    /// dividers reach). Only while disabled (`WrongState`); a source or frequency the group doesn't have is
    /// `Unsupported`. For advanced nodes that pick a clock to hit an exact bit timing; a bridge uses the default.
    #[optional(clock)]
    #[errors(Common(Unsupported, WrongState))]
    fn set_clock(clock: GroupClock) -> Result<GroupClock, CanError<'i>>;

    /// What the group guarantees together, under the current clock and system load: a device whose controllers
    /// share a bus to the host or a CPU knows it, the host can't.
    #[optional]
    fn performance() -> Performance;
}

/// Time-triggered CAN (ISO 11898-4, TTCAN) of one controller: a schedule of time windows in a matrix of basic
/// cycles started by a reference message. A device puts it next to its controllers, `ww_impl!(can_tt[]:
/// ww_can_bus::CanTimeTriggered)`, element `i` being controller `i` (valid only where the hardware has TTCAN).
#[ww_trait]
#[claim(exclusive)]
pub trait CanTimeTriggered {
    /// Configures time-triggered operation (`None` switches it off, back to event-driven CAN) and answers what was
    /// set. The controller must be in [Mode::Config] or [Mode::Off] (`WrongState`).
    #[errors(Common(Unsupported, InvalidArgument, WrongState))]
    fn configure_tt(config: Option<TtConfig>) -> Result<Option<TtConfig>, CanError<'i>>;

    /// Replaces the trigger list (time marks of the matrix cycle). `OutOfMemory` when it doesn't fit the trigger
    /// memory, `InvalidArgument` for an unsorted list or a buffer the controller doesn't have.
    #[errors(Common(InvalidArgument, WrongState), OutOfMemory)]
    fn set_schedule(
        triggers: MaxLen<RefVec<'i, Trigger>, MAX_TRIGGERS>,
    ) -> Result<(), CanError<'i>>;

    /// Synchronisation state, master role, cycle count and error level; sent to observers on every change of the
    /// state or the error level.
    property!(ro tt_status: TtStatus);
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
    /// Classic CAN data frame with 8 data bytes and a DLC of 9-15 on the wire (ISO 11898-1 allows them; SocketCAN
    /// `CAN_CTRLMODE_CC_LEN8_DLC`). For bus debuggers; nodes send [FrameKind::Data].
    DataLongDlc { dlc: U4 },
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
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct TxEvent {
    /// [TxFrame::marker] of the frame.
    pub marker: Option<UNib32>,
    pub id: CanId,
    pub outcome: TxOutcome,
    /// How long the frame waited from the device accepting it to its start of frame on the bus (queue, arbitration
    /// and retransmissions), measured on the device; `None` from a device that doesn't measure it.
    pub queued: Option<Second>,
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
    /// Not sendable as configured (an FD frame with FD off, a mode that doesn't send, bus-off with a full queue).
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
    pub error_log: Option<u8>,
    /// Flags of the last received FD frame, if one came since the last read.
    pub last_fd_frame: Option<FdFrameFlags>,
    /// A protocol exception happened since the last read.
    pub protocol_exception: bool,
    /// Transceiver delay the controller measured, in minimum time quanta (clock periods), with TDC on.
    pub tdc_value: Option<u8>,
    /// Corrected and uncorrected bit errors of the message memory (ECC / parity) since the last read, where the
    /// controller detects them.
    pub memory_errors: Option<u16>,
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
    /// Busy time of the bus in the window, in tenths of a percent (frames with their stuff bits, error and overload
    /// frames, interframe space not counted).
    pub load: u16,
    /// Frames seen on the bus in the window, sent and received.
    pub frames: u32,
    /// Error frames in the window.
    pub errors: u32,
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
    /// Powered down (clock gated where the controller can be alone), off the bus, TX recessive.
    Off,
    /// Off the bus, TX recessive, everything configurable (M_CAN `INIT` + `CCE`, bxCAN initialization, MCP2518FD
    /// configuration mode); the mode to stay in for configuration-only steps.
    Config,
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
    /// Low power, off the bus; wakes up on bus activity with [Options::wake_on_bus], or on `Can::set_mode`.
    Sleep,
}

/// Bit timing of one phase: asked for as a bit rate or as segments, answered as segments.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq),
    ww_repr = nib
)]
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
/// ([Capabilities]), except where the value asked for is what it always does.
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
    /// Hard synchronization only on two dominant time quanta after a recessive one (M_CAN `CCCR.EFBI`), against
    /// glitches on long buses.
    pub edge_filtering: bool,
    /// Protocol exception handling (ISO 11898-1): on a frame of a newer protocol, wait for bus idle instead of
    /// signalling an error. On by default; off for buses where every such frame is an error.
    pub protocol_exception: bool,
    /// In [Mode::Sleep], wake up on bus activity.
    pub wake_on_bus: bool,
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

/// How a controller's message memory is divided, see `Can::set_layout`; counts in elements.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct MemoryLayout {
    pub standard_filters: u16,
    pub extended_filters: u16,
    /// Elements of each receive FIFO, FIFO 0 first (0: not used).
    pub rx_fifos: [u16; 2],
    pub rx_buffers: u16,
    pub tx_buffers: u16,
    /// Elements of the transmit FIFO / queue.
    pub tx_queue: u16,
    pub tx_events: u16,
    /// Data bytes an element holds (8, 12, 16, 20, 24, 32, 48 or 64); frames with more are cut (and flagged by the
    /// controller).
    pub element_data: u8,
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
    /// Into receive FIFO `fifo` (0 when the controller has one), and from there on `rx`; `high_priority` makes the
    /// device pass it on ahead of other frames (M_CAN high priority message).
    Fifo { fifo: u8, high_priority: bool },
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

/// Transceiver mode, see `Can::set_transceiver`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TransceiverMode {
    Normal,
    /// Receiver on at low power (wake-up detection), driver off: the standby pin.
    Standby,
    /// Receiver on, driver off: the silent pin of transceivers that have one.
    Silent,
    /// Lowest power; woken by the bus (wake-up pattern) or the device.
    Sleep,
}

/// Transceiver mode and faults, see `Can::transceiver_state`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct TransceiverState {
    pub mode: TransceiverMode,
    /// A bus wiring fault the transceiver reports, if any (and if it can).
    pub fault: Option<TransceiverFault>,
}

/// A bus wiring fault (as SocketCAN's `CAN_ERR_TRX_*`).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TransceiverFault {
    CanHighNoWire,
    CanHighShortToBattery,
    CanHighShortToSupply,
    CanHighShortToGround,
    CanLowNoWire,
    CanLowShortToBattery,
    CanLowShortToSupply,
    CanLowShortToGround,
    CanLowShortToCanHigh,
    /// TX held dominant too long, the driver was switched off.
    DominantTimeout,
    /// Over temperature.
    Thermal,
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

/// Errors of the CAN traits: the common ones plus what is specific to CAN; each method lists the variants it
/// returns.
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
    /// Doesn't fit the controller's memory ({available} elements left).
    OutOfMemory { available: u32 },
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

/// Throughput a controller or group guarantees, see `CanGroup::performance` and [Capabilities::performance].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Performance {
    /// Frames per second received and passed on without loss, at any frame length.
    pub min_frames_per_second: Option<u32>,
    /// Data bytes per second passed on without loss.
    pub min_bytes_per_second: Option<u32>,
    /// The figures hold whatever else the device does (dedicated DMA, a core of its own).
    pub independent_of_system: bool,
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
    /// Message memory for `Can::set_layout`, in 32-bit words; `None`: fixed layout.
    pub memory: Option<u32>,
    /// [TransceiverMode]s `Can::set_transceiver` can switch to.
    pub transceiver_modes: TransceiverModes,
    /// Terminator values `Can::set_termination` can switch in; empty without a switchable terminator.
    pub termination: MaxLen<RefVec<'i, Ohm>, MAX_TERMINATION_VALUES>,
    /// What this controller guarantees on its own; `None` when only its group's figure makes sense.
    pub performance: Option<Performance>,
}

/// Which [TransceiverMode]s besides `Normal` a transceiver has.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct TransceiverModes {
    pub standby: bool,
    pub silent: bool,
    pub sleep: bool,
}

/// Clock of a [CanGroup]: a source and the frequency the controllers get from it.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct GroupClock {
    /// Index into [GroupCapabilities::clock_sources].
    pub source: u8,
    /// Frequency the controllers get (after the group's divider).
    pub frequency: Hertz,
}

/// Enabled or disabled, see `CanGroup::group_state`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq),
    ww_repr = nib
)]
pub enum GroupState {
    /// Clock off, the lowest power state.
    Disabled,
    Enabled(GroupClock),
}

/// A clock source of a [CanGroup].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct ClockSource<'i> {
    /// "HSE", "PLL1Q", "PLL2P".
    pub name: MaxLen<&'i str, MAX_NAME_LEN>,
    /// Lowest frequency the group's dividers make of it.
    pub min: Hertz,
    /// Highest one.
    pub max: Hertz,
}

/// What a [CanGroup] has, see `CanGroup::group_capabilities`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct GroupCapabilities<'i> {
    /// Controllers in `interface[]`.
    pub interfaces: u8,
    /// Clock sources `CanGroup::set_clock` chooses from; empty when the clock is fixed.
    pub clock_sources: MaxLen<RefVec<'i, ClockSource<'i>>, MAX_CLOCK_SOURCES>,
    /// The clock `CanGroup::enable` uses without `set_clock`.
    pub default_clock: GroupClock,
    /// Message memory shared by the controllers, in 32-bit words; `None` when each has its own.
    pub shared_memory: Option<u32>,
}

/// Time-triggered configuration of a controller, see `CanTimeTriggered::configure_tt`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct TtConfig {
    /// ISO 11898-4 level 1 (time-triggered messaging) or level 2 (plus global time).
    pub level: TtLevel,
    pub role: TtRole,
    /// ID of the reference message that starts every basic cycle.
    pub reference_id: CanId,
    /// Basic cycles in a matrix cycle: 1, 2, 4, 8, 16, 32 or 64.
    pub matrix_cycles: u8,
    /// Time unit ratio: one network time unit (NTU) is `ntu_numerator / ntu_denominator` local clock periods
    /// (M_CAN `TURCF`); level 2 adjusts it to the master's clock.
    pub ntu_numerator: u32,
    pub ntu_denominator: u16,
    /// Application watchdog limit in 256 NTU steps; `None` disables it.
    pub watchdog: Option<u8>,
}

/// ISO 11898-4 level.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TtLevel {
    Level1,
    Level2,
    /// Event-synchronized time-triggered operation (level 0 with external events).
    Level0,
}

/// Role in a time-triggered network.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TtRole {
    /// Time slave only.
    Slave,
    /// Potential time master with `priority` (0 highest, 0-7, the three lowest bits of the reference ID).
    Master { priority: u8 },
}

/// One trigger of a time-triggered schedule, see `CanTimeTriggered::set_schedule`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Trigger {
    /// Time mark within the basic cycle, in cycle time units.
    pub time_mark: u16,
    /// Basic cycles it fires in: every `repeat`-th (1, 2, 4, .., 64) starting at cycle `offset`.
    pub repeat: u8,
    pub offset: u8,
    pub kind: TriggerKind,
}

/// What a [Trigger] does.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TriggerKind {
    /// Sends the reference message (time master).
    TxReference,
    /// Sends dedicated TX buffer `buffer` alone in its window.
    TxExclusive { buffer: u16 },
    /// Starts a merged arbitrating window with TX buffer `buffer`.
    TxArbitratingStart { buffer: u16 },
    /// Ends a merged arbitrating window.
    TxArbitratingEnd,
    /// Checks that the frame of filter `filter` was received in the cycle.
    RxCheck { filter: u16 },
    /// Watches for a missing reference message.
    Watch,
    /// Ends the schedule of the cycle.
    EndOfList,
}

/// State of time-triggered operation, see `CanTimeTriggered::tt_status`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct TtStatus {
    pub sync: TtSync,
    /// This controller is the current time master.
    pub is_master: bool,
    /// Basic cycle within the matrix cycle.
    pub cycle: u8,
    /// ISO 11898-4 error level: 0 no error, 1 warning, 2 error, 3 severe error.
    pub error_level: u8,
}

/// Synchronisation state of time-triggered operation.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TtSync {
    /// Configured, waiting for the first reference message.
    Off,
    Synchronizing,
    InGap,
    InSchedule,
}

/// Largest data of an [XlFrame], in bytes.
#[cfg(feature = "xl")]
pub const MAX_XL_DATA_LEN: usize = 2048;

/// CAN XL frames of one controller (behind the `xl` cargo feature, so devices without XL don't carry its 2 KiB
/// frames): a device puts it next to its controllers, `ww_impl!(can_xl[]: ww_can_bus::CanXl)`, element `i` being
/// controller `i`. Classic and FD frames of the same controller stay on `Can::rx` / `Can::tx`.
#[cfg(feature = "xl")]
#[ww_trait]
#[claim(exclusive)]
pub trait CanXl {
    /// Configures the XL data phase (`None`: no XL frames) and answers what was set. Needs CAN FD configured on the
    /// controller (`WrongState`); a timing or transceiver mode switching it can't do is `Unsupported`.
    #[errors(Common(Unsupported, InvalidArgument, WrongState))]
    fn configure_xl(config: Option<XlConfig>) -> Result<Option<XlConfig>, CanError<'i>>;

    /// Received XL frames. A counted stream, unit: frame; the head's time is when the frame was received.
    #[counted]
    stream!(rx_xl: XlFrame<'i>);

    /// XL frames to send; credit-based. Reported on `Can::tx_events` with their marker, where present.
    #[credit]
    sink!(tx_xl: XlTxFrame<'i>);
}

/// A CAN XL frame.
#[cfg(feature = "xl")]
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct XlFrame<'i> {
    /// Priority ID (11 bits), arbitrated like a standard ID.
    pub priority: U11,
    /// Virtual CAN network ID; `None` when not used.
    pub vcid: Option<u8>,
    /// SDU type: what the data carries (CiA 611-1).
    pub sdt: u8,
    /// Simple extended content: CADH (CiA 613-2) present.
    pub sec: bool,
    /// Acceptance field (32 bits).
    pub af: u32,
    /// 1-2048 bytes.
    pub data: MaxLen<RefVec<'i, u8>, MAX_XL_DATA_LEN>,
}

/// An XL frame to send, with a marker for `Can::tx_events`.
#[cfg(feature = "xl")]
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct XlTxFrame<'i> {
    pub frame: XlFrame<'i>,
    pub marker: Option<UNib32>,
}

/// XL data phase settings, see `CanXl::configure_xl`.
#[cfg(feature = "xl")]
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct XlConfig {
    /// Bit timing of the XL data phase.
    pub data: Timing,
    pub tdc: Tdc,
    /// Transceiver mode switching (CAN SIC XL transceivers): the data phase as PWM symbols; `None` for an SIC
    /// transceiver without switching (data phase up to about 5 Mbit/s).
    pub pwm: Option<XlPwm>,
    /// Error signalling in the data phase; off for PWM mode, where errors are left to higher layers.
    pub error_signaling: bool,
}

/// PWM symbol timing of the XL data phase, in minimum time quanta (clock periods).
#[cfg(feature = "xl")]
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct XlPwm {
    pub short: u8,
    pub long: u8,
    pub offset: u8,
}
