---
level: basic
---

# Everything `Series`: from one line to the bit layout

*Roman Isaikin, 18 October 2026*

A motor controller measures its phase current 25 000 times a second. Written one value after another, 32 of those
readings with their timestamps take 321 bytes. As a `Series`, the same 32 readings take 34. Nothing is thrown away,
nothing is rounded, the reader gets back exactly what the device measured: the bytes were redundancy all along.

This article is the whole story of `Series<T>` in WireWeaver, in three parts. **Basic** is what it is, the one line
that uses it and what it saves, including batched streams and observing every sample. **Advanced** is every knob:
codec policies, quantities and autoranging, how it compares with ALP, sealing, budgets, the shared clock, Python,
and what it costs on a microcontroller. **Nomicon** is how the bits are laid out and how the encoder got six times
faster. Read until you have what you need; the numbers below were measured on the running code (the one worked out
from the layout says so), and each part links to the reference pages for the details.

<div class="ww-level-part" markdown>
<span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span>
</div>

## Basic: sequences that shrink

What a `Series` is, the line that uses it, what it saves, and how streams and observed properties batch into one.

### Values over time

Most of what a device sends is a sequence of values over time: currents, voltages, temperatures, counters, encoder
positions, timestamps. Consecutive values are close to each other, a timestamp grows by the same step every time,
and the unit or range of a reading almost never changes. A `Vec<T>` doesn't know any of that: it writes every item
in full.

`Series<'i, T>` is a list of `T` that does know. It writes the list **column by column** instead of item by item:
all the timestamps, then all the units, then all the values, each column compressed on its own with the code that
fits it best. A timestamp ticking at a fixed rate can cost nothing per item; a unit that doesn't change costs about
5 bits for the whole batch; a slowly moving reading costs a few bits per item instead of 4 bytes.

It is still `shrink_wrap`: zero-copy reading, `no_std` and no allocation on the device, and it evolves the way `T`
does (a field appended to `T` later is skipped by an older reader).

### One line

Mark the item type with `series` and write the slice:

```rust
use shrink_wrap::prelude::*;
use shrink_wrap::series::{Series, SeriesOwned};
use ww_si::{Ampere, Second, Timed};

let items: Vec<Timed<Ampere>> = /* 32 currents with their times */;
let bytes = Series::new(&items).to_ww_bytes_owned()?; // that's the line

let back: Vec<Timed<Ampere>> = Series::<Timed<Ampere>>::from_ww_bytes(&bytes)?.iter().collect();
let owned = SeriesOwned::<Timed<Ampere>>::from_ww_bytes_owned(&bytes)?; // std: straight into a Vec
```

`ww_si` quantities and `Timed<T>` already have `series`; your own types get it in their derive:

```rust
#[derive_shrink_wrap(borrowed, owned, series)]
struct IvSample { v: Volt, i: Ampere }
```

A `Series` goes anywhere a `Vec` would: a method argument or return value, a property, a stream item, a field of
another struct.

```rust
#[ww_trait]
trait Sequences {
    /// Voltage history with device time
    fn history() -> Series<'i, Timed<Volt>>;
    property!(rw stamps: Series<'i, u64>);
}
```

Structs, enums with or without data, `Option`, all integers, floats, `bool`, tuples, arrays and strings are in, nested
to any depth; [which `T`](../serdes/series.md#which-t) has the exact list and the few types that aren't yet.

### What it saves

The 32 currents from the top of the page: a motor phase current in mA as `I32`, around 5 A on a slow sine with a
few mA of noise, one sample every 40 µs:

| 32 × `Timed<Ampere>`              | bytes | per sample |
|-----------------------------------|-------|------------|
| `Vec<Timed<Ampere>>`              | 321   | 10.0       |
| `Series`, current stored as `f32` | 104   | 3.3        |
| **`Series`, current as `I32` mA** | **34**| **1.1**    |

On recorded test signals (`IvSample` above, a voltage and a current, 100 items):

| 100 items                                  | `Vec<T>` | smallest plain form | **`Series`** |
|--------------------------------------------|----------|---------------------|--------------|
| steady (±1 mV, ±2 µA)                      | 902 B    | 802 B               | **104 B**    |
| noisy (slow sine + noise)                  | 902 B    | 802 B               | **251 B**    |
| range switches (µA ↔ mA for 40 items)      | 902 B    | 802 B               | **110 B**    |

**3 to 8 times smaller** is the rule of thumb: 7.7x on a steady signal, 3.2x on a noisy one. A single item costs one
byte more than plain (the column headers are paid once), two items are already smaller, and most of the gain is
there by 32 items per batch, which is why 32 is the default batch size everywhere below.

### Quantities: integers with a prefix

One piece of advice comes out of every measurement: **store integers with a prefix, not floats**. A steady `f32`
voltage (100 values) is 136 B; the same signal as millivolts in a `U16` is 45 B (noisy: 277 B against 82 B). The
prefix sets the resolution, and noise below the resolution costs nothing; an `f32` pays for every mantissa bit the
ADC's noise touches.

That is exactly what a [`ww_si`](../std_library/ww_si.md) quantity is: an SI prefix and a number, usually an
integer (a firmware typically enables `I32`, `U32` and `F32` storage). `Ampere::milli(4980)` is 4 980 mA, and the
unit itself takes no bits. In a `Series` the prefix and the storage type are runs that cost a few bits per batch when
they don't change, and the integer column carries only the changes.

A device may also **change the prefix on the fly**: µA while a current is small, mA once it grows. This is
autoranging, and it costs almost nothing: at a prefix change the column starts over with a full value instead of a
meaningless delta across the jump, and goes on compressing in the new range. On the motor demo's currents, ranging
between µA and mA switched 528 times a second and still came out a little smaller than staying in µA (details in
the [Advanced part](#quantities-autoranging-and-restarts)).

### Batched streams: one word

You rarely build a `Series` by hand for a live signal. Mark a stream `batched` and the device collects items into
batches for you:

```rust
#[ww_trait]
trait Sensors {
    #[batched]
    stream!(readings: Sample);
}
```

On the device, push items wherever they come from, an interrupt included; the push is O(1) and takes no lock:

```rust
let ser = api_impl::stream_data_ser();
ser.readings_push(&mut self.readings_stream, sample, t);          // one item with its time
ser.readings_push_many(&mut self.readings_stream, t0, dt, block); // a DMA block at once
```

On the host, items arrive one at a time, each with its own time, exactly as from a stream that isn't batched, or a
whole batch at once with `recv_batch()`. The API has **no batch size**: the device owns it (32 by default), and the
host picks only the latency, `.max_age(duration)`, how long a batch may wait before it goes. Details:
[Batching streams and properties](../api/batching.md).

### Observing every sample

[Observing](../api/observing.md) a property sends its *current* value at the rate a host asks for. A value that
changes faster than that, a current sampled in a 25 kHz control loop, loses everything in between. Add `+batched`
and a host can ask for every sample:

```rust
#[ww_trait]
trait Foc {
    property!(ro+observe+batched i_a: Ampere);
    property!(ro+observe+batched i_b: Ampere);
}
```

The device hands over each new set of values once per control loop tick, and the host gets every one of them:

```rust
// device, in the control loop
server.observe.samplers().root.sample(Second::micro(now_us), api_impl::FocSamples { i_a, i_b });

// host
let mut i_a = device.foc().observe_i_a(samples(Duration::from_millis(5))).subscribe().await?;
while let Ok(s) = i_a.recv().await {
    println!("{:?} {:?}", s.t, s.value); // every sample, oldest first, each with its device time
}
```

### The motor demo, in numbers

The [motor demo](../examples/motor_demo.md) is a simulated FOC motor controller with about 50 observable properties
in three rate tiers. Its `foc` level has 21 values computed every 40 µs (25 kHz), `control` has 13 measured values at
1 kHz. Observing every single sample of a level, through the real generated code:

| every sample                         | `foc` (21 values, 25 kHz) | `control` (13 values, 1 kHz) |
|--------------------------------------|---------------------------|------------------------------|
| one update per sample, not batched   | 3.73 MB/s                 | 99.0 kB/s                    |
| **batched, 32 samples per batch**    | **812 kB/s**              | **19.0 kB/s**                |
| smaller by                           | **4.6x**                  | **5.2x**                     |

The flip side: unbatched, a `ww_si` quantity is a little *bigger* than a bare `f32` (it carries its prefix and storage
type): the demo's `foc` level observed every millisecond went from 96 to 117 kB/s when it moved to quantities.
Quantities pay off inside a `Series`, where a value costs its change from the previous one, not its width.

<div class="ww-level-part" markdown>
<span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span>
</div>

## Advanced: every knob, and what it costs

The choices behind a batch: which codecs, when it seals, how big it may get, what it costs a microcontroller, and
when to leave batching off.

### Codec policies: smallest, fixed, or fixed at compile time

Each numeric column is written with a **predictor** (raw, delta, delta of delta) and a **coder** (prefix code, bit
packed, constant, plain); floats are plain or XOR coded. Who picks them is the `SeriesPolicy`:

- `CodecPolicy::SMALLEST` (the default of `Series::new`, and of everything on `std`) tries every combination per
  column and keeps the smallest.
- `CodecPolicy::fixed(numeric, float)` uses one for every column, chosen at run time.
- A type implementing `FixedCodecs` fixes the codecs at **compile time** and links only them. `DeltaPacked` (delta,
  packed residuals, XOR floats) is one, and the `no_std` default of a batcher. `RawPlain` is another.

```rust
#[derive(Clone, Copy)]
struct DeltaOfDeltaPacked;

impl FixedCodecs for DeltaOfDeltaPacked {
    const NUMERIC: NumericCodec = NumericCodec::new(Predictor::DeltaOfDelta, Coder::Packed);
    const FLOAT: FloatCodec = FloatCodec::Xor;
}

Series::with_policy(&items, DeltaOfDeltaPacked).to_ww_bytes(&mut buf)?;
```

All of them write the same bytes for the same codecs; the reader follows the column headers, whatever wrote them.
What the choice costs, encoding 32 samples of a motor phase current (`Series<Timed<Ampere>>`) on a Cortex-M3:

| policy                          | instructions per item | bytes | encoder flash |
|---------------------------------|-----------------------|-------|---------------|
| `SMALLEST`                      | 969                   | 50    | 25.4 KB       |
| `fixed(delta, packed)`          | 341                   | 79    | 25.4 KB       |
| `fixed(delta-of-delta, packed)` | 334                   | 61    | 25.4 KB       |
| `DeltaPacked`                   | 341                   | 79    | 15.0 KB       |

So a fixed codec is about 3x cheaper in CPU than trying them all, for 1.2x to 1.7x the bytes. A run-time
`CodecPolicy` value links every codec whatever its value; only a `FixedCodecs` type leaves the rest out. And
`RawPlain` is not the CPU escape hatch it looks like: on an STM32H725 a level batch takes 567 k cycles with it
against 529 k with `DeltaPacked`, for 2.8x the bytes; writing more bits costs more than predicting saves.

Where the fixed codecs lose most is a regular clock. `SMALLEST` writes it as delta + constant, zero bits per item;
`DeltaPacked` has no constant coder and pays a packed residual per tick. The 32 currents from the Basic part are
34 B with `SMALLEST` and 65 B with `DeltaPacked`; without their times, 27 B and 30 B. That gap is what the shared
clock of observe levels (below) takes away.

### Quantities, autoranging and restarts

A `ww_si` quantity is a `final_structure` struct `{ prefix: Prefix, value: NumericValue }`, so a `Series` of
quantities has three columns per quantity: the prefix, the storage type of the number (`I32`, `U32`, `F32`, ..),
and the number itself, in one column per storage type that occurs.

Discrete leaves, enum tags like the prefix and the storage type, `bool`, `Option` presence and strings, are written
as **runs**: the first value, then one entry per change. A value that never changes in a batch costs about 5 bits in
all. Measured on the motor demo, the prefix and storage runs of a quantity and its timestamp cost 3 B per series per
update, 5 % of the bytes at 32 items: not worth a fixed-prefix wire form that would give up autoranging.

A numeric column **restarts** (a full value, fresh codec state) where an enum field declared earlier in the same
struct changed. `Ampere { prefix, value }` restarts `value` when the prefix changes, so 480 µA followed by 120 mA is
a full value, not a meaningless delta of -360 across two units. A switch costs a run entry, one full value instead of
a residual and a packed block cut short: about 6 B, worked out from the layout.

What that means in practice was measured on the motor demo's simulated FOC level (21 values at 25 kHz, 7 of them
currents, every sample, 32 per batch, an early prototype of batched observe with a time column per property and
`SMALLEST`), with the 7 currents stored three ways:

| currents                                   | bytes/s of the level | prefix switches/s |
|--------------------------------------------|----------------------|-------------------|
| fixed mA                                   | 913.1 kB/s           | 0                 |
| fixed µA                                   | 1 126.7 kB/s         | 0                 |
| µA below 1 A, mA above (autoranging)       | 1 114.4 kB/s         | 528               |

Autoranging between µA and mA switched 528 times a second and came out 12.3 kB/s below a fixed µA range: the
switches pay for themselves through the coarser range above 1 A. It still costs about what fixed µA costs, because 91
to 100 % of the current samples were below 1 A and were sent at µA resolution. **Resolution decides the bytes, not
switching**: pick the prefix for the resolution you need, and let the device range freely where its measurement
range does.

### Compared with ALP

ALP (Afroozeh, Kuffó, Boncz: [*ALP: Adaptive Lossless floating-Point Compression*](https://doi.org/10.1145/3626717),
SIGMOD 2024) is the state of the art for compressing doubles in analytical databases. It turns doubles that started
out as decimals into integers: for each vector of 1 024 values it finds an exponent `e` and a factor `f` (by sampling,
first per row group, then per vector) so that `round(n × 10^e × 10^-f)` decodes back to exactly `n`. Values that
don't round-trip are stored as exceptions (64 bits plus a 16-bit position each), the integers are bit packed with a
frame of reference, and doubles that aren't decimals go to a second scheme, ALP<sub>rd</sub>, which compresses
their front bits.

A `ww_si` quantity does ALP's first step at the producer. The device already holds an integer (an ADC count scaled
to mA, a time in µs), and the SI prefix is the decimal exponent, in steps of 10³ (with deci, centi, deca and hecto
around the unit). So there is no search, no exceptions, it is lossless by construction, and the exponent may change
per sample, where ALP keeps one per vector. What ALP has beyond that, a scale chosen from the data per block and at
any power of ten, is what the planned **common factor** column mode would add: the predictor code `11` is kept free
as an escape for it (see [Ahead](#ahead)).

### The store and the batcher

A device collects items in a `SeriesStore`, which `series` derives next to the encoder: room for `N` items as a
struct of arrays, one array per leaf field, no padding between them. `Timed<Ampere>` takes 12 bytes per item there,
against 24 in a `[Timed<Ampere>; 32]`.

A batched stream's or observe level's `Batcher` holds **two** such stores: the producer (an interrupt, the control
loop) fills one while the event loop encodes the other, and sealing swaps them behind one atomic word, no lock. On a
Cortex-M3 a push is about 62 instructions, the push that seals a batch 71, a poll that seals nothing 108. If the spare
store isn't back from encoding when the next batch seals, that batch is lost and counted, it never blocks the
producer. RAM for the motor demo's whole `foc` level at 32 samples: 8.4 KB.
[SeriesStore](../serdes/series.md#collecting-items-on-a-device-seriesstore) and
[the store](../api/batching.md#the-store) have the per-type layouts.

### Sealing and budgets

A batch is sealed for one of four reasons: it is **full** (`N` items), one more item could pass the event's **byte
budget**, it got as **old** as the host allows, or the code **flushed** it (on closing a stream).

The budget needs to know how big a batch can get without writing it. `Series::<T>::max_bytes(n)` is the worst case
under any policy, every column at its longest code and every run changing at every item, and it is far from real
data: the 32 currents above are 34 B against a bound of 1 272 B (with every number type of `NumericValue` enabled).
So the batcher starts from the bound and then seals by the last batch's measured bytes per item, aiming at 80 % of the
budget. A batch that still turns out too big is split, the largest prefix that fits goes now and the rest next time:
nothing lost, nothing sent twice. See [sealing](../api/batching.md#sealing) and [budget](../api/batching.md#budget).

### One clock per observe level

The batched properties of an observe level are sampled together, by one `sample(t, ..)` call, so they share their
times. A level batch is one `Series<T>` per observed property plus **one** `Series<Second>` of times in the update's
head (`sample_times`), not a time column per property. On the `foc` level's 21 properties at 32 samples with
`DeltaPacked`, that is 1 251 B against 1 965 B (36 % less) and half the encode cycles. Through the real motor demo
with `SMALLEST`, which already wrote each regular clock almost for free, it still saved 17 % (`foc`) and 26 %
(`control`).

Two things follow. One batcher per level seals the level's properties together: `control` went from 238 updates/s,
sealed out of step property by property, to 48. And a level's `Samples` entries share one grid and one age: they get
the finest `every_us` and the shortest `max_age_us` any of them asked for, and the grant says so. A property that
needs its own sample times belongs in its own trait level; levels are the rate tiers.

### Without generated code: CLI, dynamic clients, Python

A `Series` in an API is in its introspection data, so tools that only have that read and write it too. `ww stream
<path> --batches` prints whole batches, `ww observe --samples MS` one line per sample. The dynamic client decodes
items or columns, and it also **writes** a `Series` from a dynamic value with the same bytes the
generated code gives (`ValueOwned::ser_series_dyn`, any policy). In Python a `Series<T>` is a list, so a host writes
one as it would a list, and reads it as items or as numpy columns:

```python
dev.stamps.write([1_700_000_000, 1_700_000_010, 1_700_000_020])  # property!(rw stamps: Series<'i, u64>)

cols = dev.readings.decode_columns(data)  # one numpy array per leaf field
with m.observe({"current": {"samples": 5}}) as obs:  # every sample, batches at most 5 ms old
    b = obs.recv_batch()
    b["batches"]["current"]               # {"t": array of seconds, "": array of the values}
```

[Python: observing](../python_advanced.md#observing) and [values](../python.md#values) have the rest.

### CPU and flash on a microcontroller

Pushing is cheap and flat; **encoding** is where the CPU goes, and it scales with samples per second (properties ×
rate). Two kinds of numbers below, in different units:

- **Cortex-M3/M4F: instructions**, counted under QEMU (`examples_mcu/qemu_icount`). QEMU has no timing model, so
  these are not cycles and give no time on their own.
- **STM32H725 (Cortex-M7 at 540 MHz, caches on): cycles**, read from the core's DWT cycle counter on the real chip.
  Here one QEMU instruction took about 2 to 2.5 cycles, so a bigger number for the M7 doesn't mean more work.

With `DeltaPacked` at 32 samples per batch, a timed stream item is about 341 instructions on a Cortex-M3/M4F and
about 790 cycles on the H725, **1.46 µs** per item. An observe level's sample costs about half, thanks to the shared
clock: 213 instructions on a Cortex-M3/M4F. How that scales:

| observed (one level, shared clock)      | samples/s | Cortex-M3/M4F, instructions/s | fits                        |
|-----------------------------------------|-----------|-------------------------------|-----------------------------|
| 2 currents at 1 kHz                     | 2 000     | ~0.4 M                        | any Cortex-M, trivially     |
| 4 properties at 4 kHz                   | 16 000    | ~3.4 M                        | any Cortex-M, room to spare |
| 5 properties at 25 kHz                  | 125 000   | ~27 M                         | a fast M4F, an M7 easily    |
| all 21 FOC values at 25 kHz             | 525 000   | ~112 M                        | a fast M7 only              |

On the H725, the whole FOC level (21 values at 25 kHz, one batch of 32 samples every 1.28 ms) depends as much on the
compiler settings as on the codec. The generic writer walks the items of a batch; a column-wise writer that reads
straight from the batcher's store was measured next to it, with the same bytes (1 251 per batch):

| FOC level on the STM32H725                  | cycles per batch | per sample | share of 540 MHz |
|---------------------------------------------|------------------|------------|------------------|
| generic item walk, `opt-level = "z"`        | 302 200          | 0.83 µs    | 43.7 %           |
| generic item walk, `opt-level = 3`          | 125 300          | 0.35 µs    | 18.1 %           |
| column-wise from the store, `opt-level = "z"` | 100 400        | 0.28 µs    | 14.5 %           |
| column-wise from the store, `opt-level = 3`   | 59 900         | 0.17 µs    | 8.7 %            |

`opt-level = "z"` (smallest code) is the firmware examples' release profile, and it alone makes the encoder 2.4x
slower than `opt-level = 3`; at `"z"` the cycles also move by up to 15 % between builds as the code layout changes.
Build firmware that encodes at 25 kHz at `opt-level = 3`. Setting it only for `shrink_wrap`, `ww_si` and `ww_numeric`
doesn't help: the series code is generic and compiled in the firmware crate. The [Nomicon](#straight-from-the-store)
has where the cycles go.

Flash for the encoder is 15.0 KB on a Cortex-M3 with `DeltaPacked` (9.8 KB on an M4F), 25.4 KB (17.6 KB) with a
run-time `CodecPolicy`. Every sample taken is encoded, also when its batch is later dropped for lack of budget, so an
MCU sets a floor on the rate it accepts (`ObserveLimits::samples_min_every_us`): otherwise any host can ask for more
encoding than the CPU has.

### When not to batch

- **Latest value is enough.** If the host wants to *see* a value, not every sample of it, the plain observe modes
  (periodic, on change) are smaller and simpler. Batch what you would plot or analyze.
- **One value at a time.** A batch of 1 costs a byte more than plain; a quantity sent alone costs more than an
  `f32`. Batching pays from two items on and most of it by 16 to 32.
- **Latency matters more than bytes.** A batch waits up to its max age before it goes; an event that must arrive
  now is a plain stream item or a method's answer.
- **Unbounded items.** A type without a size bound (a string, a byte list, an opaque field) can't be batched, it's a
  compile error: the batcher couldn't keep a batch inside its budget.
- **The CPU can't keep up.** Check the table above; decimate (`every_us`) or observe fewer properties.

<div class="ww-level-part" markdown>
<span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span>
</div>

## Nomicon: the bits and the cycles

How a `Series` is laid out bit by bit, what each codec does, and where the encoder's cycles went.

### The column layout

A `Series` is a count, the columns, and padding to the next byte:

```text
n:          UNib32 (nibble-aligned)     number of items; 0: nothing else follows
group(T):   the columns of T over all n items
padding:    zero bits to the next byte
```

The group of a type is written by its own rule, recursively: a struct is its fields' groups in declaration order
(an evolvable one first writes its field count, or its bit length when nested, so an older reader can skip what it
doesn't know); an enum is a tag run, then each variant's fields over only the items in that variant; `Option` is the
same with two variants. Columns come depth first, which lets a reader find every column's start in **one forward
pass** without a directory of offsets: the skim validates every code, records a cursor per column, and `iter()` then
steps all columns in parallel, zero-copy, without allocating.

Four `IvSample`s, `v` = 3300, 3301, 3301, 3299 mV as `U16` and `i` = 152 000, 152 002, 151 999, 152 001 µA as `I32`,
are 14 bytes against 33 for the smallest plain form:

```text
bit  column                         bits
  0  n = 4                          0100
  4  IvSample field count = 2       0010
  8  v.prefix run: Milli, end       0001 0
 13  v.value tag run: U16, end      0010 0
 18  v.value U16 column             01 01                     delta, packed
                                    0000110011100100          3300
                                    00010 10 00 11            block width 2: +1 0 -2 (zigzag 2 0 3)
 49  i.prefix run: Micro, end       0010 0
 54  i.value tag run: I32, end      0110 0
 59  i.value I32 column             01 01                     delta, packed
                                    00000000000000100101000111000000   152000
                                    000011 100 101 100        block width 3: +2 -3 +2 (zigzag 4 5 4)
110  padding                        00
bytes: 42 11 14 33 90 51 91 8a 00 04 a3 80 1c b0
```

The first values and the 4.5 bytes of count, runs and headers are paid once per batch: that is why 4 items are 2.4x
smaller and 100 items 7.7x. The full reference is [`Series<T>` in the wire format](../serdes/wire_format.md#seriest).

### Predictors

A numeric column header is 4 bits: 2 for the predictor, 2 for the coder.

- **raw**: every value as it is (zigzag for signed types).
- **delta**: the segment's first value in full, then the zigzag of each difference to the previous value.
- **delta of delta**: the zigzag of the change of that difference. A regular clock becomes all zeros, a ramp after a
  step too; on a flat noisy signal it amplifies the noise (a second difference), so it's no universal win.

A **segment** starts at the first item and at every restart; the predictor state starts over there. The fourth
predictor code, `11`, is reserved as an escape for later column modes, so a new mode never changes what an existing
code means.

### Coders

- **prefix**: each residual in the variable-length delta code (`0` for zero, `10` + 7 bits, `110` + 9, ...).
- **packed**: blocks of 8 residuals, a width field (5 bits for 16-bit values, 6 for 32), then each residual at
  exactly that width. A block of zeros costs only its width field.
- **constant**: the first residual once; every later one equals it. Delta + constant is `t0 + period`.
- **plain**: every residual at full width.

Packing is where most of the gain over the older prefix code came from: a ±1 LSB delta costs 9 bits as a prefix code
and 2 to 3 packed. Over 100 values, a steady voltage column is 659 bits with the prefix code and 359 packed. The
prefix code still wins on flat data with rare jumps (235 against 433 bits), because a single jump widens a whole
block of 8, which is why the writer can choose per column, and why columns restart at a range switch: 110 B with
restarts against 118 B without on the range-switch signal.

### XOR floats

A float column is plain, or the XOR code known from Gorilla: each value XORed with the previous one, then only the
meaningful bits between the leading and trailing zeros, reusing the previous window when it fits. A repeated value is
one bit. It does what it can, but a noisy float changes most of its mantissa every sample: on the motor demo,
batching `f32` values only went from 4.57 to 3.42 bytes per value, while the same values as `I32` with a prefix went
to 1.74. Integers with a prefix were about 60 % of the whole gain. The [showcase](../serdes/showcase.md#compressed-sequences-of-numbers-series)
walks through XOR-coded temperatures bit by bit.

### How the encoder got 6x faster and 6x smaller

The first encoder was correct and slow. Profiled under QEMU, a 32-item batch with `fixed(delta, packed)` took 63 359
instructions on a Cortex-M3, and they went to:

- 27 %: restart checks building the tag arrays of every field, per item per column, also for fields that can't have
  a tag;
- 24 %: the packed coder's look-ahead, which ran the predictor twice per item;
- 20 %: nested walks over the items of each field;
- 15 %: the bit writer, a read-modify-write per byte with 64-bit masks.

Flash had its own story: 16 copies of a ~4 KB column writer, one per leaf position and sink, each with all codecs
and the trials. The fixes, each measured on its own: packed blocks buffered instead of looked ahead; restarts compare
only fields that can carry a tag; enums write only the variants their tag run saw; the predictor works on
left-aligned values in a `u32` for leaves up to 32 bits; `SMALLEST` sizes the four coders of a predictor in one pass;
the bit writer keeps the open byte in a register; column writers are small structs stepped per item, one copy per
word size; and policies became types (`FixedCodecs`), so a firmware links only its codecs. The bytes are the same as
before, down to the golden vectors.

| Cortex-M3, instructions, n = 8 / 32 / 64 | before                     | after                   |
|------------------------------------------|----------------------------|-------------------------|
| `SMALLEST`                               | 53 873 / 213 233 / 428 825 | 8 894 / 31 014 / 60 813 |
| `fixed(delta, packed)`                   | 16 488 / 63 359 / 126 075  | 3 274 / 10 922 / 21 272 |
| `DeltaPacked` (compile time)             | -                          | 3 250 / 10 920 / 21 306 |

| encoder flash      | before   | after, `CodecPolicy` | after, `DeltaPacked` |
|--------------------|----------|----------------------|----------------------|
| Cortex-M3          | 90 752 B | 25 436 B             | 14 976 B             |
| Cortex-M4F         | 68 460 B | 17 596 B             | 9 820 B              |

Per item at 32: 1 980 → 341 instructions with a fixed codec (5.8x), 6 663 → 969 with `SMALLEST` (6.9x); flash 6.1x
(M3) and 7.0x (M4F) smaller.

### On real silicon: the H725 cycle tables

QEMU counts instructions; an STM32H725 (Cortex-M7 at 540 MHz, caches on) counts cycles with its DWT counter:

| STM32H725, cycles, n = 8 / 32 / 64 | before                      | after                    |
|------------------------------------|-----------------------------|--------------------------|
| `SMALLEST`                         | 117 219 / 441 512 / 885 873 | 19 969 / 60 784 / 115 305 |
| `fixed(delta, packed)`             | 27 706 / 104 027 / 205 922  | 8 732 / 26 664 / 50 792  |
| `fixed(delta-of-delta, packed)`    | 30 153 / 110 380 / 216 670  | 8 759 / 26 406 / 49 943  |
| `DeltaPacked` (compile time)       | -                           | 8 293 / 25 241 / 47 929  |

A batch of 32 with `DeltaPacked` is 25 241 cycles, 47 µs. A cold first run is within 1 to 10 % of these warm numbers.
All of these were built at `opt-level = "z"`; at `opt-level = 3` the encoder was measured 2.4x faster.
The M7 spends about 2 to 2.5 cycles per QEMU instruction on this code (three flash wait states, a call per value
written, branchy code), more than the 1 to 1.5 first guessed, which is why the MCU table above quotes both.

### Why the shared clock halves the CPU

A `Timed<T>` item has a time column next to its value, and a time is itself a quantity: a prefix run, a storage tag
run and a value column, the same three columns as the value. So the time is half of the columns an encoder walks
per item, and sharing one time column across a level's properties removes nearly half the work. On the H725, one
`foc` level batch (21 properties, 32 samples):

| `DeltaPacked`, one level batch        | bytes          | cycles             |
|---------------------------------------|----------------|--------------------|
| 21 × `Series<Timed<T>>`               | 1 965          | 528 538            |
| 21 × `Series<T>` + one `Series<Second>` | 1 214 + 37 = 1 251 | 242 631 + 11 803 = 254 434 |
| saved                                 | **36 %**       | **52 %**           |

Per sample that is 786 → 379 cycles, and the full 25 kHz FOC rate went from about 77 % of the H725 to 37 %. (Both
measured in one build at `opt-level = "z"`; the same bench rebuilt later took 302 k cycles for the shared form with
QEMU's instruction count unchanged: at `"z"` the M7's timing moves with the code layout, the work doesn't.) The bytes
drop more than expected because `DeltaPacked` has no constant coder: each 40 µs step is a 7-bit residual, so each
property's time column cost about 36 B per batch, 38 % of the update. One shared column is 37 B for the whole level.
QEMU agrees: 46 % fewer instructions on both the M3 and the M4F.

### Straight from the store

Which part of that is the codec and which is walking items? Two experiments on the same FOC level batch answer it.

The first is a **column-wise writer**: instead of rebuilding each item from the batcher's struct-of-arrays store and
walking the items once per column, it reads each column's array straight from the store, one pass per column. It
writes the same bytes as the item walk (a property test over every policy checks it) whenever no enum or `Option`
tag changes inside the batch; a batch with such a change, strings, or the odd-width `UN`/`IN` numbers still takes the
walk. The second is **aligned plain**: values copied byte-aligned in their own width, no prediction, no bit packing,
behind the predictor escape. Below both, an upper bound: the 22 payload arrays of the batch copied with `memcpy`.

| one FOC level batch                  | bytes | H725 cycles, `opt-level = 3` | H725 cycles, `"z"` | Cortex-M3 instructions |
|--------------------------------------|-------|------------------------------|--------------------|------------------------|
| `DeltaPacked`, item walk             | 1 251 | 125 300                      | 302 200            | 143 463                |
| `DeltaPacked`, column-wise           | 1 251 | 59 900                       | 100 400            | 74 898                 |
| aligned plain, item walk             | 2 882 | 44 400                       | 323 500            | 73 071                 |
| aligned plain, from the store        | 2 882 | 34 200                       | 128 700            | 40 987                 |
| `memcpy` of the arrays, with framing | 2 861 | 7 000                        | 8 800              | 10 609                 |
| `memcpy` of the arrays, bare         | 2 816 | 2 800                        | 2 900              | 3 911                  |

The surprise is the third row: walking items with aligned columns is already 44 k cycles at `opt-level = 3`. So the
item walk was not the main cost; the **bit writer** and the codec are. Column-wise, a column still costs about 3.3 k
instructions on a Cortex-M3: about 1.2 k for the delta and block code, 1.2 k for writing bits (24 bits per push,
one byte per loop), and 0.8 k for the column's own `Series` framing (count, two tag runs, the checks).

**Aligned plain was measured and is not recommended.** From the store it is 1.75x cheaper than column-wise
`DeltaPacked` at `opt-level = 3` (34 k against 60 k cycles), but slower at `"z"` (129 k against 100 k), and it
sends 2.3x the bytes: 2.25 MB/s for the FOC level at 25 kHz, more than a full-speed USB link carries. It isn't a
plain copy either: the store keeps every `NumericValue` payload as a `u64` (firmware enables 64-bit numbers for the
protocol's own types), so an `I32` column is narrowed value by value, and the framing of 22 separate series, their
tag runs and checks, already costs 5x the `memcpy` bound. And a new wire form is one that every older reader
rejects. It stays an option for a device on USB HS or Ethernet that needs the last bit of CPU. The column-wise
writer, in contrast, changes no byte on the wire: it is purely a faster way to write the same `Series`.

### What a struct series costs: the restart check

The obvious alternative, the level batch as one `Series<Timed<LevelRow>>` with an `Option<T>` field per property,
was measured and dropped. It is a little smaller (1 233 B against 1 251 B) but took about 2.0 M cycles on the H725
against 0.3 M. Inside a struct, a column restarts its predictor when a tag of an *earlier* field changes. With 21
tagged quantity fields that check is quadratic per item, and a decimated property's `None` rows would restart every
later column's deltas. Restarts are the right rule for a quantity's own prefix; across unrelated properties they're
pure cost, so a level is separate series per column.

### Ahead

- **Observe cells.** Proposed for a hot interrupt level, like the motor demo's `foc`: an opt-in alternative to
  `sample()` with double stores, per level a ring of rows of atomic 32-bit cells. The interrupt writes each value
  where it computes it, a plain store through a setter typed by the API, on a writer that is handed out once and
  can't be cloned (no `static mut`, no `unsafe`); `poll_properties` encodes straight from the columns, which already
  are the layout the encoder wants. A row the writer overtook while it was read is detected and counted as missed.
  The wire doesn't change, so a host can't tell the two apart.
- **A faster bit writer.** After the column-wise writer the bit writer is the biggest cost left: writing into a
  64-bit accumulator, 4 bytes per store, is next, then a `u32` payload in the store of `NumericValue` when no 64-bit
  number type is enabled, and word-wise checks of the columns.
- **Room kept in the format.** The reserved predictor escape is for a common factor per column, the values divided
  by it before the predictor, as [ALP](#compared-with-alp) chooses its scale per block; the float codes have room for
  better float coders (Chimp, ALP). Diffs across batches would save the first values,
  about 8 B per batch, but only on reliable streams and with resync rules after a gap; not worth it at 32 items.

The feature tracker has the state of all of this: [SW-60](../features.md), SW-62, SW-63 for the type, the store
and the fast encoder, API-41, API-39 and API-49 for batched streams, batched observe and the shared clock.
