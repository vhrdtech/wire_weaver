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
use ww_si::prefix::{Milli, One};
use ww_si::units::{self, Per};
pub use ww_si::{Baud, Hertz, Ohm, Second};
use ww_si::{Fixed, Index, Range};

pub mod capabilities;
pub use capabilities::{
    Capabilities, MAX_TERMINATION_VALUES, MemoryLayout, TimestampInfo, TimestampPoint, TimingLimits,
};
pub mod config;
pub use config::{BitTiming, CanConfig, FdConfig, Mode, Options, RxOverflow, Tdc, Timing, TxOrder};
pub mod error;
pub use error::CanError;
pub mod filter;
pub use filter::{Filter, FilterAction, FilterConfig, FilterMatch, MAX_FILTERS, NonMatching};
pub mod frame;
pub use frame::{
    BufferedFrame, CanId, Frame, FrameKind, MAX_DATA_LEN, RxFrame, TxEvent, TxFrame, TxOutcome,
};
pub mod group;
pub use group::{
    CanGroup, ClockSource, GroupCapabilities, GroupClock, GroupState, MAX_CLOCK_SOURCES,
    MAX_NAME_LEN, Performance,
};
pub mod status;
pub use status::{
    Activity, BusError, BusLoad, Direction, ErrorState, FdFrameFlags, Phase, ProtocolError,
    ProtocolStatus, Status,
};
pub mod time_triggered;
pub use time_triggered::{
    CanTimeTriggered, MAX_TRIGGERS, Trigger, TriggerKind, TtConfig, TtErrorLevel, TtLevel, TtRole,
    TtStatus, TtSync,
};
pub mod transceiver;
pub use transceiver::{
    BusLevel, Termination, TransceiverFault, TransceiverMode, TransceiverModes, TransceiverState,
    TxPin,
};
#[cfg(feature = "xl")]
pub mod xl;
#[cfg(feature = "xl")]
pub use xl::{MAX_XL_DATA_LEN, VcId, XlConfig, XlFrame, XlPwm, XlTxFrame};

/// Most bit timings one `Can::detect_timing` tries.
pub const MAX_TIMING_CANDIDATES: usize = 16;

/// A filter element: its place in [FilterConfig::filters] (0-255, [MAX_FILTERS] of them).
pub type FilterIndex = Index<units::Filter, UNib32, Range<0, 255>>;

/// A dedicated receive or transmit buffer of the controller (`Can::rx_buffer`, `Can::tx_buffer`), from 0.
pub type BufferIndex = Index<units::Buffer, u16>;

/// A receive FIFO of the controller, from 0 ([Capabilities::rx_fifos] of them).
pub type FifoIndex = Index<units::Fifo, u8>;

/// Clock periods of the controller ([Capabilities::clock]) per time quantum: the bit timing prescaler.
pub type Prescaler = Fixed<Per<units::Cycle, units::TimeQuantum>, One, u16>;

/// A share in tenths of a percent, 0-1000 (875 = 87.5 %): bus load and sample point.
pub type Share = Fixed<units::Ratio, Milli, u16, Range<0, 1000>>;

/// Marker of a frame to send ([TxFrame::marker]), handed back in its [TxEvent]; the host picks the numbers.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    transparent,
    derive(Copy, Clone, Debug, PartialEq, Eq, Hash)
)]
pub struct TxMarker(pub UNib32);

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
    fn tx_abort() -> Fixed<units::Frame, One, u32>;

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
    fn rx_buffer(index: BufferIndex) -> Result<Option<BufferedFrame<'i>>, CanError<'i>>;

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
        index: BufferIndex,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes<T: SerializeShrinkWrap>(v: &T) -> Vec<u8> {
        let mut buf = [0u8; 64];
        v.to_ww_bytes(&mut buf).unwrap().to_vec()
    }

    /// The newtypes and typed numbers are the bytes of their plain number (DER-18, DER-20).
    #[test]
    fn typed_numbers_are_their_plain_bytes() {
        assert_eq!(bytes(&TxMarker(UNib32(69))), bytes(&UNib32(69)));
        assert_eq!(bytes(&Some(TxMarker(UNib32(5)))), bytes(&Some(UNib32(5))));
        assert_eq!(
            bytes(&FilterIndex::try_new(UNib32(200)).unwrap()),
            bytes(&UNib32(200))
        );
        assert_eq!(bytes(&Share::try_new(875).unwrap()), bytes(&875u16));
        assert_eq!(bytes(&Prescaler::new(3)), bytes(&3u16));
        #[cfg(feature = "xl")]
        assert_eq!(bytes(&VcId(0xA5)), bytes(&0xA5u8));
    }

    #[test]
    fn bit_timing_helpers() {
        let bt = BitTiming {
            prescaler: Prescaler::new(1),
            seg1: Fixed::new(139),
            seg2: Fixed::new(20),
            sjw: Fixed::new(20),
        };
        assert_eq!(bt.quanta().get(), 160);
        assert_eq!(
            bt.bit_rate(Hertz::mega(80u8)).map(|r| r.get()),
            Some(500_000)
        );
        assert_eq!(bt.sample_point().get(), 875);
    }
}
