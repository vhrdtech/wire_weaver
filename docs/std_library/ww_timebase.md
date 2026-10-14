# ww_timebase

`ww_timebase` is a global WireWeaver trait for a device's clock: reading device time, and synchronising it with
other devices over a sync line (IRIG-B, PPS, ...). It is `no_std` by default and depends only on `wire_weaver`,
`ww_si` (for the [Second](ww_si.md) timestamp type) and `ww_date_time` (for `set_time_of_day`). It
is part of the [IO trait family](io_family.md).

Add it to a device API crate with:

```toml
[dependencies]
ww_timebase = "0.1"
```

## Device time (D6)

Every device has exactly one free-running clock. Every timestamp it reports elsewhere in its API (captures,
stream items) is a `ww_si::Second` since that clock started, in whatever fixed width fits the field, so it wraps
(a `U32` of microseconds wraps every 71.6 minutes). There is no separate `Ticks` type and no unbounded `UVlq64`:
the width is picked per field, and `Clock::now()` always returns the full-width value so a host can unwrap a
truncated timestamp and correlate device time with its own.

## `Clock`

```rust
#[ww_trait]
pub trait Clock {
    fn now() -> Second;

    fn info() -> ClockInfo<'i>;

    stream!(time_map: TimeMapPoint);

    fn configure_sync(config: SyncConfig) -> Result<(), ClockError>;

    fn sync_config() -> SyncConfig;

    fn sync_status() -> SyncStatus;

    fn set_time_of_day(t: DateTime) -> Result<(), ClockError>;
}
```

* `now()` - device time now, full width.
* `info()` - static description of this clock: its resolution, what it calls itself, and which `SyncFormat`s it
  supports.
* `time_map` - local time mapped to the shared sync time, sent about once a second while synced and on every
  `SyncState` change, so a host can line up this device's timeline with others and see the unsynced spans.
* `configure_sync` / `sync_config` - switch sync off, or make this device the master on the sync line, or a
  slave locking to one, in a given `SyncFormat`; `sync_config()` reads back the last accepted configuration.
* `sync_status()` - current sync state, with the offset and local time of the last lock, when known.
* `set_time_of_day` - time of day a sync master sends on the sync line, set from its host; has no effect on a
  slave or while sync is off.

## Types

* `ClockInfo<'i>` - `resolution: Second`, `source: MaxLen<&'i str, 32>` (e.g. `"RP2350 TIMER0"`), `sync_formats:
  MaxLen<RefVec<'i, SyncFormat>, 8>` (empty without a sync line).
* `SyncConfig` - `Off`, `Master { format: SyncFormat }`, `Slave { format: SyncFormat }`; `unib32` discriminant,
  evolvable.
* `SyncFormat` - `IrigB`, `Pps`; `unib32` discriminant, evolvable.
* `SyncStatus` - `state: SyncState`, `offset: Option<Second>` (local minus shared time at the last lock),
  `last_lock: Option<Second>`.
* `SyncState` - `Free`, `Acquiring`, `Locked`, `Holdover`; `unib32` discriminant, evolvable.
* `TimeMapPoint` - one `time_map` item: `local: Second`, `shared: Second`, `state: SyncState`.
* `ClockError` - `Unsupported`, `InvalidArgument`, `WrongMode`, `InUse`; `self_describing`, `unib32`
  discriminant, evolvable (plain error enum for now; error evolution is still open for `ww_stdlib`).

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")`, so owned (`*Owned`)
variants are available by enabling the crate's `std` feature.

## Composing into a device API

A device exposes its clock once at the root (or nested, if grouped with other traits):

```rust
#[ww_api_root]
pub trait DeviceApi {
    ww_impl!(clock: ww_timebase::Clock);
    // ... other traits
}
```

## Server and client usage (`tests/io_timebase`)

There is no dedicated example crate yet (no device in this repo has a sync line); `tests/io_timebase` is the
reference mock server and client, driven through `tests_common::start_device` like a real device over USB. A
sync master, locking immediately, then publishing a `time_map` point:

```rust
client
    .clock()
    .configure_sync(SyncConfig::Master { format: SyncFormat::IrigB })
    .call()
    .await?
    .unwrap();

let mut time_map = client.clock().time_map().await?;
let point = time_map.recv().await?;
assert_eq!(point.state, SyncState::Locked);
```

On the server side, a handler sends a `time_map` update at any point it decides the sync state changed, same as
any other stream item:

```rust
fn clock_configure_sync(
    &mut self,
    cx: &mut Context<'_, impl BlockingEventOut>,
    config: SyncConfig,
) -> RpcResult<Result<(), ClockError>> {
    // ... update state ...
    api_impl::stream_data_ser()
        .clock()
        .time_map_send_blocking(&point, cx)
        .unwrap();
    Ready(Ok(()))
}
```

See `tests/io_timebase/src/lib.rs` for the full mock (a real `Instant`-backed clock, an `IrigB`-only sync line
to exercise `ClockError::Unsupported`, and a slave that locks right away instead of staying `Acquiring`).
