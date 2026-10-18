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
`samples(max_age)`, Python). It uses the same `Batcher` as a stream's, one per `+batched` property's trait level
(all of a level's `+batched` properties share one call, `sample(t, <Level>Samples { .. })`) feeding a level-wide
[`Outbox`](#the-store) instead of a stream's own store - the rest of this page (sealing, budget, cost) applies to
it unchanged.

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

An observe level's `Outbox<BYTES>` (see [Every sample](observing.md#every-sample-batched)) sits after the `Batcher`: encoded
batches (header: property index, items, length) wait there, oldest first, for `poll_properties` to pack into
updates as budget allows; when it's full, the oldest batches are dropped to make room, also counted as `missed`.
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

Count samples per second as properties (or a stream's items) x rate, instructions per second as that x the
per-item encode cost above (about 341 on a Cortex-M3/M4F with `DeltaPacked`, n = 32):

| Observed | Samples/s | Cortex-M3/M4F, instructions/s | STM32H725 (M7, 540 MHz), cycles/s | Fits |
|---|---|---|---|---|
| 2 currents at 1 kHz | 2 000 | ~0.7 M | ~1.6 M | any Cortex-M, trivially |
| 4 properties at 4 kHz | 16 000 | ~5.5 M | ~12.6 M | any Cortex-M, with room to spare |
| 5 properties at 25 kHz ("FOC subset 5") | 125 000 | ~43 M | ~99 M (18% of the core) | a fast M4F, comfortably an M7 |
| all 21 FOC properties at 25 kHz | 525 000 | ~179 M | ~414 M (77% of the core) | not realistic next to the control loop on any Cortex-M part measured so far |

So on an MCU, observe or stream a handful of properties at a few kHz, or decimate (`every_us` for observe); the
full FOC rate above is for the simulator and the host ([motor demo](#the-motor-demo-measured)), not firmware. A
device sampling faster than its CPU can encode needs a floor on the rate it accepts (`every_us`/sample-rate
clamps), since every sample taken is encoded whether or not its batch later fits the budget.

A further cut is possible when every `+batched` property of a level is sampled through the same `sample(t, ..)`
call and so shares one time column instead of paying for it per property - rather than each carrying its own. This
is under active design as this page is written, not shipped yet; a later revision of this page covers it once it
lands.

### The motor demo, measured

The [motor demo](../examples/motor_demo.md) (`motor_sim` repo) has every `Foc` property and `Control`'s 13
measured values `+observe+batched`. Measured through the real generated code (one level observed per session, at
its own level budget so budget never throttles what's measured, `CodecPolicy::SMALLEST` as `std` uses, batch 32):

| | `foc` (21 properties) | `control` (13 measured) |
|---|---|---|
| today, periodic every sample, `ww_si` quantities, unbatched | 3.73 MB/s | 99.0 kB/s |
| batched, `Samples` mode, n = 32 | 995 kB/s | 26.2 kB/s |
| gain | **3.7x** | **3.8x** |

The flip side: an unbatched `ww_si` quantity (prefix + storage tag + the integer) is bigger on the wire than a bare
`f32`, and with no series to amortize it across, every value pays that cost alone - a single update of one
quantity already costs more than the same `f32` would. Quantities only pay off once they're batched into a
`Series`; see [`Series`: integer storage](../serdes/series.md#integer-storage-beats-floats) for the general case,
and the gain above for what it looks like once they are. `control`'s 13 properties also don't yet seal in lockstep
(238 updates measured in a window where `foc`'s sealed almost exactly on the idealized count) - aligning a level's
batches to seal together is a further gain not yet implemented.

## Compatibility

Adding or removing `batched`/`batched(untimed)` on a stream, or switching between timed and untimed, changes its
item type on the wire (one `Series<Timed<T>>` or `Series<T>` per event instead of one `T`) - a break of the API
crate, same as any other item-type change; see [evolution rules: API](../evolution/rules.md#api). The batch size
(`N`) is the device's own and isn't in the API at all, so changing it is never a break.
