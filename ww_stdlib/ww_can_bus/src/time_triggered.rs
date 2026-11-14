use wire_weaver::prelude::*;
use ww_si::prefix::One;
use ww_si::units;
use ww_si::{Fixed, Index, Range};

use crate::error::CanError;
use crate::{BufferIndex, CanId, FilterIndex};

/// Most triggers of one `CanTimeTriggered::set_schedule`.
pub const MAX_TRIGGERS: usize = 64;

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
    pub matrix_cycles: Fixed<units::Cycle, One, u8, Range<1, 64>>,
    /// Time unit ratio: one network time unit (NTU) is `ntu_numerator / ntu_denominator` local clock periods
    /// (M_CAN `TURCF`); level 2 adjusts it to the master's clock.
    #[no_unit_hint = "the controller's TURCF register field as it is, half of a ratio with ntu_denominator"]
    pub ntu_numerator: u32,
    #[no_unit_hint = "the controller's TURCF register field as it is, half of a ratio with ntu_numerator"]
    pub ntu_denominator: u16,
    /// Application watchdog limit in 256 NTU steps; `None` disables it.
    #[no_unit_hint = "TTOCF.AWL register field as the controller takes it, in device-defined 256 NTU steps, not a \
                       plain duration"]
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
    Master { priority: U3 },
}

/// One trigger of a time-triggered schedule, see `CanTimeTriggered::set_schedule`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Trigger {
    /// Time mark within the basic cycle, in network time units (NTU, [TtConfig]) from its start.
    pub time_mark: Index<units::Tick, u16>,
    /// Basic cycles it fires in: every `repeat`-th (1, 2, 4, .., 64) starting at cycle `offset`.
    pub repeat: Fixed<units::Cycle, One, u8, Range<1, 64>>,
    pub offset: Index<units::Cycle, u8, Range<0, 63>>,
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
    TxExclusive { buffer: BufferIndex },
    /// Starts a merged arbitrating window with TX buffer `buffer`.
    TxArbitratingStart { buffer: BufferIndex },
    /// Ends a merged arbitrating window.
    TxArbitratingEnd,
    /// Checks that the frame of filter `filter` was received in the cycle.
    RxCheck { filter: FilterIndex },
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
    pub cycle: Index<units::Cycle, u8, Range<0, 63>>,
    pub error_level: TtErrorLevel,
}

/// ISO 11898-4 error level of time-triggered operation (M_CAN `TTOST.EL`).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u2,
    no_future_version
)]
pub enum TtErrorLevel {
    /// S0: no error.
    None,
    /// S1: warning, only the notification of the application.
    Warning,
    /// S2: error, transmissions in exclusive windows stop.
    Error,
    /// S3: severe error, all transmissions stop.
    Severe,
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
