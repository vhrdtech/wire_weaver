---
level: advanced
---

# `Series<T>`: compressed sequences of any derived type

SW-60. `Series<'i, T>` is a list of `T` written column by column instead of item by item: one column per leaf
field, each compressed on its own. For samples that change slowly (sensor readings, counters, timestamps) it is
3 to 8 times smaller than a plain `Vec<T>`, without giving up evolution, zero-copy reading, or `no_std`/no-alloc.

A `Series` is a type you reach for explicitly today, in a property, method argument/return or stream item, the same
way you'd use `Vec<T>`; a `Series` of plain numbers is the [compressed number sequence](special_types.md#compressed-sequences-of-numbers-seriest).
A stream or property can also opt into batching many samples into one `Series` automatically instead of sending one
item at a time (API-41, see [Batching](../api/batching.md)); nothing below needs that.

## Which `T`

Any `#[derive_shrink_wrap(.., series)]` struct or enum, nested to any depth, including generic ones
(`Timed<T>`, [DER-14](derive_reference.md#generics)): structs (named fields), unit-only enums and enums with
data, `Option`, integers (`u8`..`u64`, `i8`..`i64`, `U1`..`U64`, `I2`..`I64`, 128-bit), `f32`/`f64`, `bool`, tuples,
arrays, `&str`/`String`/bytes (as an opaque run), and any other type marked `#[series(opaque)]`. A field of a user
type needs `series` on that type too; see [`series`](derive_reference.md#series) for the exact directive and what it
doesn't take in this first version (recursive types like `RefBox`, `Result`, lists other than of `u8`, `TailSize`,
sparse and tuple structs) — each is a compile error naming the field, not a silent fallback.

```rust
#[derive_shrink_wrap(borrowed, owned, series, final_structure)]
struct Volt { prefix: Prefix, value: NumericValue }

#[derive_shrink_wrap(borrowed, owned, series)]
struct IvSample { v: Volt, i: Ampere }
```

## Why it's smaller

A plain `Vec<T>` writes one `T` after another; a `Series` writes one *column* per leaf field, over all items, and
picks the cheapest representation per column:

- **Discrete leaves** (enum tags — so a ww_si prefix or a `NumericValue` storage tag, `bool`, `Option` presence,
  strings) are **runs**: the first value, then one entry per change. A value that never changes across a whole
  batch costs about 5 bits total, not a bit (or a byte) per item.
- **Numeric leaves** are a **numeric column**: a predictor (raw, delta, delta of delta) and a coder (the SW-31
  prefix code, a new block bit-packed coder, constant, or plain) picked per column, by trying each and keeping the
  smallest. A column restarts (a full value, fresh codec state) where an **enum field declared earlier in the same
  struct changed** — so a current reading that jumps from the µA range to the mA range gets a full value instead of
  a meaningless delta across the jump.
- **Floats** are a **float column**, plain or the [XOR code](wire_format.md#the-delta-and-xor-codes) (Gorilla), but integers with a predictor compress markedly better than floats — see
  [below](#integer-storage-beats-floats).

A `Series` is self-contained: every batch carries its own first values and codec headers, so a lost batch never
corrupts the next one, on any stream.

The full bit-level layout (column order, run and numeric column encoding, restarts, evolution) is in
[wire format internals](wire_format.md#seriest); this page is about when and why to reach for it.

## Sizes against `Vec<T>`

`IvSample { v: Volt, i: Ampere }`, `v` as millivolts (`U16`), `i` as microamps (`I32`), 100 items per batch
(100 ms at 1 kHz):

| 100 items | `Vec<T>` (evolvable) | `Vec<T>` (`final_structure`) | **`Series`** |
|---|---|---|---|
| steady (v ±1 mV, i ±2 µA) | 902 B | 802 B | **104 B** |
| noisy (slow sine + noise σ≈8 mV / 300 µA) | 902 B | 802 B | **251 B** |
| range switches (i jumps µA ↔ mA for items 30..70) | 902 B | 802 B | **110 B** |
| exactly flat, a spike every 25 items | 902 B | 802 B | **88 B** |

7.7x smaller on a steady signal, 3.2x on a noisy one, against the smallest plain form. With time added
(`Series<Timed<IvSample>>`, `t` as microseconds since batch start) against a `final_structure` `Vec` of
`{ t, v, i }` (13 B/item plain): 113 B steady, 260 B noisy, regular 1 kHz; a regular clock costs nothing extra
(constant delta), ±3 µs of jitter adds about 6 bits/item. [`Timed<T>`](#time-timedt) is just another struct field,
not a special case.

Bytes per item by batch size, steady signal (plain `final_structure` / `Series`):

| n | 1 | 2 | 4 | 8 | 16 | 32 | 64 | 128 | 256 |
|---|---|---|---|---|---|---|---|---|---|
| bytes/item | 9.0 / 10.0 | 8.5 / 6.0 | 8.3 / 3.5 | 8.1 / 2.1 | 8.1 / 1.6 | 8.0 / 1.2 | 8.0 / 1.1 | 8.0 / 1.0 | 8.0 / 0.9 |

A single item costs one byte more than plain (the column headers have to be paid once); from 2 items on `Series` is
already smaller, and most of the gain is there by 32 items — the default batch size the planned API-41 batching
is expected to use.

The [showcase](showcase.md#compressed-columns-seriest) has the exact bytes of a 4-item batch, checked against the
running code.

## Integer storage beats floats

A steady `f32` voltage (100 values) is about 136 B with the XOR code; the same signal stored as `U16` millivolts with a
`ww_si` prefix is 45 B. Noisy: 277 B vs 82 B. The prefix sets the resolution, and noise below that resolution costs
nothing — so a device sending `ww_si` quantities (which carry a prefix and an integer storage tag by construction)
gets this for free, while a bare `f32` pays for every bit of mantissa the ADC's noise floor touches.

This has a flip side: *unbatched*, a `ww_si` quantity (prefix + storage tag + the integer) is bigger than a bare
`f32` in a single plain update — measured on a motor control demo, moving one observe property from `f32` to
`ww_si` went from 96 to 117 kB/s. Quantities pay off once they're batched into a `Series`; sending one at a time
gets you the worse of both.

## Time: `Timed<T>`

`ww_si::Timed<V, Tm> { t: Tm, value: V }` ([DER-14](derive_reference.md#generics) generic struct with
`series`) is not a special case of `Series` — it gets a `t` column like any other field, and a regular sampling
rate costs about nothing (delta + constant: `t0 + period`). `t` is a [time point](../std_library/ww_si.md#which-time-point),
`DeviceMicros<u32>` by default: a plain `u32` column, no prefix run and no number tag. Use `Series<Timed<T>>`
wherever a batch needs its own timestamps; where a stream already carries time for the whole batch, a plain
`Series<T>` works just as well.

## Using it

```rust
use shrink_wrap::series::{Series, SeriesOwned};

let items: Vec<IvSample> = /* ... */;
let bytes = Series::new(&items).to_ww_bytes_owned()?;       // picks the smallest codec per column
// or, on a small MCU: fixed codecs, chosen at compile time, only they are linked
// Series::with_policy(&items, DeltaPacked).to_ww_bytes(&mut buf)?;

let back: Vec<IvSample> = Series::<IvSample>::from_ww_bytes(&bytes)?.iter().collect();
let owned = SeriesOwned::<IvSample>::from_ww_bytes_owned(&bytes)?; // std, into a Vec<T>
```

Deserializing validates every column in one pass (`SeriesColumns::skim`) with no allocation; `iter()` then decodes
items one at a time, stepping all columns in parallel, zero-copy for borrowed `&str`/`&[u8]` leaves. Malformed data
is always an `Error`, never a panic; the number of items is capped at `u16::MAX` (a column can take zero bits per
item — a constant run — so, unlike `Vec<T>`, the count can't be checked against the remaining bytes and has to be
bounded instead).

`Series<T>` evolves exactly as `T` does (see [evolution](../evolution/rules.md)): an appended `#[default = ..]`
field gets its own column at the end of the group, which an older reader skips and a newer reader reading older
data fills from the default; a new enum variant an older reader doesn't know fails the same way a plain decode of
that item would.

## CPU and flash on an MCU

SW-63. How the codecs of the columns are picked is a `SeriesPolicy`: `CodecPolicy::SMALLEST` tries every codec per
column and keeps the smallest, `CodecPolicy::fixed(numeric, float)` uses one for every column, both values chosen at
run time, so both link every codec and the trials. A type with `FixedCodecs` fixes the codecs at compile time and
links only them: `DeltaPacked` (delta and packed residuals, XOR floats) is one, and the `no_std` default of a
`ww_stream` batcher. All of them write the same bytes for the same codecs.

```rust
#[derive(Clone, Copy)]
struct DeltaOfDeltaPacked;

impl FixedCodecs for DeltaOfDeltaPacked {
    const NUMERIC: NumericCodec = NumericCodec::new(Predictor::DeltaOfDelta, Coder::Packed);
    const FLOAT: FloatCodec = FloatCodec::Xor;
}

Series::with_policy(&items, DeltaOfDeltaPacked).to_ww_bytes(&mut buf)?;
```

What a `Series<Timed<Ampere>>` costs (64 samples of a motor's phase current, `t` a `DeviceMicros<u32>`,
`examples_mcu/mcu_qemu` and `mcu_qemu_m4f` at `opt-level = "z"` with fat LTO, instructions of one batch counted in
QEMU with `examples_mcu/qemu_icount` and divided by `n`; flash is the bin's `.text` minus the same bin without a
`Series`):

| Cortex-M3                       | instructions per item, n = 8 / 32 / 64 | bytes, n = 8 / 32 / 64 | flash   |
|---------------------------------|----------------------------------------|------------------------|---------|
| `SMALLEST`                      | 1096 / 965 / 946                       | 20 / 49 / 87           | 21.5 KB |
| `fixed(delta, packed)`          | 355 / 300 / 290                        | 25 / 78 / 151          | 21.5 KB |
| `fixed(delta-of-delta, packed)` | 359 / 290 / 278                        | 26 / 60 / 99           | 21.5 KB |
| `DeltaPacked`                   | 354 / 302 / 294                        | 25 / 78 / 151          | 15.9 KB |

A Cortex-M4F takes about the same instructions; its flash is 18.9 KB with a `CodecPolicy` and 13.6 KB with
`DeltaPacked`. On an STM32H725 (Cortex-M7 at 540 MHz, caches on, `examples_mcu/usb_stm32h725ig`'s
`series_encode_cycles`, same profile) a batch of 32 takes 22 700 cycles with `DeltaPacked` (42 µs, 710 cycles per
item) and 59 300 with `SMALLEST`. A fixed codec is about 3x cheaper in CPU than `SMALLEST`, for 1.2x to 1.7x the
bytes on this signal (a ramp after a step: delta of delta is the better fixed codec there, delta on flatter signals).
While `t` was a `Second` (a prefix and a number type per item) the same batch took 20 % to 50 % more instructions
in QEMU, 50 % to 75 % more cycles on the H725, and 30 % (M3) to 55 % (M4F) more flash.

### From the store, column by column

SW-64. A batch kept in a `SeriesStore` can be written without rebuilding its items: `write_store_series` writes rows
of a store column by column, straight from its arrays, the same bytes as `Series::with_policy(items, policy)`.
`ww_stream`'s batchers do it on their own. It needs every enum and `Option` in the rows to keep one variant (a ww_si
prefix, a `NumericValue` type), so that no column restarts; otherwise, and for strings, byte lists and `UN` / `IN`
leaves, `write_store_series` writes nothing and says so, and the items are walked as before.

```rust
let mut wr = BufWriter::new(&mut buf);
if !write_store_series::<Ampere, _, 32>(&mut wr, &store, 0..32, &DeltaPacked)? {
    // a prefix changed in the batch: rebuild the items and write `Series::with_policy(&items, DeltaPacked)`
}
```

One FOC level batch of the motor demo (21 quantities and the clock, n = 32: 21 x `Series<T>` + `Series<Second>`,
`examples_mcu/mcu_qemu`'s `qemu-store-columns` and `usb_stm32h725ig`'s `shared_clock_cycles`):

|                               | bytes | Cortex-M3 instructions | STM32H725 cycles, opt-level `z` / `3` |
|-------------------------------|-------|------------------------|---------------------------------------|
| `DeltaPacked`, item walk      | 1 251 | 118 283                | 264 900 / 96 800                      |
| `DeltaPacked`, from the store | 1 251 | 46 665                 | 53 200 / 36 800                       |
| `RawPlain`, from the store    | 2 882 | 78 873                 | 89 700 / 76 800                       |

Before the word-wide bit writer (SW-68, see the
[Nomicon of the Series article](../articles/series.md#the-bit-writer-and-reader-a-word-at-a-time)) these were
143 463, 74 898 and 115 063 instructions.

At opt-level `z` the cycles move by up to 15 % between builds with the code layout. A firmware that writes series
in a hot path gains more from `opt-level = 3` than from any codec: the series code is
generic and compiled into the firmware's own crate, so a `[profile.release.package.shrink_wrap]` override doesn't
reach it. A firmware that stays at `z` gets the bit writer's speed back with `shrink_wrap`'s `fast-io` feature (or
`optimize-speed` from Rust 1.101 or on nightly): since SW-70 it is out of line at `z` by default, 0.75 KB smaller and up to 22 % slower, see
[Hot paths at opt-level z](code_size.md#hot-paths-at-opt-level-z).

### A prefix per item

A `ww_si` `Quantity` (`Ampere`, `Volt`, `Second`) carries its prefix and number type in every item; a
`Fixed<Ampere, Milli, i32>` has them in its type. In a `Series` both are a column of their own, a run, so a prefix
that stays the same over a batch costs about one byte per column on the wire. It costs more in CPU: the item walk
handles the prefix and the `NumericValue` of every item, and the store writer first checks that every row has the
same prefix and number type. A prefix that changes mid-batch (an autoranging device) restarts the numeric column
there, which the wire handles, but the store writer doesn't yet (SW-77): the whole column is rebuilt item by item
and walked.

One FOC level batch of the motor demo, the same numbers both ways (21 properties in mA, µ and nV as `i32`, n = 32,
two batches, `DeltaPacked`; `examples_mcu/mcu_qemu`'s `qemu-dyn-prefix-quantity`, `-fixed` and `-auto`, and
`usb_stm32h725ig`'s `dyn_prefix_cycles`, the H725 warm):

|                                    | bytes per batch | Cortex-M3 instructions, `z` / `3` | STM32H725 cycles, `z` / `3` |
|------------------------------------|-----------------|-----------------------------------|-----------------------------|
| 21 x `Series<Quantity>`, item walk | 1 214 / 1 151   | 289 200 / 117 300                 | 350 400 / 125 600           |
| 21 x `Series<Fixed>`, item walk    | 1 188 / 1 128   | 85 500 / 54 800                   | 98 600 / 66 200             |
| 21 x `Series<Quantity>`, store     | 1 214 / 1 151   | 66 300 / 34 100                   | 77 000 / 39 500             |
| 21 x `Series<Fixed>`, store        | 1 188 / 1 128   | 52 000 / 24 500                   | 64 900 / 25 200             |
| clock: `Series<Second>`, store     | 37              | 3 124 / 1 583                     | 3 552 / 1 739               |
| clock: `Series<DeviceMicros<u32>>` | 36              | 2 426 / 1 098                     | 2 978 / 1 100               |

From the store, the way `ww_stream`'s batchers and observe cells write a level, a prefix per item costs about 21
instructions per item on the M3 at `z` (14 at `3`) and 18 cycles on the H725 (21 at `3`): 19 % more cycles at `z`, 57
% more at `3`. On the M3 at `z` about 60 % of it is the check for one prefix and one number type (SW-78), the rest is
reading the number out of the `NumericValue` store. By the item walk a `Quantity` takes 2x to 3.5x the CPU of a
`Fixed`. The bytes differ by 23 to 26 per batch, 2 %. The `Fixed` bin's `.text` is 15.6 KB against 22.3 KB at `z`
(20.2 KB against 52.1 KB at `3`).

Autoranged (each value at the finest prefix whose number fits an `i16`, so a phase that wraps or a flux through
zero changes prefix mid-batch: 8 changes in 4 of the 21 columns of the first batch, 2 in 1 of the second), a column
the store can't take costs 16 700 instructions on the M3 at `z` against 3 300 for one it can, and the first batch
138 200 cycles on the H725 at `z` against 77 000 for the quantities without changes.

A device that knows its prefixes at build time (most do: an ADC's step, a PWM's resolution) gets the cheaper column
with `Fixed`, at the same bytes; a `Quantity` is for a unit chosen at run time, and an autoranging one is cheap on
the wire but, until SW-77, not in CPU.

## Collecting items on a device: `SeriesStore`

SW-62. A device that sends items in batches keeps them somewhere until the batch is written. `SeriesStore` is that
place: room for `N` items as a struct of arrays, one array per leaf field, which `#[derive_shrink_wrap(.., series)]`
implements next to `SeriesColumns`:

```rust
use shrink_wrap::series::SeriesStore;

let mut store = Timed::<Ampere>::empty_store::<32>();       // in a static or the device's state
Timed::set(&mut store, k, &Timed::new(t, Ampere::milli(i))); // item k of the batch
let item: Timed<Ampere> = Timed::get(&store, k);
```

Without padding between fields of different sizes it takes less RAM than the items themselves: `Store<32>` of
`Timed<Ampere>` is 11 bytes per item with `ww_numeric`'s `U8`, `U32`, `I32` and `F32` enabled, where
`[Timed<Ampere>; 32]` takes 16. What each kind of type keeps:

| Type                          | Store                                                                            |
|-------------------------------|----------------------------------------------------------------------------------|
| numbers, `bool`, strings      | an array of themselves                                                           |
| struct, tuple                 | the stores of its fields                                                         |
| `[X; M]`                      | `M` stores of `X`                                                                |
| `Option<X>`                   | an array of flags and the store of `X`                                           |
| enum                          | an array of discriminants (`u8`, `u16` or `u32`, `u32` with a `#[future_version]`) and the stores of every data variant's fields |
| `PhantomData`                 | nothing                                                                          |
| `ww_numeric::NumericValue`    | by hand: the variant (a byte) and one array as wide as the widest variant enabled |

An enum's store takes its discriminant plus the fields of all its variants per item, so for an enum with several
data variants it can be bigger than the enum itself (`enum { A(u32), B(u32) }`: 9 bytes against 8).
`series(no_store)` leaves the derived store out, to write a better one by hand as `NumericValue` does
(`ww_numeric/src/store.rs`): one payload array for all its variants. A type with a `#[series(opaque)]` field gets no
store, and a type with a field of such a type needs `series(no_store)` too.

### Cells: a store shared with an interrupt (`SeriesCells`)

`series(cells)` also implements `shrink_wrap::series::SeriesCells`, the same struct of arrays made of atomics
(`AtomicU8` / `AtomicU16` / `AtomicU32`, two or four `AtomicU32`s for 64 and 128 bits): one context `put`s items into
rows with relaxed stores (on a Cortex-M a plain `str`, as for a global variable), another copies rows into a
`SeriesStore` with relaxed loads (`copy_rows`), without a lock and without `static mut`. A value of two words can
mix two items only in a row copied while it is written; the code sharing the cells has to find and drop such rows
(observe cells count rows, see [observing.md](../api/observing.md#cells-sampling-from-an-interrupt)). All-zero bytes
are an empty, valid set of cells (`empty_cells()`, or `core::mem::zeroed()` in a `const fn`), which is why the trait
is `unsafe` to implement. `ww_numeric::NumericValue` has its cells by hand, `ww_si`'s quantities and `Timed` derive
them. Opt-in, because a type with a hand-written store inside would need cells of its own; strings, byte lists and
opaque fields have none.

## Size bounds

The batch has to fit into the message it goes out in, so a device needs to know how big a `Series` of `n` items can
get without writing it. `SeriesColumns::MAX_ITEM_BITS` and `MAX_FIXED_BITS` are the worst case of a type's group,
per item and per series: every column at its longest code under any `CodecPolicy` (not only the one `SMALLEST`
picks), every run changing at every item. `Series::<T>::max_bytes(n)` puts them together with the count and the
padding. Strings, byte lists and opaque fields have no bound (`None`).

The bound is far from what real data takes: 32 samples of `Timed<Volt>` at a regular clock are 21 bytes, against a
bound of 545 with the number types up to 32 bits and 893 with every one enabled (`NumericValue` is bounded by its
widest variant). It tells when a batch is safely below a budget, and a measurement (`BitCounter`) is needed only
close to it.

## Introspection and dynamic clients

A `Series<T>` in an API is in its introspection data as `Series<T>` (`ww_self::Type::Series`, with `T` as written and
`#[series(opaque)]` marked on the field), so clients without generated code read it too: `ww` and the dynamic
`wire_weaver_client` API show it as a list of items, and Python gives the items or, with `decode_columns`, one numpy
array per leaf field:

```python
cols = dev.readings.decode_columns(data)   # or a property's / stream's decode_columns
cols["value"]               # array([3298, 3299, ...], dtype=int32)
cols["status"]              # variant of each item: array(['Idle', 'Busy', ...], dtype=object)
cols["status.Fault.code"]   # masked where an item is in another variant
```

The dynamic decoder (`ww_self`, `ValueOwned::des_shrink_wrap_dyn` for the items, `des_series_columns_dyn` for the
columns) walks the type tree with the same rules as the code the derive generates: column by column, restarting a
column after an earlier enum or `Option` tag changes, a newer or older `T` read as the typed decoder reads it (missing
fields take their declared `#[default]`). The bytes come from a device, so they are untrusted: malformed data is an
error, and since an item can take no bits at all (a constant column), the work and the memory are bounded by the
decode's budget (values and bytes of memory, a repeated value counted once per copy), not by the length of the bytes.

## What's not here yet

`Series<T>` as a type you put in a property, method or stream today works end to end (phase 1 of SW-60). Picking the
batch for you — a stream or property opting into batching (no size in the API itself: the device owns the batch
size through its own config, default 32, and the host picks latency) sending `Series<Timed<T>>` batches instead of
one item per event — is phase 2, API-41, done: see [Batching](../api/batching.md) for the how-to, sealing, budgets
and the measured numbers. The numbers above (default batch 32, integer storage, autoranging cost) come from the
simulator built for that work and hold for `Series` used by hand too.
