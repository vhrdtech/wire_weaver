## Unreleased

### ⚠️ Breaking

- `SteerPoint<'i>` and `SyncStatus<'i>` carry a lifetime and are no longer `Copy` (SRV-31): they hold a
  `Steerer<'i>` now. Hosts use `SteerPointOwned` / `SyncStatusOwned` (the generated client takes and returns those),
  devices return `SyncStatus { steerer: None, noted: None, .. }` until they track steerers. Wire-compatible: the new
  fields are appended and defaulted.

- `ClockError<'i>`: its only variant so far is the `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`,
  `InvalidArgument`, `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`.
  It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime (`ClockErrorOwned` on
  hosts), has `Display` from its doc lines, and every method lists its variants with `#[errors(..)]`. Devices return
  `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`).
### 🚀 Features

- Steer points say who steers (SRV-31): `SteerPoint::steerer: Option<Steerer>` (`#[default = None]`), with
  `Steerer { id, host, program, pid }` (id: the steering program's client instance id). A device follows one steerer
  at a time and only notes another's points until the followed one has been quiet for its holdover time;
  `SyncStatus::steerer` says whom it follows, `SyncStatus::noted` the latest other one (both `#[default = None]`).

- `SyncStatus::slew_us: Option<i32>` (`#[default = None]`, wire-compatible): the offset a device still slews in, µs.
  `SyncState::Acquiring` now also means "following a source, not within the device's lock tolerance yet" and
  `Locked` "within it" (bedrock boards: 2 ms of slew left plus the source's error); the states are documented in
  the trait and in `docs/std_library/ww_timebase.md#states` (STD-49).
- Host sync and one UTC timescale (STD-49): shared time is UTC, and a device follows its best source, a locked
  sync line before a host steering it, before holdover, before free. New optional group `host_sync` of `Clock`:
  `steer(point: SteerPoint) -> Result<Option<SyncSource>, ClockError>` (a host's fit: `local` in the prefix and
  storage of `now()`, `shared_us: u64` UTC, `rate_ppb`, `error_us`; a device locked to a better source, or whose
  source list leaves `Host` out, only notes it and answers what it follows, `None` for nothing;
  `Common(InvalidArgument)` for a point it can't use), `shared_now() -> Option<u64>` (UTC µs), and
  `set_sources(Option<MaxLen<RefVec<SyncSource>, 8>>)` / `sources()`: an explicit ordered list of the sources the
  device may follow (`[Line { format: IrigB }]`: never steered by a PC), `None` keeps the fixed ranking
  (`Common(Unsupported)` for a line format the device doesn't have, `Common(InvalidArgument)` for a source twice). New
  `SyncSource` (`Host`, `Line { format }`), `SyncStatus::source` and `SyncStatus::error`, `TimeMapPoint::source`
  (all `#[default = None]`, wire-compatible), `SyncState::Conflict` (two masters on one line). `configure_sync`
  and `SyncFormat` are unchanged. Servers that don't steer leave the group out with `omit = ["clock.host_sync"]`
  (or the path of their `Clock` item); servers built before this need that line or the four handlers (`ww api
  scaffold` prints them), and new fields in their `SyncStatus` / `TimeMapPoint` literals (`source: None,
  error: None`). The host side is the new `ww_clock_sync` crate.
- New crate (STD-17): `Clock` with `now`, `info`, sync master / slave (IRIG-B, PPS), `sync_status`, a `time_map` stream mapping device time to shared time, `set_time_of_day` for a master. Timestamps are `ww_si::Second`, no own time type. Full rustdoc with `# Errors` on `configure_sync` / `set_time_of_day`, docs page (`docs/std_library/ww_timebase.md`), a mock server and in-process tests (`tests/io_timebase`).
