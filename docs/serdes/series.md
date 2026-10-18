---
level: advanced
---

# `Series<T>`: compressed sequences of any derived type

SW-60. `Series<'i, T>` is a list of `T` written column by column instead of item by item: one column per leaf
field, each compressed on its own. For samples that change slowly (sensor readings, counters, timestamps) it is
3 to 8 times smaller than a plain `Vec<T>`, without giving up evolution, zero-copy reading, or `no_std`/no-alloc.

A `Series` is a type you reach for explicitly today, in a property, method argument/return or stream item, the same
way you'd use `Vec<T>` or [`Delta<T>`](special_types.md#delta-deltaofdelta-and-xorfloat-compressed-sequences).
Batching many samples into one `Series` automatically (streams and observe properties sending a `Series<Timed<T>>`
batch instead of one item at a time) is planned as API-41, see the [feature tracker](../features.md); nothing
below needs it.

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
- **Floats** get the same treatment as [`XorFloat`](special_types.md#delta-deltaofdelta-and-xorfloat-compressed-sequences)
  (plain or XOR), but integers with a predictor compress markedly better than floats — see
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

A steady `f32` voltage (100 values) is 136 B as `XorFloat`; the same signal stored as `U16` millivolts with a
`ww_si` prefix is 45 B. Noisy: 277 B vs 82 B. The prefix sets the resolution, and noise below that resolution costs
nothing — so a device sending `ww_si` quantities (which carry a prefix and an integer storage tag by construction)
gets this for free, while a bare `f32` pays for every bit of mantissa the ADC's noise floor touches.

This has a flip side: *unbatched*, a `ww_si` quantity (prefix + storage tag + the integer) is bigger than a bare
`f32` in a single plain update — measured on a motor control demo, moving one observe property from `f32` to
`ww_si` went from 96 to 117 kB/s. Quantities pay off once they're batched into a `Series`; sending one at a time
gets you the worse of both.

## Time: `Timed<T>`

`ww_si::Timed<T> { t: Second, value: T }` ([DER-14](derive_reference.md#generics) generic struct with
`series`) is not a special case of `Series` — it gets a `t` column like any other field, and a regular sampling
rate costs about nothing (delta + constant: `t0 + period`). Use `Series<Timed<T>>` wherever a batch needs its own
timestamps; where a stream already carries time for the whole batch, a plain `Series<T>` works just as well.

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

What a `Series<Timed<Ampere>>` costs (64 samples of a motor's phase current, `examples_mcu/mcu_qemu` and
`mcu_qemu_m4f`, instructions counted in QEMU with `examples_mcu/qemu_icount`; flash is the bin's `.text` minus the
same bin without a `Series`):

| Cortex-M3                       | instructions per item, n = 8 / 32 / 64 | bytes, n = 8 / 32 / 64 | flash   |
|---------------------------------|----------------------------------------|------------------------|---------|
| `SMALLEST`                      | 1112 / 969 / 950                       | 21 / 50 / 89           | 25.4 KB |
| `fixed(delta, packed)`          | 409 / 341 / 332                        | 27 / 79 / 153          | 25.4 KB |
| `fixed(delta-of-delta, packed)` | 411 / 334 / 321                        | 28 / 61 / 100          | 25.4 KB |
| `DeltaPacked`                   | 406 / 341 / 333                        | 27 / 79 / 153          | 15.0 KB |

A Cortex-M4F takes about the same instructions; its flash is 17.6 KB with a `CodecPolicy` and 9.8 KB with
`DeltaPacked`. On an STM32H725 (Cortex-M7 at 540 MHz, caches on, `examples_mcu/usb_stm32h725ig`'s
`series_encode_cycles`) a batch of 32 takes 25 200 cycles with `DeltaPacked` (47 µs, 790 cycles per item) and
60 800 with `SMALLEST`. A fixed codec is about 3x cheaper in CPU than `SMALLEST`, for 1.2x to 1.7x the bytes on this signal
(a ramp after a step: delta of delta is the better fixed codec there, delta on flatter signals).

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
`Timed<Ampere>` is 12 bytes per item with `ww_numeric`'s `U8`, `U32`, `I32` and `F32` enabled, where
`[Timed<Ampere>; 32]` takes 24. What each kind of type keeps:

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

## Size bounds

The batch has to fit into the message it goes out in, so a device needs to know how big a `Series` of `n` items can
get without writing it. `SeriesColumns::MAX_ITEM_BITS` and `MAX_FIXED_BITS` are the worst case of a type's group,
per item and per series: every column at its longest code under any `CodecPolicy` (not only the one `SMALLEST`
picks), every run changing at every item. `Series::<T>::max_bytes(n)` puts them together with the count and the
padding. Strings, byte lists and opaque fields have no bound (`None`).

The bound is far from what real data takes: 32 samples of `Timed<Volt>` at a regular clock are 22 bytes, against a
bound of 570 with the number types up to 32 bits and 1272 with every one enabled (`NumericValue` is bounded by its
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
one item per event — is phase 2, tracked as API-41 in the [feature tracker](../features.md), and not implemented
yet. The numbers above (default batch 32, integer storage, autoranging cost) come from a simulator built for that
work and already hold for `Series` used by hand.
