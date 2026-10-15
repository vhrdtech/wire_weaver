//! Device time and synchronisation over WireWeaver.
//!
//! Every device has one free-running clock that timestamps everything it captures, as [Second]s since the clock
//! started (see `docs/std_library/io_family.md` for how this ties into the other IO traits). `Clock` lets a host
//! read it in full (to unwrap fixed-width
//! timestamps and correlate with host time), and synchronise several devices: one is the master on a sync line,
//! the others lock to it and publish `time_map` points that map their local time to the shared one.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_date_time::DateTime;
pub use ww_si::Second;

/// Device clock: reads device time, and synchronises it with other devices over a sync line (IRIG-B, PPS, ...).
///
/// A device has exactly one free-running clock. Every timestamp it reports elsewhere in its API is a [Second]
/// since that clock started, in whatever fixed width fits the field (so it wraps: a `U32` of microseconds wraps
/// every 71.6 minutes). `now()` gives the full-width value, so a host can unwrap those truncated timestamps and
/// correlate device time with its own.
#[ww_trait]
pub trait Clock {
    /// Device time now, full width: the value every truncated timestamp elsewhere in this device's API wraps
    /// around.
    fn now() -> Second;

    /// Static description of this clock: its resolution, what it calls itself, and which [SyncFormat]s it
    /// supports.
    fn info() -> ClockInfo<'i>;

    /// Local time mapped to the shared sync time, sent about once a second while synced and on every
    /// [SyncState] change, so a host can line up this device's timeline with others and see the unsynced spans.
    stream!(time_map: TimeMapPoint);

    /// Switch sync off, or make this device the master on the sync line, or a slave locking to one, in the given
    /// [SyncFormat]. `Unsupported` if this device has no sync line or doesn't support the requested [SyncFormat]
    /// (see `info().sync_formats`), `InvalidArgument` for a format the device recognizes but whose configuration it
    /// rejects, `InUse` if sync mode cannot be switched while already locked to something else.
    #[errors(Common(Unsupported, InvalidArgument, InUse))]
    fn configure_sync(config: SyncConfig) -> Result<(), ClockError<'i>>;

    /// Current sync configuration, as last accepted by `configure_sync` (`Off` until configured).
    fn sync_config() -> SyncConfig;

    /// Current sync state, with the offset and local time of the last lock, when known.
    fn sync_status() -> SyncStatus;

    /// Time of day a sync master sends on the sync line, set from its host. Has no effect on a slave or while
    /// sync is off: `WrongState` unless `configure_sync` currently has this device set as `Master`.
    #[errors(Common(WrongState))]
    fn set_time_of_day(t: DateTime) -> Result<(), ClockError<'i>>;
}

/// Static description of a device's clock, see `Clock::info`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct ClockInfo<'i> {
    /// Smallest step of the clock.
    pub resolution: Second,
    /// "RP2350 TIMER0", "DWT cycle counter".
    pub source: MaxLen<&'i str, 32>,
    /// Sync formats this device can use; empty without a sync line.
    pub sync_formats: MaxLen<RefVec<'i, SyncFormat>, 8>,
}

/// What a device's sync line should do, see `Clock::configure_sync` / `Clock::sync_config`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SyncConfig {
    /// No synchronisation; `now()` runs free.
    Off,
    /// This device drives the sync line in `format`, so others can lock to it.
    Master { format: SyncFormat },
    /// This device locks `now()` to a master's sync line in `format`.
    Slave { format: SyncFormat },
}

/// A format a sync line can carry, see `ClockInfo::sync_formats`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SyncFormat {
    /// IRIG-B time code.
    IrigB,
    /// Pulse-per-second, edge-only: carries phase, not time of day (see `Clock::set_time_of_day`).
    Pps,
}

/// Current sync state of a device's clock, see `Clock::sync_status`.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SyncStatus {
    /// Current state of the sync state machine.
    pub state: SyncState,
    /// Local minus shared time at the last lock, when known.
    pub offset: Option<Second>,
    /// Local time of the last lock.
    pub last_lock: Option<Second>,
}

/// State of a device's sync line, see `SyncStatus::state` and `TimeMapPoint::state`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SyncState {
    /// Sync is off, or no signal has been seen yet: `now()` runs free.
    Free,
    /// A signal is present but not yet locked onto: `now()` still runs free.
    Acquiring,
    /// Locked: `offset` and `last_lock` of `SyncStatus` are current.
    Locked,
    /// Was locked, lost the signal, and is coasting on the last known offset until it reacquires or times out.
    Holdover,
}

/// One point of the `time_map` stream: this device's local time mapped to the shared sync time.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct TimeMapPoint {
    /// This device's own time at the point.
    pub local: Second,
    /// The corresponding shared (master) time.
    pub shared: Second,
    /// Sync state at the point, so a host can tell a real lock from a stale mapping.
    pub state: SyncState,
}

/// Errors of `Clock`: the common ones (unsupported sync line or format, invalid configuration, in use, wrong
/// state); each method lists the variants it returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum ClockError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
}
