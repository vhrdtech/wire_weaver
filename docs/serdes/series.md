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
(`Timed<T>`, [DER-14](derive_reference.md#type-parameters)): structs (named fields), unit-only enums and enums with
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
[wire format internals](wire_format.md#series-t); this page is about when and why to reach for it.

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
(constant delta), ±3 µs of jitter adds about 6 bits/item. [`Timed<T>`](#time-timed-t) is just another struct field,
not a special case.

Bytes per item by batch size, steady signal (plain `final_structure` / `Series`):

| n | 1 | 2 | 4 | 8 | 16 | 32 | 64 | 128 | 256 |
|---|---|---|---|---|---|---|---|---|---|
| bytes/item | 9.0 / 10.0 | 8.5 / 6.0 | 8.3 / 3.5 | 8.1 / 2.1 | 8.1 / 1.6 | 8.0 / 1.2 | 8.0 / 1.1 | 8.0 / 1.0 | 8.0 / 0.9 |

A single item costs one byte more than plain (the column headers have to be paid once); from 2 items on `Series` is
already smaller, and most of the gain is there by 32 items — the default batch size the implementation (and the
planned API-41 batching) uses.

The [showcase](showcase.md#compressed-columns-series-t) has the exact bytes of a 4-item batch, checked against the
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

`ww_si::Timed<T> { t: Second, value: T }` ([DER-14](derive_reference.md#type-parameters) generic struct with
`series`) is not a special case of `Series` — it gets a `t` column like any other field, and a regular sampling
rate costs about nothing (delta + constant: `t0 + period`). Use `Series<Timed<T>>` wherever a batch needs its own
timestamps; where a stream already carries time for the whole batch, a plain `Series<T>` works just as well.

## Using it

```rust
use shrink_wrap::series::{Series, SeriesOwned};

let items: Vec<IvSample> = /* ... */;
let bytes = Series::new(&items).to_ww_bytes_owned()?;       // picks the smallest codec per column
// or, on a small MCU that wants to skip trying every codec:
// Series::with_policy(&items, CodecPolicy::fixed(..)).to_ww_bytes(&mut buf)?;

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

## What's not here yet

`Series<T>` as a type you put in a property, method or stream today works end to end (phase 1 of SW-60). Picking the
batch for you — a stream declared `batched(max_n = .., max_ms = ..)` sending `Series<Timed<T>>` batches instead of
one item per event, and the matching batched observe property updates — is phase 2, tracked as API-41 in the
[feature tracker](../features.md), and not implemented yet. The numbers above (default batch 32,
integer storage, autoranging cost) come from a simulator built for that work and already hold for `Series` used by
hand.
