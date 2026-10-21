---
level: advanced
---

# ww_timebase

Part of the [IO trait family](io_family.md). `ww_timebase` is a global WireWeaver trait for a device's clock: reading device time, and synchronising it with
other devices over a sync line (IRIG-B, PPS, ...). It is `no_std` by default and depends only on `wire_weaver`,
`ww_si` (for the [Second](ww_si.md) timestamp type) and `ww_date_time` (for `set_time_of_day`).

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

    #[optional(host_sync)]
    fn steer(point: SteerPoint) -> Result<(), ClockError>;

    #[optional(host_sync)]
    fn shared_now() -> Option<Second>;
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
* `steer(point)` - a host's fit of this device's clock, see [host sync](#host-sync-std-49); optional (group
  `host_sync`).
* `shared_now()` - the device's shared time now, `None` while it runs free; optional (group `host_sync`).

## Types

* `ClockInfo<'i>` - `resolution: Second`, `source: MaxLen<&'i str, 32>` (e.g. `"RP2350 TIMER0"`), `sync_formats:
  MaxLen<RefVec<'i, SyncFormat>, 8>` (empty without a sync line).
* `SyncConfig` - `Off`, `Master { format: SyncFormat }`, `Slave { format: SyncFormat }`; `unib32` discriminant,
  evolvable.
* `SyncFormat` - `IrigB`, `Pps`, `Host` (no sync line, steered over the API); `unib32` discriminant, evolvable.
* `SyncStatus` - `state: SyncState`, `offset: Option<Second>` (local minus shared time at the last lock),
  `last_lock: Option<Second>`.
* `SyncState` - `Free`, `Acquiring`, `Locked`, `Holdover`; `unib32` discriminant, evolvable.
* `TimeMapPoint` - one `time_map` item: `local: Second`, `shared: Second`, `state: SyncState`.
* `SteerPoint` - `local: Second`, `shared: Second`, `rate_ppb: i32`: device time `local` is shared time `shared`,
  and shared time runs `rate_ppb` parts per billion faster than local time from there on.
* `ClockError<'i>` - `Common(CommonError)` ([common errors](../api/errors.md#common-errors)); unsized (`unib32`
  discriminant), evolvable.

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")`, so owned (`*Owned`)
variants are available by enabling the crate's `std` feature.

## Host sync (STD-49)

Devices without a sync line can still share one time base: the host's. It works like Klipper's clock sync. The host
reads each device's `now()` a few times a second and timestamps every request and answer with its own monotonic
clock. It keeps the readings with the shortest round trips (their midpoint is closest to when the device read its
clock) and fits a line through them over a sliding window: an offset and a rate. Then it pushes that fit to the
device as a `SteerPoint` about once a second:

```rust
client.clock().configure_sync(SyncConfig::Slave { format: SyncFormat::Host }).call().await?;
// ... a few seconds of now() samples into ww_timebase::host::Estimator ...
let point = device_time.steer_point(&fit, fit.local)?;
client.clock().steer(point).call().await?;
```

From then on the device runs the shared clock on its own,
`shared(t) = point.shared + (t - point.local) * (1 + point.rate_ppb * 1e-9)`, reports `Locked` in `sync_status` and
`time_map`, and `Holdover` when the points stop coming (it keeps the last line). Timestamps it gets in shared time,
like an indication epoch ([`ww_indication`](ww_indication.md)) or a "do X at T", mean the same moment on every
device steered by the same host.

* The values of a `SteerPoint` are in the prefix and storage of the device's `now()`. A device without 64-bit
  numbers (a Cortex-M0+ answering `U32` µs) gets 32-bit ones, wrapping as its `now()` does, and the host unwraps
  its `now()` readings ([`ww_si::Unwrapper`](ww_si.md)).
* The host side is `ww_timebase::host` (feature `std`): `Estimator` keeps samples in a 30 s window and fits the best
  half by round trip, `DeviceTime` converts device timestamps to nanoseconds and back. It never reads a clock or
  sleeps, so tests drive it with a simulated device clock (1 % fast HSI, a crystal drifting with temperature,
  USB full-speed jitter of up to 2 ms a round trip).
* `ww sync DEV...` ([CLI](../cli_reference.md#ww-sync)) keeps several devices steered and can set their indication
  mode with one epoch.
* What to expect over USB full speed (answers wait for the next 1 ms frame): the offset to a few hundred µs, the rate
  to tens of ppm after 10 s and a few ppm after half a minute. High speed is about ten times better. On the device,
  embedded_bedrock's `bedrock_sync` crate keeps the shared clock.

Host sync doesn't replace the sync line: `IrigB` and `Pps` stay what they are, and a device lists `Host` in
`info().sync_formats` only when it serves `steer`. `Host` is a slave format only: `Master { format: Host }` is
`InvalidArgument`.

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
    .await?; // one `?`: a `ClockError` arrives as `CallError::Device`

let mut time_map = client.clock().time_map().call().await?;
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
