---
level: advanced
---

# Batching streams and properties

Batching (API-41) lets a stream or a `+observe` property send many samples as one [`Series`](../serdes/series.md)
(SW-60) instead of one item per event: one header, one presence bit and one set of per-column overhead for the
whole batch, not for every item. It's opt-in, per stream or per property, and the device owns the batch itself -
there is no batch size in the API, only a host-picked latency (how long a batch may wait before it's sent).

## How to batch a stream

Add `batched` (or `batched(untimed)` for an item type that already carries its own position or time) next to a
stream's other directives:

```rust
#[ww_trait]
trait Sensors {
    #[batched]
    stream!(readings: Sample);                            // plain, timed

    #[counted]
    #[batched]
    stream!(counted_readings: Sample);                    // counted, timed

    #[stream(Sample, reliable, batched)]
    fn calibrated(gain: i32) -> Result<(), Error>;         // reliable, timed

    #[stream(Frame, counted, batched(untimed))]
    fn frames();                                            // counted, untimed: Frame has no own time
}
```

`batched` goes next to `reliable`/`counted` in `#[stream(..)]`'s argument list, or as its own `#[batched]`
attribute above a plain or `#[counted]` `stream!`. `Sample`/`Frame` need `#[derive_shrink_wrap(.., series)]` (see
[which `T`](../serdes/series.md#which-t)); a timed stream's item becomes `ww_si::Timed<Sample>` on the wire, so the
API crate needs `ww_si`. A type without a series size bound (a string, a byte list, an opaque field) is a compile
error on `batched`.

### Device side

The stream field's type becomes `ww_stream::batch::Batched<Sink, Timed<Sample>, N>` (or `Batched<Sink, Frame, N>`
untimed), built as `Batched::new(sink, policy)` from the same `Sink` an unbatched stream would use - `Plain`,
`Counted`, or `Reliable<Store>` (see [Counted and reliable streams](counted_streams.md)) - and a
[`SeriesPolicy`](../serdes/series.md#cpu-and-flash-on-an-mcu), `ww_stream::batch::DEFAULT_POLICY` usually. `N` is
the batch size (`DEFAULT_BATCH` = 32), a compile-time constant of the server, not part of the API.

```rust
struct Server {
    readings_stream: Batched<Plain, Timed<Sample>, 32>,
    // ..
}

async fn stream_readings(&mut self) -> &mut Batched<Plain, Timed<Sample>, 32> {
    &mut self.readings_stream
}

// feeding it, anywhere in the device's own code - an ISR, a control loop tick
let ser = api_impl::stream_data_ser();
ser.readings_push(&mut self.readings_stream, sample, t);               // one item, its own time
ser.readings_push_many(&mut self.readings_stream, t0, dt, block);      // a regularly-sampled block at once
ser.frames_push(&mut self.frames_stream, frame);                       // untimed: no `t`

// event loop, next to `poll_properties`
let mut scratch = Box::new(ww_stream::batch::Scratch::<SB>::new());
server.poll_streams(&mut out, now_ms, &mut buf, 4096, &mut scratch).await?;
```

`<name>_push`/`<name>_push_many` are O(1) and interrupt-safe (a copy of the item's fields and two atomic
operations, see [Sealing](#sealing) below), returning a [`Seal`](#sealing) - whether this push sealed a batch, and
whether anything was lost. `poll_streams` takes one extra argument next to the buffer and message-length bound an
unbatched server already passes: a `Scratch<B>`, shared by every batcher of the server, where a sealed batch's
items are rebuilt before they're written as a `Series` (see [the store](#the-store)). `close_<name>`/the stream
closing should call `.batcher.flush()` to send a half-full batch rather than wait out `max_age`.

### Host side

Opening a batched stream is like opening any other (`client.readings().call()`, etc.), plus `.max_age(duration)`
to pick the latency: the device clamps it to its own bounds and answers with what it granted
(`StreamOpened::max_age_us` on the wire). Items come one at a time exactly as from an unbatched stream -
`recv_event()`/`recv()`, each with its own `t` (or none, untimed) and its own position, as if it had been sent on
its own - or `recv_batch()` gives a whole batch at once (`pos`, `items`, oldest first). The dynamic
client (`DynStreamOpen`, no generated code needed) and `wire_weaver_py`'s `recv_batch()` (numpy columns, via
[`decode_columns`](../python.md#values)) give the same two ways; `ww stream <path> --batches` prints whole batches
from the CLI instead of one line per item.

## How to batch property samples

A `ro+observe+batched` property sends every sample it takes, not just its latest value, when a host observes it
in `Samples` mode - this is API-39, documented in full at
[Observing properties: every sample](observing.md#every-sample-batched) (the device's `<Level>Sampler`, the host's
`samples(max_age)`, Python). It uses the same `Batcher` as a stream's, one per trait level with `+batched`
properties: all of a level's `+batched` properties share one call, `sample(t, <Level>Samples { .. })`, so the
batcher keeps rows (the time and every value) and a level batch is sent as one `Series<T>` per observed property plus
**one** `Series<Second>` of times for all of them, the update's `sample_times` (API-49, a shared clock). The batches
feed a level-wide [`Outbox`](#the-store) instead of a stream's own store - the rest of this page (sealing, budget,
cost) applies to it the same way.

## Advanced

### Sealing

A batch is sealed - handed from the filling side to the side that encodes it - for one of four reasons
(`ww_stream::batch::Cause`): it reached `N` items (`Full`), one more item would pass the event's byte budget
(`Budget`), it got as old as the host allows (`Age`), or the user code asked for it early (`Flush`, e.g. on
`close_<name>`). `Full`/`Budget` are decided by the push itself (`Producer::push`); `Age`/`Flush` by the event
loop's next `poll`/`flush` call.

Each `Batcher` holds two stores of `N` items: one filling, one spare. Sealing swaps them - the filling store
becomes the sealed one, the spare (now empty) starts filling - behind one atomic word (a push is a `fetch_or` and
a compare-exchange, no lock, safe to call from an interrupt; see the module's own doc comment,
`ww_stream/src/batch/mod.rs`, for the exact bit layout). If the spare isn't back from being encoded yet when a
batch would seal, its items are lost instead (`Seal::Lost`) and the store starts over; losses are counted
(`BatchStats::lost`) and reported in the next batch's `lost_before`, which becomes a stream's `missed`
([Stream delivery](stream_reliability.md)) or an observe level's `missed`.

### Budget

The event loop gives the batcher a byte budget per event (`set_budget`); the producer seals at the most items that
estimate allows, so the batch it hands over always fits. Two estimates, the bigger one wins: the worst case
(`Series::max_bytes`, from the type's [size bounds](../serdes/series.md#size-bounds) - always safe, but usually far
from what real data takes) before anything has been encoded, and after that, the last batch's measured bytes per
item with a margin (aimed at 80% of the budget) - real data is almost always much smaller than the worst case, so
this measured limit quickly lets through far more than `N` would need. A sealed batch that still turns out bigger
than the room it's encoded into is split: the largest prefix that fits goes out, the rest waits for the next call,
so nothing is lost or sent twice over a budget surprise.

For a stream, the budget is the event/message size; for observe, it's a level's
[byte budget](observing.md#budget), shared with every other property of the level - a sample taken is always
encoded (so an MCU needs a floor on sample rate, see [below](#which-rates-fit-an-mcu)), but a batch that then
doesn't fit the level's [`Outbox`](#the-store) is dropped, oldest first, and counted as `missed`.

### The store

A `Batcher<T, N>`'s two stores are `T::Store<N>` ([`SeriesStore`](../serdes/series.md#collecting-items-on-a-device-seriesstore),
SW-62): a struct of arrays, one per leaf field, with no padding between items - usually less RAM than `[T; N]`
twice over (`SeriesStore`'s own page has the exact per-item sizes). Encoding reads a sealed batch's items back out
of its store into the shared `Scratch<B>` (`scratch_bytes::<T>(N)` sizes it for one batcher; a server with several
batchers shares one `Scratch` big enough for its biggest), then writes them with the ordinary `Series::with_policy`
- no second writer path, so [`Series`](../serdes/series.md) itself needed no change for this. A `T` aligned to
more than 16 bytes, or one whose `Store<N>` doesn't fit `B`, is a compile error, not a runtime one.

An observe level's `Outbox<BYTES>` (see [Every sample](observing.md#every-sample-batched)) sits after the `Batcher`:
each level batch is its time column followed by the column of each property observed in `Samples` mode (each entry
with a header: slot, items, length), encoded once, column by column (the scratch holds one column, not a row). They
wait there, oldest first, for `poll_properties` to send each whole in one update as budget allows; when it's full,
the oldest batches are dropped to make room, also counted as `missed`.
A stream has no outbox - its encoded batch goes straight into the stream's own store (reliable) or the next event
(plain/counted).

### Cost: CPU and flash

Pushing an item and polling for a seal are cheap and flat regardless of batch size - on a Cortex-M3
(`examples_mcu/mcu_qemu`, `Timed<Ampere>`, instructions counted under QEMU): a plain push about 62 instructions,
the push that fills and seals a batch about 71, a poll that seals nothing about 108, one that seals a due batch
about 159. No interrupt is masked either way (`LDREX`/`STREX`, not a critical section, except on `thumbv6m` cores
without compare-exchange).

Encoding is where the cost scales with batch size - see [`Series`: CPU and flash on an
MCU](../serdes/series.md#cpu-and-flash-on-an-mcu) for the measured table (per-item instructions and flash by
codec, Cortex-M3/M4F/M7, after SW-63's rework of the column writers: a fixed codec at n = 32 costs about 341
instructions per item on a Cortex-M3, 2.4-6.9x less than before it, and `DeltaPacked` (the `no_std` default,
delta-packed residuals and XOR floats, chosen at compile time) links only its own codecs - about 15 KB of flash on
an M3 against 25 KB for a runtime `CodecPolicy`). The `Batcher` itself (rebuilding a batch into the scratch, giving
the store back) is a small fraction of that: about 2 270 of the roughly 65 000 instructions a 32-item encode takes
on a Cortex-M3 with the pre-SW-63 numbers, the rest being the `Series` encode proper.

#### Which rates fit an MCU

Count samples per second as properties (or a stream's items) x rate. A timed stream encodes each item with its own
time: about 341 instructions per item on a Cortex-M3/M4F with `DeltaPacked`, n = 32, about 790 cycles on an
STM32H725. Observed properties share their level's time column (API-49), which halves that: measured on the motor
demo's 21 FOC properties (`I32` storage, n = 32, `DeltaPacked`), a level batch of 672 samples takes about 213
instructions per sample on a Cortex-M3/M4F (QEMU) and about 380 cycles per sample on the H725, against 395 and 786
with a time column per property, and 36 % fewer bytes (1 251 B against 1 965 B):

| Observed (one level, shared clock) | Samples/s | Cortex-M3/M4F, instructions/s | STM32H725 (M7, 540 MHz), cycles/s | Fits |
|---|---|---|---|---|
| 2 currents at 1 kHz | 2 000 | ~0.4 M | ~0.8 M | any Cortex-M, trivially |
| 4 properties at 4 kHz | 16 000 | ~3.4 M | ~6.1 M | any Cortex-M, with room to spare |
| 5 properties at 25 kHz ("FOC subset 5") | 125 000 | ~27 M | ~48 M (9% of the core) | a fast M4F, comfortably an M7 |
| all 21 FOC properties at 25 kHz | 525 000 | ~112 M | ~199 M (37% of the core) | a fast M7 next to the control loop; no smaller part |

So on a small MCU, observe or stream a handful of properties at a few kHz, or decimate (`every_us` for observe);
the full FOC rate above takes a fast M7, else it is for the simulator and the host
([motor demo](#the-motor-demo-measured)). A device sampling faster than its CPU can encode needs a floor on the rate
it accepts (`every_us`/sample-rate clamps), since every sample taken is encoded whether or not its batch later fits
the budget.

### Observe cells, measured

A level on [observe cells](observing.md#cells-sampling-from-an-interrupt) moves the copy of a row out of the
interrupt: the setters store each value into a ring where it is computed, `poll_properties` copies the waiting rows
into a staging batch. Measured on the motor demo's 21 FOC properties (`examples_mcu`: `stm32h725ig-foc-observe-cycles`,
DWT cycles at 540 MHz with caches on; `qemu-foc-observe` and `qemu-m4f-foc-observe`, instructions under QEMU), a host observing all of
them in `Samples` mode, a poll every 16 rows, `observe_batch = 32`, a ring of 32 rows:

| STM32H725, cycles | `opt-level = "z"`: `sample()` | z: cells | `opt-level = 3`: `sample()` | 3: cells |
|---|---|---|---|---|
| a row, observed: mean (first, worst) | 412 (576, 641) | 354 (426, 545) | 419 (545, 858) | 328 (422, 598) |
| a row, nobody observes | 204 | 300 | 153 | 284 |
| a poll that encodes a batch | 349-509 k | 356-518 k | 133-185 k | 137-191 k |
| a poll with nothing to send (16 rows waiting) | 5.4 k | 15.3 k | 3.0 k | 9.2 k |
| stack of a row | 108 B | 44 B | 116 B | 52 B |

On the Cortex-M3 and M4F models a row takes 189 instructions on cells against 250 through `sample()` at
`opt-level = "z"`, 206 against 260 at 3. Both send the same 2 021 bytes for the 64 rows; the level's RAM is the same
too (8 640 B against 8 656 B). The total CPU is about the same, the cells' a bit more: what the interrupt saves, the
event loop spends copying the ring (about 380 cycles a row on the H725). Without observers the cells' setters still
store (the sampler returns before copying): skip them with `is_sampling()`. Both builds keep the batches in a
`static` built in place (`ObserveBatches::init`, see [Observing](observing.md#device-side)).

### The motor demo, measured

The [motor demo](../examples/motor_demo.md) (`motor_sim` repo) has every `Foc` property and `Control`'s 13
measured values `+observe+batched`. Measured through the real generated code (one level observed per session, at
its own level budget so budget never throttles what's measured, `CodecPolicy::SMALLEST` as `std` uses, batch 32):

| | `foc` (21 properties) | `control` (13 measured) |
|---|---|---|
| today, periodic every sample, `ww_si` quantities, unbatched | 3.73 MB/s | 99.0 kB/s |
| batched, `Samples` mode, n = 32, a time column per property (before API-49) | 983 kB/s | 25.8 kB/s |
| batched, `Samples` mode, n = 32, one time column per level (API-49) | 812 kB/s | 19.0 kB/s |
| gain over unbatched | **4.6x** | **5.2x** |

(`motor_demo observe --foc samples:20ms --control off --telemetry off --budget 100000000`, and the same per level,
8 s each; the "before" row from the same tool built on the commit before API-49.) The shared clock saves 17 % on
`foc` and 26 % on `control` here, less than the 36 % of the `DeltaPacked` bench in
[which rates fit](#which-rates-fit-an-mcu): `SMALLEST` already wrote each property's regular clock as a nearly
constant run, so what goes is mostly the per-column overhead and one `Series` head per property. On `no_std`, with
`DeltaPacked`, a time column costs about as much as a value column, and sharing it is worth more. One batcher per
level also seals the level's properties together: `control` sends 48 updates/s, one batch per 20 ms max age (its
1 kHz fills 20 rows in that time), where a batcher per property sealed out of step and sent 238.

The flip side: an unbatched `ww_si` quantity (prefix + storage tag + the integer) is bigger on the wire than a bare
`f32`, and with no series to amortize it across, every value pays that cost alone - a single update of one
quantity already costs more than the same `f32` would. Quantities only pay off once they're batched into a
`Series`; see [`Series`: integer storage](../serdes/series.md#integer-storage-beats-floats) for the general case,
and the gain above for what it looks like once they are.

## Compatibility

Adding or removing `batched`/`batched(untimed)` on a stream, or switching between timed and untimed, changes its
item type on the wire (one `Series<Timed<T>>` or `Series<T>` per event instead of one `T`) - a break of the API
crate, same as any other item-type change; see [evolution rules: API](../evolution/rules.md#api). Making a property
`+batched` or not changes its observe slot (a `Batched<T>` instead of a `T`): a break too. The times of a level's
samples travel in the `PropertyUpdate` event (`sample_times`), not in a slot, so adding resources to such a level
follows the usual rules. The batch size (`N`) is the device's own and isn't in the API at all, so changing it is
never a break.
