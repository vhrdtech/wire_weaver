//! Device time and synchronisation over WireWeaver.
//!
//! Every device has one free-running clock that timestamps everything it captures, as [Second]s since the clock
//! started (see `docs/std_library/io_family.md` for how this ties into the other IO traits). `Clock` lets a host
//! read it in full (to unwrap fixed-width
//! timestamps and correlate with host time), and synchronise several devices: one is the master on a sync line,
//! the others lock to it and publish `time_map` points that map their local time to the shared one.
//!
//! Shared time is one timescale, UTC (STD-49), and each source of it is an estimate with an error bound. A device
//! follows its best source ([SyncSource]): by default a locked sync line (IRIG-B, PPS: µs) before a host steering
//! it over the API (`steer`: tens of µs to the host, whose UTC is as good as its NTP), before holdover on the last
//! one, before running free; `set_sources` replaces that ranking with an explicit list. Switching from one source to another slews the shared clock instead of stepping it, and shows in
//! `time_map` as a change of `source`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_date_time::DateTime;
pub use ww_si::Second;

/// Device clock: reads device time, and synchronises it with other devices over a sync line (IRIG-B, PPS, ...) or
/// steered by a host over the API (the optional `host_sync` group: `steer`, `shared_now`).
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
    /// [SyncState] or [SyncSource] change, so a host can line up this device's timeline with others and see the
    /// unsynced spans and the seams where the device switched sources.
    stream!(time_map: TimeMapPoint);

    /// Switch the sync line off, or make this device the master on it, or a slave locking to one, in the given
    /// [SyncFormat]. A line has exactly one master, set explicitly; the master's own shared clock follows its host
    /// (`steer`) when it has one. `Unsupported` if this device has no sync line or doesn't support the requested
    /// [SyncFormat] (see `info().sync_formats`), `InvalidArgument` for a format the device recognizes but whose
    /// configuration it rejects, `InUse` if sync mode cannot be switched while already locked to something else.
    ///
    /// Host steering needs no configuration: it is a source of its own, taken when the line gives nothing better.
    #[errors(Common(Unsupported, InvalidArgument, InUse))]
    fn configure_sync(config: SyncConfig) -> Result<(), ClockError<'i>>;

    /// Current sync configuration, as last accepted by `configure_sync` (`Off` until configured).
    fn sync_config() -> SyncConfig;

    /// Current sync state, with the source followed, its estimated error and the offset and local time of the
    /// last lock, when known.
    fn sync_status() -> SyncStatus<'i>;

    /// Time of day a sync master sends on the sync line, set from its host. Has no effect on a slave or while
    /// sync is off: `WrongState` unless `configure_sync` currently has this device set as `Master`.
    #[errors(Common(WrongState))]
    fn set_time_of_day(t: DateTime) -> Result<(), ClockError<'i>>;

    /// A host's fit of this device's clock (STD-49), sent about once a second. The device follows it when it has
    /// nothing better and `Host` is allowed (see `set_sources`): its shared clock becomes
    /// `shared(t) = point.shared_us + (t - point.local) * (1 + point.rate_ppb * 1e-9)` for local time `t` (slewed
    /// there from where it was, not stepped), `sync_status` and `time_map` say source `Host`, `Acquiring` until the
    /// device is within its lock tolerance of the points and `Locked` from then on (see [SyncState]), and
    /// `Holdover` when the points stop coming. A device locked to a better source (a sync line), or one whose source
    /// list leaves `Host` out, keeps what it follows and only notes the point: the host then correlates through
    /// `time_map` instead of steering. Returns the source the device follows after taking the point, `None` when
    /// it follows nothing (`Host` not allowed and no other source locked). `InvalidArgument` for a point it can't
    /// use (`local` a float or in a storage it doesn't read, a rate beyond what it can follow).
    ///
    /// Shared time is UTC from the host's wall clock (NTP-disciplined), in µs since the Unix epoch, so devices
    /// steered by different hosts agree to NTP precision, and devices on one sync line to the line's. A host fits
    /// against its monotonic clock and adds the wall clock's offset when it sends, so a step of its wall clock
    /// moves the points without disturbing the fit (the `ww_clock_sync` crate does all this).
    #[optional(host_sync)]
    #[errors(Common(InvalidArgument))]
    fn steer(point: SteerPoint<'i>) -> Result<Option<SyncSource>, ClockError<'i>>;

    /// The device's shared time now, in µs since the Unix epoch (UTC); `None` while the clock runs free. What a
    /// host compares with its own clock to see how well the device follows (the residual offset).
    #[optional(host_sync)]
    fn shared_now() -> Option<u64>;

    /// The sources the shared clock may follow, best first: `[Line { format: IrigB }, Host]`,
    /// `[Line { format: Pps }, Line { format: IrigB }, Host]`, or `[Line { format: IrigB }]` for "never let a PC
    /// steer me". A source left out is never followed, only noted; holdover and free come after the list. `None`
    /// (the default, also after a reset) is the fixed ranking: any locked sync line, then the host, then holdover,
    /// then free. `Unsupported` for a line format the device doesn't have (see `info().sync_formats`),
    /// `InvalidArgument` for a source named twice. Switching to another source because of a new list slews, as
    /// any switch does.
    #[optional(host_sync)]
    #[errors(Common(Unsupported, InvalidArgument))]
    fn set_sources(
        sources: Option<MaxLen<RefVec<'i, SyncSource>, 8>>,
    ) -> Result<(), ClockError<'i>>;

    /// The source list last accepted by `set_sources`; `None` for the fixed ranking.
    #[optional(host_sync)]
    fn sources() -> Option<MaxLen<RefVec<'i, SyncSource>, 8>>;
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
    /// No sync line; the shared clock follows the host if it steers, or runs free.
    Off,
    /// This device drives the sync line in `format` from its shared clock, so others can lock to it.
    Master { format: SyncFormat },
    /// This device locks its shared clock to a master's sync line in `format`.
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

/// Where a device's shared clock takes its time from, best first: a locked sync line, then a host steering it.
/// See `SyncStatus::source`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SyncSource {
    /// A host steering the device over its API with `Clock::steer`.
    Host,
    /// The sync line, as a slave locked to it.
    Line { format: SyncFormat },
}

/// Current sync state of a device's clock, see `Clock::sync_status`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct SyncStatus<'i> {
    /// Current state of the sync state machine.
    pub state: SyncState,
    /// Local minus shared time at the last lock, when known.
    pub offset: Option<Second>,
    /// Local time of the last lock.
    pub last_lock: Option<Second>,
    /// The source the shared clock follows (`Locked`) or last followed (`Holdover`); `None` while free, and from
    /// devices built before sources were reported.
    #[default = None]
    pub source: Option<SyncSource>,
    /// Estimated bound of the shared clock's error against UTC: the source's own error, the offset still to slew
    /// in and what holdover adds since. `None` when the device can't tell.
    #[default = None]
    pub error: Option<Second>,
    /// Offset still to slew in, µs: where the source puts shared time minus where the shared clock is now
    /// (positive: it runs fast until it caught up). 0 once it has; `None` from devices that don't tell (built
    /// before this field, or not slewing in software). A plain `i32`, as `SteerPoint::shared_us` is a plain
    /// number: a device without signed numbers in `ww_numeric` sends it too.
    #[default = None]
    pub slew_us: Option<i32>,
    /// The host whose steer points the shared clock follows (source `Host`) or last followed (`Holdover`), as its
    /// points named it; `None` for points that name nobody (hosts built before this field), while no host steers,
    /// and from devices that don't tell (SRV-31).
    #[default = None]
    pub steerer: Option<Steerer<'i>>,
    /// Another host that sent steer points while `steerer` was followed: only noted, not followed (the latest one
    /// within the last few seconds). `None` without one, and from devices that don't tell.
    #[default = None]
    pub noted: Option<Steerer<'i>>,
}

/// State of a device's shared clock, see `SyncStatus::state` and `TimeMapPoint::state`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SyncState {
    /// No source yet (sync off and no host steering, or no signal seen): the shared clock runs free.
    Free,
    /// A source is followed but the shared clock isn't within the device's lock tolerance of it yet (still slewing
    /// in an offset, or the source's own error is too large; a sync line seen but not yet decoded). Shared time
    /// runs and moves towards the source, without jumps; LEDs on several boards may be visibly apart.
    Acquiring,
    /// Following `SyncStatus::source` and within the device's lock tolerance of it (the offset still to slew plus
    /// the source's error; bedrock boards use 2 ms): shared time is the source's, LEDs are in phase.
    /// `offset` and `last_lock` of `SyncStatus` are current. A device falls back to `Acquiring` when a new point
    /// finds it well outside the tolerance.
    Locked,
    /// Was locked, lost its source, and is coasting on the last known offset and rate until one comes back or it
    /// times out.
    Holdover,
    /// This device is the master of its sync line and sees another master's signal on it: two masters on one line,
    /// reported until one of them goes. Slaves on that line can't tell which one to follow.
    Conflict,
}

/// One point of the `time_map` stream: this device's local time mapped to the shared sync time.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct TimeMapPoint {
    /// This device's own time at the point.
    pub local: Second,
    /// The corresponding shared time (UTC), in the prefix and storage of `local` (wrapping as it does).
    pub shared: Second,
    /// Sync state at the point, so a host can tell a real lock from a stale mapping.
    pub state: SyncState,
    /// The source followed at the point: a change from one point to the next is a seam (slewed, not stepped).
    #[default = None]
    pub source: Option<SyncSource>,
}

/// A host's fit of a device's clock, see `Clock::steer`: the device's local time `local` is the shared time
/// `shared_us`, and shared time runs `rate_ppb` parts per billion faster than local time from there on (negative:
/// slower, as for a device clock that runs fast).
///
/// `shared_us` is a plain `u64`, not a [Second]: shared time is UTC and needs all 64 bits, and a `Second` in a
/// `U64` would make a device without 64-bit numbers (`ww_numeric`'s `u64` feature, 8 more bytes in every
/// `NumericValue`) enable them for this one field.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct SteerPoint<'i> {
    /// Device time at the point, in the prefix and storage of `Clock::now()` (a `U32` wraps as `now()` does).
    pub local: Second,
    /// Shared time at `local`: µs since the Unix epoch, UTC.
    pub shared_us: u64,
    /// Shared rate minus local rate, in parts per billion: shared time advances `1 + rate_ppb * 1e-9` s per
    /// local second. An HSI clock 1 % fast is about `-9_900_990`.
    pub rate_ppb: i32,
    /// The host's estimate of how far `shared_us` may be off, in µs: half the shortest round trip and the
    /// scatter of its samples. Not counting how far the host's own wall clock is from UTC.
    pub error_us: u32,
    /// Who sends the point (SRV-31). A device follows one steerer at a time: the one it follows keeps it while its
    /// points come, another one's points are only noted (`SyncStatus::noted`) and taken once the followed one has
    /// been quiet for the device's holdover time (bedrock boards: 3 s). Points without one (`None`, hosts built
    /// before this field) count as one steerer of their own.
    #[default = None]
    pub steerer: Option<Steerer<'i>>,
}

/// Who steers a device's clock (SRV-31): a program on a host, see `SteerPoint::steerer` and `SyncStatus::steerer`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct Steerer<'i> {
    /// What the device tells steerers apart by: random per steering program (the instance id of its WireWeaver
    /// client, `ww_link::ClientMeta::instance_id`), never 0.
    pub id: u64,
    /// The host's name.
    pub host: &'i str,
    /// The program's name: `ww hub` for the hub, `ww` for `ww sync`, an app's own name.
    pub program: &'i str,
    /// The program's process id on `host`.
    pub pid: u32,
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
