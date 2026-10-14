//! Device time and synchronisation over WireWeaver.
//!
//! Every device has one free-running clock that timestamps everything it captures, as [Second]s since the clock
//! started (see `docs/std_library/io_family.md`). `Clock` lets a host read it in full (to unwrap fixed-width
//! timestamps and correlate with host time), and synchronise several devices: one is the master on a sync line,
//! the others lock to it and publish `time_map` points that map their local time to the shared one.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_date_time::DateTime;
pub use ww_si::Second;

#[ww_trait]
pub trait Clock {
    /// Device time now, full width.
    fn now() -> Second;

    fn info() -> ClockInfo<'i>;

    /// While synced, and on every sync state change: local time and the shared time it corresponds to.
    stream!(time_map: TimeMapPoint);

    fn configure_sync(config: SyncConfig) -> Result<(), ClockError>;

    fn sync_config() -> SyncConfig;

    fn sync_status() -> SyncStatus;

    /// Time of day a master sends on the sync line, set from its host.
    fn set_time_of_day(t: DateTime) -> Result<(), ClockError>;
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct ClockInfo<'i> {
    /// Smallest step of the clock.
    pub resolution: Second,
    /// "RP2350 TIMER0", "DWT cycle counter".
    pub source: MaxLen<&'i str, 32>,
    /// Sync formats this device can use; empty without a sync line.
    pub sync_formats: MaxLen<RefVec<'i, SyncFormat>, 8>,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SyncConfig {
    Off,
    Master { format: SyncFormat },
    Slave { format: SyncFormat },
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SyncFormat {
    IrigB,
    Pps,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SyncStatus {
    pub state: SyncState,
    /// Local minus shared time at the last lock, when known.
    pub offset: Option<Second>,
    /// Local time of the last lock.
    pub last_lock: Option<Second>,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SyncState {
    Free,
    Acquiring,
    Locked,
    Holdover,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct TimeMapPoint {
    pub local: Second,
    pub shared: Second,
    pub state: SyncState,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = unib32
)]
pub enum ClockError {
    Unsupported,
    InvalidArgument,
    WrongMode,
    InUse,
}
