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
| `Samples { every_us, max_age_us }` | every sample the device takes (or the first in each `every_us`), in batches at most `max_age_us` old; `+batched` properties only, see [Every sample](#every-sample-batched) |

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

`ObserveState` holds one `ObserveTable<[Slot; N]>` per trait level, sized at compile time (`N` = the level's
property count) - the per-property deadlines, modes and the level's token bucket. The table's code is the same for
every level (it works on `ObserveTable<[Slot]>`, SRV-23), and so is the loop of `poll_properties` (SRV-24): plan,
choice, writing the update and its bookkeeping are `ww_stream::observe`'s (`plan_level`, `select`, `write`,
`ser_update`), one loop over the levels sends from one place, and what the generated code adds per property is
reading its value (`size` and `write` arms, and the `.await` of an async getter). A firmware carries the algorithm
once however many levels its API has. `ww_stream::observe::ObserveLimits` is
what the device allows every observer: shortest period, default/max budget per level, the total budget, and
whether to send timestamps.

- **Due**: `Periodic` on its grid, never early; `OnChange` once marked changed and `min` elapsed since its last
  send; `OnChangeOrBeat` also once `max` passed without a send.
- **Values**: `get_set` properties are read through their getter when sent; `value_on_changed` ones from their
  field directly. An unbounded value (e.g. a string) is measured before it is chosen: a sync getter is called
  for that and again to write it, an async one (`handler_model`, SRV-21) once, its value kept for both. A getter
  that fails skips that send.
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
  the wire), wrapping every ~71.6 minutes; the host unwraps it into a `U64` per level, by the same rule as stream
  times ([timestamps](stream_reliability.md#timestamps)). Only sent when
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

## Every sample (`+batched`)

The modes above send a property's *current* value, at most once per update. A value that changes faster than
updates can go - a motor current sampled in a 25 kHz control loop - loses everything in between. A `+batched`
property (API-39) can also be observed in `Samples` mode: the device keeps every sample it takes and sends them as
compressed [`Series`](../serdes/series.md) columns in the level's update: a `Series<T>` in each property's slot, and
one `Series<Second>` of the sample times for the whole update (`sample_times`, next to its `t`), since a level's
samples are taken together.

```rust
#[ww_trait]
trait Foc {
    property!(ro+observe+batched i_a: Ampere);
    property!(ro+observe+batched i_b: Ampere);
    property!(ro+observe duty: f32); // observed as before
}
```

`+batched` comes after `+observe` (alone it is an error). The property's slot in `<Trait>Update` becomes a
`Batched<T>`: `One(T)`, one value as in the other modes, or `Samples(..)`, a batch of samples. A host that doesn't
ask for `Samples` gets `One` values from the getter, as for any `+observe` property.

### Device side

Next to the getter, the device feeds samples where it takes them. Codegen generates, per level with `+batched`
properties, a `<Level>Sampler` with `sample(t, <Trait>Samples { .. })`: one value per `+batched` property of the
level and their common time. It is O(1) and takes no lock: a gate per level (one atomic load) says whether a host
wants this sample (any property of the level in `Samples` mode, and the sample on the level's grid), and the row,
the time and every value, goes into the level's batch being filled. Encoding happens later, in `poll_properties`,
one column at a time: the time once, then each property a host observes in `Samples` mode.

```rust
let mut server = Server {
    observe: api_impl::ObserveState::new(
        ObserveLimits::DEFAULT,
        api_impl::ObserveBatches::new(ww_stream::batch::DEFAULT_POLICY),
    ),
    // ..
};

// in the control loop, every tick
server.observe.samplers().root.sample(
    Second::micro(now_us),
    api_impl::FocSamples { i_a, i_b },
);

// event loop: encodes sealed batches, then sends updates while batches wait; the scratch holds one column of a
// batch: the biggest of `Second` and the `+batched` property types, `OBSERVE_BATCH` of them
const SB: usize = ww_stream::batch::scratch_bytes::<Second>(api_impl::OBSERVE_BATCH);
let mut scratch = Box::new(ww_stream::batch::Scratch::<SB>::new());
server.poll_properties(&mut ww_server.sink(), medium, now_us, &mut buf, &mut scratch).await?;
```

To sample from an interrupt or another thread, `ObserveBatches::split()` hands out the samplers (`Send`, moved into
the interrupt) and the consumers, which the `ObserveState` takes instead (`ObserveState::new(limits, consumers)`,
the batches in a `static`). `ww api scaffold` writes the `observe_batch` argument and says how to build the state.

On a firmware, build the batches and the state where they stay: `ObserveBatches::init` and `ObserveState::init`
write them into `static`s in place, every store, table and outbox where it lives, with a few dozen bytes of stack
(8 B at `opt-level = 3`, 72 B at `z` for the motor demo's 21 FOC values on a Cortex-M4F). `new` returns them by
value, and building them that way takes a stack frame of a few times their size (31.6 KB for the same level, more
than a small MCU has). The server then keeps a `&'static mut` to the state:

```rust
type State = api_impl::ObserveState<api_impl::ObserveBatchConsumers<'static>>;
static BATCHES: StaticCell<api_impl::ObserveBatches> = StaticCell::new();
static STATE: StaticCell<State> = StaticCell::new();

let batches = api_impl::ObserveBatches::init(BATCHES.uninit(), ww_stream::batch::DEFAULT_POLICY);
let (samplers, consumers) = batches.split(); // samplers.root to the interrupt
let observe: &'static mut State = State::init(STATE.uninit(), ObserveLimits::DEFAULT, consumers);
```

- **Batches**: each level with `+batched` properties has two stores of `observe_batch = N` rows (codegen argument,
  default 32), a row being one `sample()` call's time and values, kept column by column: the sampler fills one
  store while the event loop encodes the other. RAM is 2 x N x (the time and every value): for 21 FOC currents and
  voltages at N = 32 about 8.4 KB. A batch is sealed when it is full, when its worst case reaches what fits one
  update, or when it is as old as `max_age_us`. A burst of more than two batches between two `poll_properties`
  calls loses rows: a sample of each observed property per row, counted in the next update's `missed`.
- **Outbox**: a sealed batch is encoded into the level's outbox (`observe_outbox = N` bytes, default 4096) as one
  unit, the time column and the column of each property in `Samples` mode, and sent from there whole, in one
  update, as budget allows, more than one update per call (at most `MAX_UPDATES_PER_POLL`). When the outbox is full
  or the budget can't keep up, the oldest batches go and count as `missed`.
- **Time**: the samples of an update share its `sample_times`: every `Samples` slot has as many samples as it has
  times, sample `i` taken at time `i`. An update that carries only batches has no head `t`, one with `One` values or
  plain properties has it as before. Rows carry every value of the level, so a property switched into `Samples`
  also gets the rows not sent yet, each at its time; one switched out leaves the waiting batches.
- **Limits**: `Samples` is refused for a property without `+batched` (`ModeNotSupported`); `max_age_us` is
  clamped to `ObserveLimits::samples_max_age_us` (1 ms to 10 s by default); `every_us` decimates on the device's
  grid (the first sample in each `every_us`), 0 is every sample, and is raised to
  `ObserveLimits::samples_min_every_us` (0, every sample, by default). The `Samples` entries of a level share one
  grid and one age, so they get the finest `every_us` and the shortest `max_age_us` any of them asked for: the
  grant says so, a host that asked for less gets more. Every sample taken is encoded, also when
  the budget then drops its batch, so on an MCU set that floor from the table [below](#which-rates-fit-an-mcu):
  without it any host can ask for more encoding than the CPU has. A property type that borrows (bytes, a string,
  a vector) can't be `+batched`.

### Cells: sampling from an interrupt

`sample(t, FocSamples { .. })` takes the row as one value: the interrupt builds the whole `<Trait>Samples` on its
stack and the sampler copies it into the batch. For a level sampled in an interrupt, `ww_codegen!(..,
observe_cells = "root_foc")` (the level's field name in `ObserveState`, several separated by commas) puts it on
*observe cells* instead: a ring of atomics the interrupt writes in place, each value where it is computed, and
`poll_properties` reads. The API and the wire don't change, so this is the device's choice per level, and a host
can't tell which one a device uses.

```rust
// ww_codegen!(motor_demo_api::MotorDemo for Device, server = true, .., observe_cells = "root_foc")
static FOC: api_impl::RootFocCells<32> = api_impl::RootFocCells::new(); // const fn, a ring of 32 rows
static BATCHES: StaticCell<api_impl::ObserveBatches<'static>> = StaticCell::new();
// the state borrows the batches in their static
type State = api_impl::ObserveState<&'static mut api_impl::ObserveBatches<'static>>;
static STATE: StaticCell<State> = StaticCell::new();

let batches = api_impl::ObserveBatches::init(
    BATCHES.uninit(),
    ww_stream::batch::DEFAULT_POLICY,
    FOC.reader().unwrap(),
);
let mut server = Server {
    observe: State::init(STATE.uninit(), ObserveLimits::DEFAULT, batches), // a `&'static mut State`
    // ..
};

// the interrupt keeps the writer (taken once)
let mut w = FOC.writer().unwrap();
// .. every PWM period, where the values are computed
w.i_a(Ampere::milli(ia));
w.i_b(Ampere::milli(ib));
// .. one setter per `+batched` property of the level
w.commit(Second::micro(now_us));
```

- **Writer**: `<Level>Cells::writer()` hands out the one `<Level>Writer` (`Send`, not `Sync`, given back on drop).
  Each setter is a few relaxed stores into the row being written; `commit(t)` publishes the row when a host
  observes the level in `Samples` mode and `t` is on its grid (the gate `sample()` checks), between two release
  fences: no lock, no read-modify-write, nothing on the stack but the values. A debug build panics at `commit` when
  a setter of the row wasn't called; a release build pays nothing for that. With nobody observing, the setters
  still store: put them (or the expensive values) under `if w.is_sampling() { .. }`.
- **Reader**: `<Level>Cells::reader()` goes to `ObserveBatches::init` or `new` (one per cells level, in the order
  of the levels), which then borrow the cells: `ObserveBatches<'static>` for a `static`, or
  `ww_stream::batch::CellsReader::shared(arc)` for cells in an `Arc` on `std`. Each `poll_properties` copies the
  rows committed since the last one into one staging batch, the time and the observed properties' columns only,
  then encodes and sends it as the sampler's batches are: the same columns, the same bytes. A property observed
  while rows are staged gets its column copied for them from the ring, so it has the batch's earlier samples as
  with `sample()`; if the interrupt wrote over those rows meanwhile, they go without it and the next batch has it.
- **Ring size**: `R` rows (`<Level>Cells<R>`) hold what the interrupt commits between two `poll_properties` calls;
  `R - 1` can wait. A row the writer writes over before it is read (the ring lapped) is dropped and counted in
  `missed`, never sent with values of two rows. RAM is `R` rows plus one batch of `observe_batch` rows, about what
  the sampler's two batches take (for the motor demo's 21 FOC values, `R` = `observe_batch` = 32: 8.6 KB).
- **Under backpressure**: while a sealed batch waits for budget or room, the sampler fills its second batch and
  loses the rows past it; the cells keep up to `R - 1` rows in the ring and send them later, so they lose fewer.
  When nothing waits, both send the same bytes.
- **Types**: the property types need `SeriesCells` (`#[derive_shrink_wrap(.., series(cells))]`, see
  [`Series`](../serdes/series.md)); `ww_si` quantities, numbers and `bool` have it.
- **Cost**: on an STM32H725 a row of the 21 FOC values takes 354 cycles at `opt-level = "z"` against 412 through
  `sample()`, 328 against 419 at `opt-level = 3`, and 44-52 B of stack against 108-116 B; the copy into the staging
  batch moves to `poll_properties`. See
  [Batching: observe cells, measured](batching.md#observe-cells-measured).

`ww api scaffold --observe-cells root_foc` writes the `static`, the `ObserveState` type and the `ww_codegen!`
argument.

### Host side

`<Trait>Observe::samples(max_age)` puts every `+batched` property of the level into `Samples` mode
(`observe::samples(max_age)` / `samples_every(every, max_age)` for one property). A property observer gets one
`Observed` per sample, each with its own device time, whether the device sent one value or a batch; a level
observer gets `BatchedOwned<T>` slots and `Observed::sample_times`, unwrapped like `t` (`BatchedOwned::into_samples`
pairs them, a count that doesn't match is an error), and `<Trait>Latest` keeps the newest sample. An update whose
`Samples` slots don't match its `sample_times` is malformed: a warning, its samples left out. Observers of one level
merge into `Samples` if any of them asks for it.

```rust
use wire_weaver_client::observe::samples;
let mut i_a = device.foc().observe_i_a(samples(Duration::from_millis(5))).subscribe().await?;
while let Ok(s) = i_a.recv().await {
    println!("{:?} {:?}", s.t, s.value); // every sample, oldest first
}
```

The dynamic client decodes the same slots from introspection (`DynObserved::samples`), `ww observe --samples MS`
prints one line per sample, and Python gets them per update or as numpy columns
([`recv_batch()`](../python_advanced.md#observing)).

### Which rates fit an MCU

`sample()` itself is a few loads and stores; the cost is encoding, and it scales with how many properties and how
fast they're sampled - see [Batching: which rates fit an MCU](batching.md#which-rates-fit-an-mcu) for the measured
instructions/cycles-per-sample table (Cortex-M3/M4F/M7) and what it means for a real device. So on an MCU observe
a few properties at a few kHz, or decimate (`every_us`); full-rate observing of a whole control loop is for the
simulator and the host ([motor demo](../examples/motor_demo.md)), where it costs about 1 MB/s of link with integer
storage.

## Wire format

`EventKind::PropertyUpdate { path, t, missed, data }` is one level's update. `data` is the `<Trait>Update`
`sparse` struct: an `OptionAnyVec` presence header (the smallest of four modes - all, dense bits, two-level bits,
or an index list, picked by the writer - see [`OptionAnyVec`](../serdes/special_types.md#optionanyvec-sparse-slots-of-any-types)
for the exact bit layout), then only the present properties' values in slot order, no padding between them. Slots
are the level's resource indices, methods and streams included as gaps that are never present.

A `+batched` property's slot is a `Batched<T>`: a nibble tag, then `T` for `One` (tag 0), or for `Samples` (tag 1)
padding to the next byte and a `Series<T>` to the end of the slot. The event's `sample_times` (after `missed`, an
`Option<Series<Second>>`) is present whenever a `Samples` slot is, with one time per sample of each. In
introspection such a property is `batched: Some(SharedClock)`.

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

The [motor demo](../examples/motor_demo.md) (code in [motor_sim](https://github.com/vhrdtech/motor_sim)) observes a simulated FOC motor controller's ~50 properties
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
`+batched` properties come sample by sample (`DynObserved::samples`, `ww observe --samples MS`, Python's
`{"name": {"samples": max_age_ms}}`), see [Every sample](#every-sample-batched).
