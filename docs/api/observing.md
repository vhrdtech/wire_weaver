---
level: advanced
---

# Observing properties

Observing (API-38) is how a host gets a device's fast-changing values without polling: it asks once, and the
device sends updates on its own, at the rates the host picked, within a byte budget.

## The concept

The unit is a **trait level** - the root API, or one `ww_impl!` trait - not a single property. A host observes a
whole level with one `Observe` request (`RequestKind::Observe`, [`ww_client_server::observe`](../std_library/ww_client_server.md#observing-properties)),
picking a mode per `+observe` property, and from then on the device sends one `EventKind::PropertyUpdate` per level
whenever anything in it is due, carrying whatever subset of its properties is actually due this time - not every
observed property every time. That update is a [`sparse` struct](../serdes/layouts.md#sparse-a-struct-of-optional-fields)
([`OptionAnyVec`](../serdes/special_types.md#optionanyvec-sparse-slots-of-any-types)): a presence header up front,
then only the present values, so a property that isn't due this time costs roughly one bit, not a byte, let alone
its own message. This is what makes observing cheaper than one `StreamData` event per value - see
[cost vs `StreamData`](#cost-vs-streamdata) below.

```rust
#[ww_trait]
trait Motor {
    property!(ro+observe speed: f32);
    property!(ro+observe current: f32);
    property!(rw+observe mode: u8);
    property!(rw gain: f32); // not observable
}
```

Only properties declared `ro+observe`/`rw+observe` can be observed; see [Properties](properties.md) for declaring
them and the two ways a server can implement a property (`get_set`/`value_on_changed`), which also decide how its
observed value is read.

## Modes

Per property, one of:

| Mode | Sent |
|---|---|
| `Off` | never |
| `OnChange { min_us }` | on a change, at most once every `min_us` |
| `Periodic { every_us }` | every `every_us`, on a fixed grid, changed or not |
| `OnChangeOrBeat { min_us, max_us }` | on a change (at most every `min_us`), and at least every `max_us` even without one |

A property that starts being observed is always sent once right away, so the host has a value to start from.
Periods and `min`/`max` are in microseconds (`UNib32`), evolvable: a mode a device doesn't know (sent by a newer
host) reads as `Unknown(_)` and is refused for that entry rather than guessed at.

## Budget

Each level gets a budget in bytes per second, granted by the device (a host may ask for one, or leave it to the
device); all levels together are capped at `ObserveLimits::total_budget` - properties' share of the link, next to
reserved streams ([stream stores](stream_stores.md)/[reliable streams](stream_reliability.md) get what's left per
turn). What the device grants and clamps:

- a period or `min`/`max` below `ObserveLimits::min_period_us` is raised to it;
- a level's budget is clamped to `ObserveLimits::max_level_budget`, and to what's left of `total_budget`;
- an `Observe` request is validated entry by entry before anything changes: an unknown index, a property without
  `+observe`, a duplicate index or an unknown mode is refused on its own and listed in the answer
  (`ObserveGranted::refused`), the rest of the request still applies. A request that doesn't decode, or lists more
  than 16 refused entries, fails outright (`ArgsDesFailed`/`BadIndex`) and changes nothing; an answer that doesn't
  fit the reply buffer fails with `ResponseSerFailed`, also undone.

## The device side

Three things the user adds, for an API with any `+observe` properties: an `ObserveState` field, handed out by
`observe_state()`, a `poll_properties` call in the event loop, and `mark_changed_<name>()` where the device changes
a value by itself (a `value_on_changed` write calls it automatically). `ww api scaffold` prints the field and
`observe_state()`.

```rust
struct Server {
    observe: api_impl::ObserveState,
    // ..
}

impl Server {
    fn observe_state(&mut self) -> &mut api_impl::ObserveState {
        &mut self.observe
    }

    fn on_new_measurement(&mut self, current: f32) {
        self.current = current;
        self.mark_changed_current(); // sent to hosts observing it on change
    }
}

let mut server = Server {
    observe: api_impl::ObserveState::new(ww_stream::observe::ObserveLimits::DEFAULT),
    // ..
};

// event loop, every millisecond or so, between requests
let mut buf = [0u8; 256]; // as big as the biggest update the link carries
server.poll_properties(&mut ww_server.sink(), medium, now_us, &mut buf).await?;
server.poll_streams(&mut ww_server.sink(), now_ms, &mut stream_buf, 4096).await?; // stored streams after
```

`ObserveState` holds one `ObserveTable<N>` per trait level, sized at compile time (`N` = the level's property
count) - the per-property deadlines, modes and the level's token bucket. `ww_stream::observe::ObserveLimits` is
what the device allows every observer: shortest period, default/max budget per level, the total budget, and
whether to send timestamps.

- **Due**: `Periodic` on its grid, never early; `OnChange` once marked changed and `min` elapsed since its last
  send; `OnChangeOrBeat` also once `max` passed without a send.
- **Values**: `get_set` properties are read through their getter when sent (an unbounded value, e.g. a string, is
  also measured once before that); `value_on_changed` ones from their field directly. A getter that fails skips
  that send.
- **Token bucket, earliest deadline first**: each level has a bucket filled at its granted bytes/s, holding at
  most a tenth of a second of it (64 bytes at least). `poll_properties` sends at most one update per level per
  call: its due properties earliest deadline first, as many as fit the bucket and the buffer. **A level short of
  budget holds**: when the due properties don't all fit yet the bucket isn't full, nothing is sent this call - the
  level waits until they fit or the bucket fills, so it sends fewer, fuller updates (one header each) instead of
  one value per call with a header of its own (SRV-18; see the [motor demo](../examples/motor_demo.md#a-level-short-of-budget)
  for measured numbers). What's left out keeps its deadline, so a property that slipped goes first next time and
  none starves.
- **Missed**: a periodic property counts one missed send per whole period it fell behind; an on-change one counts
  once it was held back or left out for `min` or longer. Held back for less, it's only late - polling often
  doesn't inflate the count. The count travels in the next update's `missed`.
- **Timestamps**: `t` is the device's microsecond clock as a wrapping `u32` (`Second { Micro, U32 }`, 5 bytes on
  the wire), wrapping every ~71.6 minutes; the host unwraps it into a `u64`. Only sent when
  `ObserveLimits::timestamps` is set.
- **Limits**: a level inside a resource array can't be observed yet (`OperationNotSupported`, the bound of its
  indices isn't known yet, [API-29](arrays.md)); nor can a `+observe` property that is itself an array or
  `#[optional]`. Observing stays set while the link is down; `ObserveState::reset()` (e.g. on a fresh connection)
  stops it all.

## The host side

Each trait level with `+observe` properties gets `observe(cfg)` on its generated client (the root client, or
`device.motor()`); each `+observe` property also gets `observe_<name>(mode)`. Both return a `PreparedObserve`,
built further with `.budget(bytes_per_s)`, then `.subscribe()`/`.subscribe_blocking()` to get an `Observer`:

```rust
use wire_weaver_client::observe::{on_change, on_change_or_beat, periodic};
use api_client::motor_api_motor::MotorObserve; // in the level's client module (`<api crate>_<trait>`)

let ms = Duration::from_millis(1);
let mut motor = device
    .observe(MotorObserve::default().speed(periodic(10 * ms)).current(on_change(ms)))
    .budget(20_000)
    .subscribe()
    .await?;
let granted = motor.granted().unwrap(); // Granted { modes: MotorObserve, budget, refused }
while let Ok(u) = motor.recv().await {  // Observed { value: MotorUpdate, t, missed, received }
    if let Some(speed) = u.value.speed { /* .. */ }
}
motor.with_latest(|l| l.speed.as_ref().map(|s| (s.value, s.received)));

let mut mode = device.observe_mode(on_change(ms)).subscribe().await?; // Observer of u8 values
mode.configure(MotorObserve::default().mode(periodic(100 * ms))).await?;
```

- **Types**: `<Trait>Update` (the level's `PropertyUpdate` data, one `Option` per `+observe` property),
  `<Trait>Observe` (one `Option<ObserveMode>` per property, used both to ask and as `Granted::modes`), and
  `<Trait>Latest` (the latest-value cache: a `Sample { value, received, t }` per property).
- **One subscription per level**: every observer of a level asks for its own settings; the host merges them
  (shortest `min`/period, biggest budget per property) into one `Observe` sent to the device, resent when an
  observer comes, calls `configure(..)`, or goes - `Off` once the last one is gone. Every observer of a level sees
  every update of it; a property observer only the updates that carry its property. Settings apply to all
  observers of a level together, so one may get updates more often than it asked for.
- **Reconnects**: observers outlive `disconnect().keep_streams()`; `resume_streams()` resubscribes every level on
  the next connection (the device may or may not still have the old state) and observers see `Resumed`. A link
  that's down for good ends them.
- **Old devices**: a device whose API model predates `ww_client_server` 0.2 can't observe; `.subscribe()` fails
  with `Error::Unsupported`.
- **Limits**: like the device side, a level inside a resource array has no `observe()` yet. Absolute paths only -
  a trait client (`client = "trait_client"`) observes once it has a base path (`attachment()`).

## Wire format

`EventKind::PropertyUpdate { path, t, missed, data }` is one level's update. `data` is the `<Trait>Update`
`sparse` struct: an `OptionAnyVec` presence header (the smallest of four modes - all, dense bits, two-level bits,
or an index list, picked by the writer - see [`OptionAnyVec`](../serdes/special_types.md#optionanyvec-sparse-slots-of-any-types)
for the exact bit layout), then only the present properties' values in slot order, no padding between them. Slots
are the level's resource indices, methods and streams included as gaps that are never present.

Slot order matters for size: a `bool` property takes one bit, but a byte-or-wider type aligns itself to a byte as
always, so a `bool` between two `f32`s still costs a byte of padding around it. [The `sparse` worked
example](../serdes/layouts.md#sparse-a-struct-of-optional-fields) shows this byte-for-byte (`speed: Option<u16>` and
`on: Option<bool>` present: `17 c6 dc 05 4c 00 00 c0 3f 81`) - **group the sub-byte (`bool`) properties of a level
together** in the trait if the savings matter, so their bits share one byte instead of forcing alignment around
them.

The event itself (`ser_property_update_event`) adds a small fixed header on top of the update's own bytes: with a
device timestamp it's 10 bytes (sequence, result flag, the level's path, `t` as a `u32`), 4 bytes without `t` - see
the [motor demo's measured numbers](../examples/motor_demo.md#measured) for what this costs on a real update of
many properties.

## Cost vs `StreamData`

Sending the same values one at a time as `StreamData` events (`stream_data_ser().<name>_send(..)`, see
[Before observe requests](properties.md#before-observe-requests)) costs a separate event header per value instead
of one header per level per update. The [motor demo](../examples/motor_demo.md#measured) measures this on a real
controller with 52 observed properties: about **42% less bytes/s** observing a level than sending the same values
as `StreamData`, growing with how many properties share one update (a single current, a `ww_si::Ampere` of mA as
`I32`, 5 bytes, already takes 9-10 bytes alone as `StreamData`; an update of 21 such values as one `PropertyUpdate`
takes 117 bytes, 5.6 bytes per value). A level
short of its budget that holds for fuller updates (see [above](#the-device-side)) widens that gap further: the
[same page](../examples/motor_demo.md#a-level-short-of-budget) measures 2.7x more values per byte once updates
stop being sent one value at a time.

## Walk-through: the motor demo

[`examples/motor_demo`](../examples/motor_demo.md) observes a simulated FOC motor controller's ~50 properties
across three trait levels (one per rate tier: 25 kHz, 1 kHz, 50 Hz), each tier as one observe subscription at
whatever rate a host asks for, within a budget. It's the reference for everything on this page in a real API:
declaring `+observe` properties by tier, wiring `ObserveState`/`poll_properties`/`mark_changed_<name>` on the
device, observing tiers at chosen rates and a budget from the CLI, and the measured bytes/s numbers above.

## From the CLI and Python

Observing also works from introspection alone, no generated code needed (TOOL-11): `DynResource`'s
`observable_properties()` lists a level's `+observe` properties (resource index, name, type), and
`observe(cmd, entries)`/`observe_blocking(..)` (`entries: BTreeMap<u32, ObserveMode>`) build a
`wire_weaver_client::DynObserver` that decodes each update's sparse `OptionAnyVec` by resource index instead of a
generated struct's fields - reusing the same `Level<U>`/`Observer<U, T>` machinery as the generated client
unchanged: one device subscription per level, `t` unwrapped, reconnect/resume included. Each `DynObserved` carries
`values` (present properties by name, in resource-index order), `t`, `missed`, `received`, and `data_len` (the
update's raw byte length, e.g. for measuring link bytes/s).

[`ww observe <path> [--rate MS]`](../cli_reference.md#ww-observe) uses it directly:

```
$ ww observe motor --rate 10
observing motor (3 properties): speed, current, position
budget: 20.0 kB/s
Ctrl+C to stop
{speed: 1500, current: 3, position: 1024}  rate 98.2/s  link 982/s
```

and [`wire_weaver_py`](../python_advanced.md#observing)'s `Node.observe(cfg)` iterates the same updates as dicts:

```python
m = dev.motor
m.observable_properties()                              # {"speed": "u32", "current": "u32", "position": "u32"}
with m.observe({"speed": 10, "current": None}) as obs:  # speed periodically every 10ms, current on change
    for u in obs:
        print(u["values"], u["t"], u["missed"])
```

Both ask for every observable property on-change without an explicit rate/`cfg`, and share one device subscription
per level with any generated-client observers of the same level.
