---
level: advanced
---

# ww_timebase

Part of the [IO trait family](io_family.md). `ww_timebase` is a global WireWeaver trait for a device's clock: reading device time, and synchronising it with
other devices over a sync line (IRIG-B, PPS, ...) or steered by its host onto UTC. It is `no_std` by default and depends only on `wire_weaver`,
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
    fn steer(point: SteerPoint) -> Result<Option<SyncSource>, ClockError>;

    #[optional(host_sync)]
    fn shared_now() -> Option<u64>;

    #[optional(host_sync)]
    fn set_sources(sources: Option<MaxLen<RefVec<SyncSource>, 8>>) -> Result<(), ClockError>;

    #[optional(host_sync)]
    fn sources() -> Option<MaxLen<RefVec<SyncSource>, 8>>;
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
* `steer(point)` - a host's fit of this device's clock, followed when the device has no better source and its
  source list allows the host; returns the source it follows (`None`: nothing). See [host sync](#host-sync-std-49);
  optional (group `host_sync`, left out with `omit = ["clock.host_sync"]`).
* `shared_now()` - the device's shared time now in UTC µs, `None` while it runs free; optional (group `host_sync`).
* `set_sources(list)` / `sources()` - an explicit ordered list of the sources the device may follow, `None` for the
  fixed ranking, see [choosing the sources](#choosing-the-sources); optional (group `host_sync`).

## Types

* `ClockInfo<'i>` - `resolution: Second`, `source: MaxLen<&'i str, 32>` (e.g. `"RP2350 TIMER0"`), `sync_formats:
  MaxLen<RefVec<'i, SyncFormat>, 8>` (empty without a sync line).
* `SyncConfig` - `Off`, `Master { format: SyncFormat }`, `Slave { format: SyncFormat }`; `unib32` discriminant,
  evolvable.
* `SyncFormat` - `IrigB`, `Pps`; `unib32` discriminant, evolvable.
* `SyncSource` - `Host`, `Line { format }`: what the shared clock follows; `unib32` discriminant, evolvable.
* `SyncStatus` - `state: SyncState`, `offset: Option<Second>` (local minus shared time at the last lock),
  `last_lock: Option<Second>`, `source: Option<SyncSource>`, `error: Option<Second>` (estimated bound against UTC);
  the last two `#[default = None]`, read as `None` from devices built before them.
* `SyncState` - `Free`, `Acquiring`, `Locked`, `Holdover`, `Conflict` (two masters on the line); `unib32`
  discriminant, evolvable.
* `TimeMapPoint` - one `time_map` item: `local: Second`, `shared: Second` (UTC, in the prefix and storage of
  `local`), `state: SyncState`, `source: Option<SyncSource>` (`#[default = None]`).
* `SteerPoint` - `local: Second`, `shared_us: u64` (UTC), `rate_ppb: i32`, `error_us: u32`: device time `local` is
  shared time `shared_us`, and shared time runs `rate_ppb` parts per billion faster than local time from there on.
* `ClockError<'i>` - `Common(CommonError)` ([common errors](../api/errors.md#common-errors)); unsized (`unib32`
  discriminant), evolvable.

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")`, so owned (`*Owned`)
variants are available by enabling the crate's `std` feature.

## Host sync (STD-49)

### One timescale, ranked sources

Shared time is UTC, whatever it comes from, and every source of it is an estimate with an error bound. A device
follows its best source and only measures the others:

1. a **sync line** it is locked to as a slave (IRIG-B, PPS): µs between the boards on the line;
2. its **host**, steering it over the API (`steer`): tens of µs to the host over USB, but the host's UTC is only as
   good as its NTP (ms);
3. **holdover** on the last source, with its last offset and rate;
4. **free**: no shared time yet.

A line has exactly one master, set explicitly with `configure_sync`; the master's own shared clock follows its host
(or a GPS later), and it drives the line from it. Slaves follow the line, and fall back to their host when the line
is lost. A master that sees another master's signal on its line reports `SyncState::Conflict`. Switching from one
source to another slews the shared clock instead of stepping it, and the switch shows in `time_map` as a change of
`TimeMapPoint::source`, so a host sees the seam. So boards on one line agree to µs whatever PCs they hang on, and
boards only on different PCs agree to those PCs' NTP (ms, plenty for LEDs, see
[`ww_indication`](ww_indication.md#animations-in-phase-std-50)).

`configure_sync` stays the line's role (`Off`, `Master`, `Slave` in `IrigB` or `Pps`); host steering needs no
configuration. `sync_status()` says which source the device follows (`source`) and its estimated error (`error`).

### Choosing the sources

The ranking above is the default. `set_sources` replaces it with an explicit list, best first, of the sources the
device may follow:

| List                                         | Meaning                                                         |
|----------------------------------------------|-----------------------------------------------------------------|
| `None` (default, also after a reset)         | any locked line, then the host, then holdover, then free        |
| `[Line { format: IrigB }, Host]`             | the IRIG-B line when locked, else the host                      |
| `[Line { format: Pps }, Line { format: IrigB }, Host]` | PPS first, IRIG-B next, the host last                 |
| `[Line { format: IrigB }]`                   | never let a PC steer me: only the line, else holdover and free  |
| `[Host, Line { format: Pps }]`               | the host even while the line is locked, the line as a fallback  |

A source the list leaves out is never followed: a `steer` from a host that isn't allowed answers with the source the
device follows instead, or `None`, and the device only notes the point, as a line-locked one does. Holdover and free
always come after the list. A new list that leaves out the source being followed puts the clock into holdover on it
until an allowed source steers it, slewed as any switch. `sources()` reads the list back; `sync_status()` reports
the effective source and its error as before.

### Two PCs, two boards, one line

Two PCs, each with one board, the boards joined by an IRIG-B line, and every clock (both PCs' NTP, both boards'
oscillators) equally bad. Exactly one board is configured `Master { format: IrigB }`: its shared clock follows its
own PC (`steer`), and it drives the line from it. The other is `Slave { format: IrigB }` and ignores its PC's steering
while the line is locked (the fixed ranking does this; `set_sources([Line { format: IrigB }])` makes it explicit,
also when the line drops). Both boards then agree to µs, on the master PC's timescale, whatever the two PCs think. The
second PC keeps reading its board's `now()` and `time_map`, which gives it its own offset from the master PC, and so
from the boards (a host never steers a device that follows something better, it only correlates).

The master is chosen explicitly, with `configure_sync`. IRIG-B and PPS are one-way: a slave can't tell its master
anything, so boards can't hold an election over the line. Two masters on one line are detected and reported
(`SyncState::Conflict`), not resolved. An automatic choice, PTP-BMCA style (the best source wins, the lowest serial
breaks a tie), needs a two-way link between the boards (CAN, Ethernet, USB through one host); it is an idea for later
(STD-51), not built.

### Steering from the host

It works like Klipper's clock sync. The host reads the device's `now()` a few times a second and timestamps every
request and answer with its own monotonic clock. It keeps the readings with the shortest round trips (their midpoint
is closest to when the device read its clock) and fits a line through them over a sliding window: an offset and a
rate. About once a second it sends the fit as a `SteerPoint` (the device's `local` time, the UTC `shared_us` of it in
µs, `rate_ppb`, and its own error estimate `error_us`), adding the wall clock's offset from the monotonic clock as it
sends, so a step of the wall clock moves the points without disturbing the fit. The device runs
`shared(t) = shared_us + (t - local) * (1 + rate_ppb * 1e-9)` on its own and answers with the source it follows:
`Host` when it took the point, a `Line` when it is locked to something better and only noted it (then the host
correlates the device through `time_map` instead of steering it).

* `SteerPoint::local` is in the prefix and storage of the device's `now()`: a device without 64-bit numbers (a
  Cortex-M0+ answering `U32` µs) gets 32 bits, wrapping as its `now()` does, and the host unwraps its readings
  ([`ww_si::Unwrapper`](ww_si.md)). `shared_us` is a plain `u64`, since UTC needs all 64 bits and a `Second` in a
  `U64` would make such a device enable 64-bit numbers for one field.
* The host side is the `ww_clock_sync` crate, for any process holding a device connection (the `ww` CLI, a device's
  own CLI or GUI): `ClockSync::start(&cmd, Options::default())` polls, fits and steers in a tokio task, and its
  `Status` has the fit, the followed source and the residual offset it measures with `shared_now()`. There is no
  central daemon: every process steers its devices onto the same UTC. `Estimator`, `DeviceTime` and `HostClock`
  are the parts; the first two take time as numbers, so tests drive them with a simulated device clock (an HSI 1 %
  fast, a crystal drifting with temperature, USB full-speed jitter of up to 2 ms a round trip).
* `ww sync` ([CLI](../cli_reference.md#ww-sync)) keeps several devices steered and shows how well they follow.
* On the device, embedded_bedrock's `bedrock_sync` crate keeps the shared clock (with an optional backend that
  steers the oscillator itself where a chip has a fractional PLL).

What to expect: in the simulation of a full-speed link with up to 2 ms of jitter per round trip, the offset is good
to a few hundred µs and the rate to tens of ppm after 10 s, a few ppm after half a minute. Real USB bulk transfers
are served anywhere in a frame with spare bandwidth, not only at frame starts, so real links do better; `ww sync`
shows the measured residual.

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
