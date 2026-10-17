---
level: nomicon
---

# Wire format internals

How ShrinkWrap lays values out in bytes, and the reader and writer that do it. You don't need any of this to use
[`#[derive_shrink_wrap]`](derive.md); it explains _why_ the derive macro's directives (evolvable by default,
`final_structure` / `self_describing` / `sized`) exist and what each one trades. It is also the reference for a
hand-written `SerializeShrinkWrap` impl or a reader in another language (see [ShrinkWrap in C](../c.md)).

The [showcase](showcase.md) has worked byte-by-byte examples of most of what is described here.

## The FIFO of lengths

The core idea: lengths are kept at the _back_ of the buffer, written in reverse while values are written forward
from the front. This allows serialization in a single pass with no extra copying.

Consider two strings of arbitrary length in one buffer. To get both back, their lengths have to be stored too. Most
formats write a value's length right before the value:

```text
l1 abc l2 qwerty
```

That's fine for a string, whose length is known before its first byte is written. It falls apart for a nested
struct or a `Vec` of them. Either you make a full pass first just to compute the serialized length (and get it
exactly right), or you write a placeholder, serialize the value and patch the placeholder. But how many bytes should
the placeholder take, if small lengths are to take little space?

A fixed-width length field has the same problem in another shape. One byte caps a value at 255 bytes; four bytes
waste space on the overwhelming majority of values, which are small.

ShrinkWrap never writes a length where the value starts:

1. When a value's size can't be known up front, a slot is reserved at the back of the buffer.
2. The value is written normally from the front.
3. The slot is filled in with the now-known size, as a variable-length `UNib32` (see the
   [showcase](showcase.md#unib32-variable-length-numbers-by-the-nibble)).

Several slots stack up back to front, forming a FIFO that a reader consumes in the same order while walking the
buffer forward. The [showcase](showcase.md#the-fifo-of-lengths-why-strings-can-come-before-their-length) has a
worked byte-by-byte example.

## `BufWriter` and `BufReader`

The two core types are
[`BufWriter`](https://github.com/vhrdtech/wire_weaver/blob/master/shrink_wrap/shrink_wrap/src/buf_writer.rs) and
[`BufReader`](https://github.com/vhrdtech/wire_weaver/blob/master/shrink_wrap/shrink_wrap/src/buf_reader.rs). Both
work on plain byte slices that may start at any address: the bit, nibble and byte alignment below is tracked as a
bit and byte cursor, not imposed by the platform.

### `BufWriter`

```rust
use wire_weaver::shrink_wrap::prelude::*;

fn simple_wr() {
    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap();
    wr.write_u8(0xaa).unwrap();
    let bytes = wr.finish().unwrap();
    assert_eq!(bytes, &[0x80, 0xaa]);
}
```

`BufWriter` is created from a mutable byte slice, which does not need to be zeroed first (zeroing a large scratch
buffer isn't free on `no_std`). It keeps one index advancing from the front for the data, and one retreating from
the back for the FIFO of lengths.

- `write_bool` consumes a single bit, here the top bit of the first byte.
- `write_u8` aligns to the next byte first, which is why the `true` sits alone in `0x80`.
- Bit-aligned writes (`write_bool`, the `write_un*` family behind `U1`..`U64` / `I2`..`I64`) pack back to back. A
  new byte starts only when something byte-aligned (`write_u8`, `write_u16`, a `&str`, ...) or an explicit
  `align_nibble` / `align_byte` comes along.
- `finish()` encodes the lengths still pending as reversed `UNib32`s at the back and returns the written bytes.

When nothing pushed a length (no string, list or nested `Unsized` value), `finish_no_lens()` returns the same bytes
without linking the code that encodes the FIFO, which matters on a CPU with a few kilobytes of flash (see
[code size](code_size.md)). `wr.finish_for::<T>()` picks one of the two at compile time by `T::PUSHES_LEN`, which
`#[derive_shrink_wrap]` computes from the fields; `to_ww_bytes` does that for you. A hand-written impl may set
`const PUSHES_LEN: bool = false;` when no path through its `ser_shrink_wrap` pushes a length. The default is `true`
for everything that is not `Sized`, which is always correct.

### `BufReader`

```rust
use wire_weaver::shrink_wrap::prelude::*;

fn simple_rd() {
    let bytes = [0x80u8, 0xaa];
    let mut rd = BufReader::new(&bytes);
    assert_eq!(rd.read_bool().unwrap(), true);
    assert_eq!(rd.read_u8().unwrap(), 0xaa);
}
```

`BufReader` mirrors `BufWriter`: it walks the same slice forward with matching read methods, and can `split()` off a
sub-reader bounded to exactly the number of bytes a length slot says a value takes.

`split()` is also what makes evolution work. A reader bounded to a value's byte range never reads past it, so:

- code that doesn't know a newer field skips whatever trailing bytes it doesn't understand;
- code reading data that lacks a field it knows runs out of bytes and falls back to that field's default.

See the [evolution rules](../evolution/rules.md) for which changes stay wire-compatible.

### Nesting depth

A type that contains itself (`Box<T>`, `RefBox<T>`, `Vec<T>` or `RefVec<T>` of a type with `T` in it, like a tree
or the introspection `Type` / `Value`) is read by a recursion as deep as the data: 38 KB of 10 000 nested lists
would overflow the stack of whoever reads them. So `BufReader` counts how deep it is, and a value nested deeper than
it allows is `Error::NestingTooDeep`:

- `read::<T>()` / `read_owned::<T>()` of a value that is neither `Sized` nor `SelfDescribing` goes one level deeper,
  and back up when it is done; `split()` gives a reader one level deeper. Every field, list element and boxed value
  is read that way, so every way a type can contain itself takes a level per step.
- Values read later keep the depth they were read at: a `RefBox`, a `RefVec`, the slots of a `sparse` struct and the
  elements of an `EitherAnyVec` are read with a reader as deep as the one they came from.
- `BufReader::new` allows `BufReader::DEFAULT_MAX_DEPTH` = **128** levels, `from_ww_bytes` and
  `from_ww_bytes_owned` too. `BufReader::new(buf).with_max_depth(n)` sets another limit, `depth_left()` tells what
  is left.

128 is far more than real data needs: the introspection data of every API in this repository's tests reads within 20. A
level costs about 3.5 KiB of stack in a debug build (far less in release), so 128 levels of the introspection
`Type` take about 450 KiB, well within the 2 MiB of a spawned thread. Lower the limit on a small stack (an MCU) that
decodes a recursive type. A hand-written `des_shrink_wrap` that calls `T::des_shrink_wrap` of a nested value directly,
without `read`, takes no level for it: read nested values with `read` there too.

### `BufWriterOwned`

`BufWriterOwned` writes the same bits into a growable `Vec<u8>` instead of a fixed slice (`std` only), and has no
`u16::MAX` limit per value, which plain `BufWriter` currently has.

## Alignment

`bool`, `U1`..`U64` and `I2`..`I64` are bit-aligned, `Nibble` and `UNib32` nibble-aligned, everything else
byte-aligned. Byte arrays, strings and `Unsized` values are all byte-aligned, to keep the code and the computations
small. Unused bits are set to zero and can be reclaimed when evolving a type:

```rust
fn version1() {
    let mut buf = [0u8; 8];
    let mut wr = BufWriter::new(&mut buf[..]);
    wr.write_bool(true).unwrap();
    wr.write_u8(0xAA).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    assert_eq!(bytes, &[0x80, 0xAA]);
}
```

A later version, while the older one is still in use, adds an `Option<U6>` in the free bits:

```rust
fn version1_1() {
    let mut buf = [0u8; 8];
    let mut wr = BufWriter::new(&mut buf[..]);
    wr.write_bool(true).unwrap();
    wr.write(&Some(U6::new(5).unwrap())).unwrap();
    wr.write_u8(0xAA).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    assert_eq!(bytes, &[0xC5, 0xAA]);
}
```

Older code reads the new data and skips the bits, newer code reads old data and gets `None` (the zero bits). The
size didn't even change.

## Traits and `ElementSize`

Every serializable type implements
[`SerializeShrinkWrap` and `DeserializeShrinkWrap`](https://github.com/vhrdtech/wire_weaver/blob/master/shrink_wrap/shrink_wrap/src/traits.rs).
Both work on borrowed data, no allocator required. Types that also support `std` implement the `..Owned`
counterparts, `SerializeShrinkWrapOwned` / `DeserializeShrinkWrapOwned`, which write into a `BufWriterOwned`.
`#[derive_shrink_wrap(..)]` generates one pair per representation you ask for (see
[borrowed, owned, or both?](derive.md#borrowed-owned-or-both)).

Each trait carries `ELEMENT_SIZE: ElementSize`, the one piece of information the FIFO of lengths is built on. It
tells a _parent_ type, at compile time, whether a field needs a length slot at all:

| `ElementSize`           | Meaning                                                                                                                                      |
|-------------------------|----------------------------------------------------------------------------------------------------------------------------------------------|
| `Unsized`               | Size isn't known up front; a parent reserves a slot for it. The default for structs and enums: fully evolvable.                             |
| `UnsizedFinalStructure` | Also size-unknown, but flattened onto the parent instead of getting its own slot. `Vec<T>`, `String`, `&str`, arrays and tuples are this.  |
| `SelfDescribing`        | No size is stored; the reader tells where the value ends from the encoding itself (`UNib32`'s continuation bits, `Option`'s flag, ...).     |
| `Sized { size_bits }`   | Size is fixed and known at compile time; nothing is stored for it.                                                                           |

How a struct's or enum's own `ELEMENT_SIZE` is computed from its fields:

- `ElementSize::add` combines fields. It is deliberately "sticky" upward: `Sized` + `SelfDescribing` =
  `SelfDescribing`, anything + `Unsized` = `Unsized`, and `UnsizedFinalStructure` wins over everything. A type can
  never claim a stronger guarantee than its fields give; a compile-time assertion checks it.
- `ElementSize::one_of` combines the variants of an enum, of which only one is on the wire. Variants of the same
  `Sized` size keep it, `Sized` variants of different sizes give `SelfDescribing` (the discriminant tells the
  length), anything else follows `add`.
- The discriminant is added on top: `Sized` for every `ww_repr` but `unib32`.
- `size_bits` counts the bits of the value itself, without alignment padding: `struct { a: bool, b: u8 }` is 9 bits
  and takes 2 bytes.

[Size assumptions](size_assumptions.md) covers how to pick one of these for your own types.

## `write` / `read` vs `ser_shrink_wrap` / `des_shrink_wrap`

There are two ways to put a value into a `BufWriter`. They are **not** interchangeable: each has a read-side
counterpart, and mixing them up gives a reader misaligned with what was written.

- **`value.ser_shrink_wrap(&mut wr)`** writes exactly the value's own bytes, with no length slot. The caller
  already knows (or doesn't need) its extent. Read back with **`T::des_shrink_wrap(&mut rd)`**.
- **`wr.write(&value)`** first reserves a slot, but only if `T::ELEMENT_SIZE` is `Unsized`, then calls
  `ser_shrink_wrap` and fills the slot in. Read back with **`rd.read::<T>()`**, which reads the length and
  `split()`s a bounded sub-reader first, again only for `Unsized` types.

`write` / `read` is what makes a value's size discoverable to whatever is _around_ it: use it for a value that is a
field of another one (generated code does exactly that for every field). `ser_shrink_wrap` / `des_shrink_wrap` is
right when nothing needs to skip over the value from outside, most commonly the _top-level_ value of a message,
whose extent is the buffer's own length:

```rust
fn to_ww_bytes<'i>(&self, buf: &'i mut [u8]) -> Result<&'i [u8], Error> {
    let mut wr = BufWriter::new(buf);
    self.ser_shrink_wrap(&mut wr)?; // not `wr.write(self)`: no point sizing the whole message
    wr.finish_and_take()
}
```

## `to_ww_bytes` / `from_ww_bytes`

The traits have default methods that wrap that boilerplate for the top-level value:

```rust
let mut buf = [0u8; 64];
let bytes: &[u8] = my_value.to_ww_bytes(&mut buf)?;      // SerializeShrinkWrap::to_ww_bytes
let value = MyType::from_ww_bytes(bytes)?;               // DeserializeShrinkWrap::from_ww_bytes

// std: no scratch buffer to manage, and no u16::MAX limit per value
let bytes: Vec<u8> = my_value.to_ww_bytes_owned()?;      // SerializeShrinkWrapOwned::to_ww_bytes_owned
let value = MyType::from_ww_bytes_owned(&bytes)?;        // DeserializeShrinkWrapOwned::from_ww_bytes_owned
```

These are what every example on this site calls. `BufWriter` / `BufReader` and `write` / `read` only matter for a
hand-written type, a streaming API on top of ShrinkWrap (see the
[builder pattern](showcase.md#patching-a-discriminant-after-youve-already-started-writing-its-payload)), or
anything else that needs more control than one call.

## Encodings of the special types

How the types of [special types](special_types.md) look on the wire. These codes are part of the wire format and
stay as they are.

### `TailSize<N>`

A `UVlq32` right-justified in `N` bytes (1 to 5, 5 by default), padded with empty `0x80` groups in front, so any
`UVlq32` reader decodes it. `N` is the capacity: 127 bytes for `N = 1`, 16 KB for 2, 2 MB for 3, 256 MB for 4,
4 GB for 5; serialization fails with `Error::LenTooLong` beyond it.

The value is the number of bytes from the end of the field to the end of the enclosing value, the value's own
reverse lengths included. The derive macro generates, around the fields after the slot, what you would write by
hand with `BufWriter::reserve_tail_size()` and `backfill_tail_size()`:

1. reserve the slot and write the rest of the fields;
2. encode the reverse lengths those fields pushed, so they land inside the value's range, where they would have
   gone anyway;
3. align to a byte and fill the slot in.

On read: read the slot, `split()` the reader to that many bytes, read the rest from the split. The
[showcase](showcase.md#bounding-a-value-from-the-inside-with-tailsize) has the bytes.

### `Delta`, `DeltaOfDelta` and `XorFloat`

Around the elements the layout is that of `Vec<T>`: `UnsizedFinalStructure`, the element count as a reverse length
at the back of the enclosing value, nothing else. Inside:

- The first element is written at the full width of `T` through the bit cursor (most significant bit first, so a
  `u32` first element reads as big-endian in a hex dump, unlike a `u32` field). Every further element is a prefix
  code against the previous one. There is no alignment in between or after: a `bool` written next shares the byte,
  a `u8` aligns itself as always.
- **`Delta<T>`**: the difference to the previous element, wrapped at the width `W` of `T` (so `u8` 0 after 255 is
  `+1`), zigzag-encoded to `z` and written as:

    | Prefix   | Then    | For            |
    |----------|---------|----------------|
    | `0`      | nothing | `z = 0`        |
    | `10`     | 7 bits  | `z < 128`      |
    | `110`    | 9 bits  | `z < 512`      |
    | `1110`   | 12 bits | `z < 4096`     |
    | `11110`  | 20 bits | `z < 2^20`     |
    | `111110` | 32 bits | `z < 2^32`     |

    Buckets at or above `W` are left out, and the last bucket is `W` bits after a `1` for every bucket and no `0`:
    `11` + 8 bits for `u8`/`i8`, `1111` + 16 bits for 16-bit types, `11111` + 32 bits for 32-bit, `111111` + 64
    bits for 64-bit types. A repeated value costs 1 bit, a step within `-64..=63` costs 9.

- **`DeltaOfDelta<T>`**: the same code applied to the change of the difference (the previous difference starts at
  0). This is the Gorilla timestamp code: a value that grows by the same step as the one before costs 1 bit.
- **`XorFloat<T>`**: the XOR of the bit patterns of the value and the previous one.
    - `0` when it is zero (a repeated value);
    - `10` and the bits inside the window of the previous explicitly coded XOR (its leading and trailing zero
      counts) when the new XOR fits in it;
    - otherwise `11`, the leading zero count (5 bits for `f32`, 6 for `f64`), the number of significant bits minus
      one (5 / 6 bits) and those bits, which become the new window.

    Pure bit operations: NaN payloads, `-0.0` and infinities round-trip exactly.

The [showcase](showcase.md#compressed-sequences-delta-deltaofdelta-and-xorfloat) has the bytes.

### `OptionAnyVec`

Layout: `|presence header|present values in slot order|reverse lengths|`. The values are written exactly as
`BufWriter::write` writes them anywhere else (an `Unsized` value with its size, lists and strings with their length
at the back), one after another without padding between them: a `bool` takes one bit, a `u8` aligns itself to a
byte as always.

The header starts with a mode nibble. The writer knows the present set before writing anything and picks the mode
with the fewest bits for it (on a tie, the first one in this table):

| Nibble    | Mode       | Then                                                                                                                                         |
|-----------|------------|----------------------------------------------------------------------------------------------------------------------------------------------|
| `0`       | all        | `n: UNib32`: slots `0..n` are present                                                                                                        |
| `1`       | dense      | `len: UNib32` (last present slot + 1), then `len` bits, one per slot, MSB first                                                              |
| `2`       | two-level  | `groups: UNib32` (of 8 slots, up to the last present one), a bit per group, then 8 bits for each group whose bit is set, slot `8g` in the MSB |
| `3`       | index list | `count: UNib32`, then `count` `UNib32`: the first present slot, then for each next one `slot - previous - 1`                                 |
| `4`..`15` | reserved   | a reader fails with `Error::EnumFutureVersionOrMalformedData`                                                                                |

No present slot at all is written as no bytes (a reader also takes `0 0` and `3 0` as empty). The mode nibble costs
nothing extra in practice: the `UNib32` after it is nibble-aligned anyway. The reserved codes leave room for slots
with a third state (absent / value / error, SW-56) at no cost to the two-state vec.

Header bits from a nibble boundary:

| Mode       | Bits                                                   |
|------------|--------------------------------------------------------|
| all        | `4 + 4·nibbles(n)`                                     |
| dense      | `4 + 4·nibbles(len) + len`                             |
| two-level  | `4 + 4·nibbles(groups) + groups + 8·(non-empty groups)` |
| index list | `4 + 4·(nibbles(count) + Σ nibbles(delta))`            |

Dense wins for clustered sets up to a few dozen slots, the index list for a few slots among many, two-level for a
few dense clusters far apart. The [showcase](showcase.md#sparse-updates-with-optionanyvec) has the numbers.

### `Series<T>`

SW-60. What [`Series<T>` is for and when to reach for it](series.md) is its own page; this is the bit-level layout.
All bit-level codes below write most significant bit first through the bit cursor, with no alignment unless stated;
`nib(x)` is the [`UNib32`](showcase.md#unib32-variable-length-numbers-by-the-nibble) code written through the bit
cursor without nibble alignment.

```text
n:          UNib32 (nibble-aligned)     number of items; 0: nothing else follows
group(T):   the columns of T over all n items
padding:    zero bits to the next byte
```

`Series` is `ElementSize::Unsized`: as a field it gets a length slot in the parent's FIFO like any other evolvable
value, and nothing *inside* a `Series` uses the FIFO - a lazy reader walks the columns in parallel, which would
consume FIFO entries out of order. At the top of a message or stream item its extent is the buffer's own.

**Groups.** The group of a type over a subsequence of items (all of them at the top, fewer inside an enum variant
or an `Option`) is written by the type's own rule, recursively; an empty subsequence writes nothing (its size is
already known to the reader from the tag runs above it):

| Type | Group |
|---|---|
| integers (`u8`..`u64`, `i8`..`i64`, `U1`..`U64`, `I2`..`I64`, `Nibble`, `UNib32`, `UVlq32`, `UVlq64`) | a numeric column at the type's value width |
| `u128`, `i128` | a numeric column, predictor raw and coder plain only (other codes malformed) |
| `f32`, `f64` | a float column |
| `bool` | a two-valued run |
| unit-only enum, or enum with data | a tag run (always the general form, even with 2 variants: a later version may add a third), then, for each variant that has items here, the variant's fields as a struct group over just those items |
| `Option<X>` | the `{ None, Some(X) }` enum: a two-valued tag run, then `group(X)` over the `Some` items |
| struct, `final_structure` / `self_describing` / `sized` | the fields' groups in declaration order |
| struct, evolvable (`Unsized`, the default) | outermost: `fields: nib` (how many fields this writer's `T` has), then the fields' groups; nested inside another group: `len: nib` (bits of the group after `len`), then the fields' groups |
| tuple, `[X; N]` | like a `final_structure` struct of the elements |
| `&str`/`String`, `&[u8]`, `RefVec<u8>`/`Vec<u8>`, or any type marked `#[series(opaque)]` | an opaque run of the bytes (`to_ww_bytes` for `#[series(opaque)]`) |

Columns appear in the order the groups above produce them: depth first, declaration order, an enum's tag run
before its variants' columns - the order a reader needs to find every column's start in one forward pass.

**Numeric column:**

```text
predictor:  2 bits   00 raw, 01 delta, 10 delta of delta, 11 escape (reserved in v1: malformed)
coder:      2 bits   00 prefix, 01 packed, 10 constant, 11 plain
values:     m values of width W
```

The values split into segments at restarts (below). With predictor delta or delta of delta, a segment's first value
is written at the full width `W`; every other value is a **residual**: the zigzag of the difference to the
previous value (delta) or of the change of that difference (delta of delta, the running difference starting at 0
at each segment start), wrapped at `W` exactly as [`Delta`](#delta-deltaofdelta-and-xorfloat) does. With predictor
raw there are no segment starts; every value is a residual (its zigzag for signed types, itself for unsigned ones).

Coders, for the residuals:

- **prefix** - each residual in the [`Delta`](#delta-deltaofdelta-and-xorfloat) code at width `W`.
- **packed** - the residuals of a segment in blocks of 8 (the last block of a segment shorter, its length known
  from `m` and the restarts): `width` in `bits(W)` bits (4 bits for `W = 8`, 5 for 16, 6 for 32, 7 for 64 - enough
  for `0..=W`), then each residual of the block at exactly `width` bits. A block of all zeros costs only its width
  field.
- **constant** - the first residual of the column in the prefix code; every later residual equals it and is not
  written. Delta + constant is `t0 + period` (a regular clock, a counter); raw + constant is a plain constant
  column.
- **plain** - each residual at the full width `W`.

The writer may try any subset of predictor x coder combinations (or a fixed one via `CodecPolicy`) and keeps the
smallest encoding; the reader follows whatever the 4-bit header says. Predictor `11` is reserved for a later,
extended column mode (an escape, so existing codes keep their meaning and cost); a v1 reader fails on it with
`Error::MalformedSeries`.

**Float column:**

```text
codec:      4 bits   0000 plain (each value at full width), 0001 XOR (the XorFloat code), others reserved
```

XOR: the first value of a segment at full width, then the [`XorFloat`](#delta-deltaofdelta-and-xorfloat) code
against the previous value; a segment start also resets the leading/trailing zero window.

**Run** (discrete leaves - enum tags, `bool`, `Option` presence, opaque bytes):

```text
first:      value of the first item
repeat:     1, gap: nib, value      a change: gap = items since the previous change (or the start) minus 1;
                                    value left out when two-valued (a change just flips it)
end:        0
```

An enum's discriminant is written as `nib` (per its `ww_repr`); a `bool` as 1 bit; an opaque value as `len: nib`
bytes, zero bits to the next byte, then the bytes. A change landing at or past the subsequence's end is malformed.

**Restarts.** A numeric or float column restarts (full value, codec state reset, packed block ended) at an item
where: the item enters this enum variant (or `Some`) from another one, so the column only ever holds that variant's
own items; an **enum-typed field declared earlier in the same struct changed** (`Volt { prefix, value }` restarts
`value` when `prefix` changes, so 480 µA followed by 120 mA is a full value, not a delta of -360); or the enclosing
group itself restarts (it propagates down). `bool`, string and opaque runs never restart anything - a flag toggling
every other item would otherwise cost a full value each time. "Declared earlier" keeps one forward pass enough to
decode, and keeps evolution working: a field appended later can never restart a column an older reader already
knows how to read.

**Evolution.** `Series<T>` follows `T`'s own evolution rules, same as `Vec<T>` does: an appended `#[default = ..]`
field's columns go at the end of its struct's group, which an older reader skips (nested: via the group length;
outermost: the `Series` simply ends after the columns it knows, filling the field's default into every item for a
newer reader reading older data). A `final_structure` / `self_describing` / `sized` struct has no count or length
inside a `Series` either, exactly as in plain form. A new enum variant an older reader doesn't know fails with
`Error::EnumFutureVersionOrMalformedData`, as a plain decode of that item would.

The [showcase](showcase.md#compressed-columns-seriest) has a 4-item example worked out byte by byte, including a
column restart.
