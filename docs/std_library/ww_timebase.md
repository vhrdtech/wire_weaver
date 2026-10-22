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

    fn sync_status() -> SyncStatus<'i>;

    fn set_time_of_day(t: DateTime) -> Result<(), ClockError>;

    #[optional(host_sync)]
    fn steer(point: SteerPoint<'i>) -> Result<Option<SyncSource>, ClockError>;

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
* `SyncStatus<'i>` - `state: SyncState`, `offset: Option<Second>` (local minus shared time at the last lock),
  `last_lock: Option<Second>`, `source: Option<SyncSource>`, `error: Option<Second>` (estimated bound against UTC),
  `slew_us: Option<i32>` (offset still to slew in), `steerer: Option<Steerer>` (the host whose points it follows)
  and `noted: Option<Steerer>` (another one it only notes, see [one steerer at a time](#one-steerer-at-a-time));
  all but the first three `#[default = None]`, read as `None` from devices built before them.
* `SyncState` - `Free`, `Acquiring`, `Locked`, `Holdover`, `Conflict` (two masters on the line), see
  [States](#states); `unib32` discriminant, evolvable.
* `TimeMapPoint` - one `time_map` item: `local: Second`, `shared: Second` (UTC, in the prefix and storage of
  `local`), `state: SyncState`, `source: Option<SyncSource>` (`#[default = None]`).
* `SteerPoint<'i>` - `local: Second`, `shared_us: u64` (UTC), `rate_ppb: i32`, `error_us: u32`: device time `local`
  is shared time `shared_us`, and shared time runs `rate_ppb` parts per billion faster than local time from there
  on; `steerer: Option<Steerer>` (`#[default = None]`) says who sends it.
* `Steerer<'i>` - `id: u64` (random per steering program, what the device tells steerers apart by), `host: &str`,
  `program: &str` (`ww hub`, `ww`, an app), `pid: u32`.
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

### States

`sync_status().state` and `TimeMapPoint::state` say how far a device's shared time can be trusted:

| State       | Source followed | Means                                                                                        |
|-------------|-----------------|----------------------------------------------------------------------------------------------|
| `Free`      | none            | never steered (or in holdover too long): shared time, if any, is nobody's                    |
| `Acquiring` | yes             | following the source, but not within the lock tolerance yet: still slewing, or the source's error is too large |
| `Locked`    | yes             | within the lock tolerance of the source: LEDs are in phase, scheduled actions line up          |
| `Holdover`  | the last one    | the points stopped: coasting on the last offset and rate, the error grows with time            |
| `Conflict`  | -               | the master of a line sees another master on it                                               |

The tolerance is the device's: `slew_us` (what is left to slew in) plus the source's own error must be within it to
enter `Locked`, and a new point that finds the clock more than twice that far off (or steps it) goes back to
`Acquiring`. bedrock boards (`bedrock_sync`) use 2 ms (a full-speed USB board's own error bound is about 1 ms), plenty for LEDs, and slew 5 % while acquiring (250 ms gone in
5 s) and 1 % once locked; shared time never jumps or goes back except for a step beyond 1 s. A host steers more often
while a device is acquiring (`clock_sync` 10 Hz, then 1 Hz once locked). `Locked` only says the device agrees
with its source: how close the source is to UTC is `error`, and what the host measures is the residual of
`shared_now()` against its own clock (`ww sync` shows both).

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
* The host side is `wire_weaver_client::clock_sync` (its `clock_sync` feature, on by default; the `ww_clock_sync`
  crate re-exports it), for any process holding a device connection (the `ww` CLI, a device's own CLI or GUI). Every
  direct connection steers its device by itself ([below](#steering-in-every-direct-connection)), and
  `ClockSync::start(&cmd, Options::default())` does it with options of a program's own: it polls, fits and steers in
  a tokio task, and its
  `Status` has the fit, the followed source, the residual offset it measures with `shared_now()` and whether the
  device follows it (`followed`). Every process steers its devices onto the same UTC; on a PC with a
  [ww hub](../transport/hub.md#clock-sync-in-the-hub-srv-31) the hub is the one steerer for the devices it holds.
  `Estimator`, `DeviceTime` and `HostClock` are the parts; the first two take time as numbers, so tests drive them
  with a simulated device clock (an HSI 1 % fast, a crystal drifting with temperature, USB full-speed jitter of up
  to 2 ms a round trip).
* `ww sync` ([CLI](../cli_reference.md#ww-sync)) keeps several devices steered and shows how well they follow; with
  a hub it shows the hub's steering instead.
* On the device, embedded_bedrock's `bedrock_sync` crate keeps the shared clock (with an optional backend that
  steers the oscillator itself where a chip has a fractional PLL).

### Steering in every direct connection

A program that connects to a device directly (not through the [ww hub](../transport/hub.md#clock-sync-in-the-hub-srv-31))
keeps its clock on the PC's UTC without code of its own (CLT-46): when the device serves `Clock` with `host_sync`
(found in its introspection data; a connection without them, `traits_only`, tries `now()` once), the connection runs
a sync in a task of its own, with the [rate cache](#the-rate-cache) of the device's serial and the connection's
`ClientMeta` as its [steerer](#one-steerer-at-a-time).

```rust
let device = MyDevice::connect().await?; // steers, if the device has the clock
if let Some(steering) = device.cmd.clock_sync() {
    println!("{:?}", steering.phase()); // Starting, Steering, Paused, ThroughHub, Unsupported, Failed(..)
    let status = steering.status(); // the same Status as ClockSync's
    steering.set_paused(true); // no steer points meanwhile, the device coasts in holdover
}
```

* **Off** with `ClientConfig::clock_sync(false)`, or `WW_CLOCK_SYNC=off` for every connection that doesn't say;
  `ClientConfig::clock_sync_options` sets the pace. `ww sync` steers with a `ClockSync` of its own and switches it off;
  the hub does for the connections it opens, since it steers them itself.
* **Through the hub** the connection doesn't steer (`Phase::ThroughHub`): the hub does. A reconnect that reaches the
  device directly again starts steering again, and the other way round.
* **Never keeps the connection open.** The task holds the connection weakly, for one round of calls at a time: the
  connection closes with the program's last `Commander`, and the steering stops with it; the device goes on in
  holdover.
* **Paused** while the program changes the device: `ww dfu` pauses its connection's steering when it holds the device
  directly, as the hub pauses under a device-wide claim.

### One steerer at a time

Steer points say who sends them (`SteerPoint::steerer`: a random id per steering program, its host, program name and
pid), so a device reached by two hosts (USB to one PC, UDP to another) or by a hub and a program on another link
doesn't flip between their fits. The device follows one steerer: while that one's points keep coming (bedrock
boards: within 3 s, their holdover time), another one's points are only noted, answered with the source the device
follows (`Host`) and reported in `SyncStatus::noted`; once the followed one has been quiet that long, the next
steerer's point takes over, slewed like any switch of source. Points without a steerer (hosts built before the
field) count as one steerer of their own. `SyncStatus::steerer` names the one followed, so a host sees whether it is
(`clock_sync::Status::followed`) and who is instead; `ww sync` shows it in its STEERED BY column.

### The rate cache

A fit needs about a second of readings before its rate is worth anything, and a young fit's rate is off by the
jitter over its span (100 µs over 1 s is 100 ppm). A device's rate against a PC's clock changes slowly (temperature,
ageing), so `clock_sync::RateCache` keeps the last one each PC measured per device serial, in the user's state
dir (`$XDG_STATE_HOME/ww/clock_rates/<serial>.toml`, `~/Library/Application Support/ww` on macOS,
`%LOCALAPPDATA%\ww\state` on Windows, `WW_STATE_DIR` to move it), written to a temporary file and renamed, shared
by the hub and every program that steers directly. With `Options::rate_cache` set, the fit takes the cached rate and
only fits the offset from the first reading on (`Estimator::set_prior`), so the first steer point goes out a round
trip after connecting; after 20 s (`EstimatorConfig::prior_span`) the readings' own rate takes over, and it is saved
again every minute. Between connections the device keeps its last rate in holdover, so a new connection, or a
restarted hub, finds it close and pulls it back within its lock tolerance quickly.

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
