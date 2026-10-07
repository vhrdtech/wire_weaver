# Shrink Wrap

<p align="center">
<img src="../assets/logo-shrinkwrap-256.png" alt="logo"/>
</p>

All serializing and deserializing operations go through a wire format called `shrink_wrap`. It targets both
microcontroller and host usage, and lives in its own crate (`shrink_wrap`), with no dependency on the rest of
`wire_weaver` - it's usable completely standalone if all you need is a serialization format.

Features:

- 1-bit, 4-bit and 1-byte alignment
- Support for all the types listed on the [types page](../types.md)
- `no_std` without allocator support (even with types like `String` and `Vec`, for both reading and writing)
- `std` support (standard `Vec` and `String` are used)
- Zero-copy deserialization
- Self-referential types (see the [showcase](showcase.md#self-referential-types-with-refbox))
- Built-in mechanism for backwards and forwards compatibility

Most of this is handled for you by [`#[derive_shrink_wrap(..)]`](derive.md), so understanding how the serdes
system works manually is optional. This page explains the mechanics anyway, because they explain _why_ the derive
macro's directives (`final_structure` / `self_describing` / `sized`, evolvable-by-default types, ...) exist and what
trade-off each one makes. Feel free to skip ahead to [derive](derive.md) and come back later.

## High-level overview

The core idea is a FIFO of sizes kept at the _back_ of the buffer, written in reverse as values are serialized
forward from the front. This allows serialization in a single pass with no extra copying, which matters more than
it might first appear.

Consider serializing two strings of arbitrary length into a byte buffer of known length. To get both strings back
on the other end, their lengths have to be encoded somewhere too. Most formats write a value's length immediately
before the value itself:

```text
l1 abc l2 qwerty
```

That's fine for a string, whose length you already know before writing a single byte of it. It falls apart once the
"value" is a nested struct, or a `Vec` of them: you'd need to either make a full pass over the data first just to
compute its serialized length (and get it _exactly_ right), or write a placeholder length, serialize the value, and
come back to patch the placeholder in - except you don't yet know how many bytes that placeholder itself should be,
if you want a variable-length encoding to avoid wasting space on small values. Picking a fixed-width length field
avoids that problem but reintroduces it in a different shape: too small (say, one byte, capping objects at 255
bytes) and it's useless for anything but tiny messages; too large and you're wasting space on the overwhelming
majority of values, which are small.

`shrink_wrap` sidesteps all of this: a length is never written where the value starts. Instead, whenever a value's
final size can't be known up front, a placeholder slot is reserved at the _back_ of the buffer, the value is
written normally from the front, and only then is the placeholder filled in with the now-known size - as a
variable-length `UNib32` (see the [showcase](showcase.md#unib32-variable-length-numbers-by-the-nibble)), reusing
the same "small values are common" idea. Multiple such slots stack up back-to-front, forming a FIFO that a reader
consumes in the same reverse order while walking the buffer forward. See the
[showcase](showcase.md#the-fifo-of-lengths-why-strings-can-come-before-their-length) for a worked byte-by-byte
example.

The two core types implementing this are [`BufWriter`](https://github.com/vhrdtech/wire_weaver/blob/master/shrink_wrap/shrink_wrap/src/buf_writer.rs)
and [`BufReader`](https://github.com/vhrdtech/wire_weaver/blob/master/shrink_wrap/shrink_wrap/src/buf_reader.rs). Both operate on plain byte
slices with no alignment requirements (the slice itself can start at any address) - all the bit/nibble/byte
alignment described below is tracked internally as a bit and byte cursor, not imposed by the platform.

### BufWriter

`BufWriter` is created from a mutable byte slice, which does not need to be zeroed first (a small win on `no_std`
where zeroing a large scratch buffer isn't free). It tracks its position with a couple of indices into that slice -
one advancing from the front for the data being written, one retreating from the back for the FIFO of sizes.

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

`write_bool` only ever consumes a single bit (here, the top bit of the first byte); `write_u8` re-aligns to the next
byte boundary before writing, which is why the `true` above ends up alone in `0x80` instead of sharing a byte with
`0xaa`. Bit-aligned writes (`write_bool`, the `write_un*` family behind `U1`..`U64`/`I2`..`I64`) pack contiguously
with each other and only force a new byte once something byte-aligned (`write_u8`, `write_u16`, a `&str`, ...) or
explicitly aligned (`align_nibble`/`align_byte`) comes along. `finish()` closes out the FIFO of sizes (encoding any
that are still pending as reversed `UNib32`s at the back) and returns the written prefix as a `&[u8]`.

### BufReader

`BufReader` mirrors `BufWriter`: it walks the same slice forward with matching bit/nibble/byte-aligned read methods,
and knows how to `split()` off a sub-reader bounded to exactly the number of bytes a size slot said an `Unsized`
value should occupy.

```rust
use wire_weaver::shrink_wrap::prelude::*;

fn simple_rd() {
    let bytes = [0x80u8, 0xaa];
    let mut rd = BufReader::new(&bytes);
    assert_eq!(rd.read_bool().unwrap(), true);
    assert_eq!(rd.read_u8().unwrap(), 0xaa);
}
```

`split()` is also the mechanism behind forwards/backwards compatibility: because a reader bounds itself to exactly
the byte range a size slot describes, code that doesn't recognize a newer field simply never advances past the end
of that slot and skips whatever trailing bytes it doesn't understand, while code reading data that's missing a
field it does know about just runs out of bytes and falls back to that field's default. See
[evolution rules](../evolution/rules.md) for the full picture of what changes stay wire-compatible.

### BufWriterOwned

`BufWriterOwned` is a bit compatible writer that uses Vec storage instead of a mutable slice.

## Traits and `ElementSize`

Every serializable type implements [`SerializeShrinkWrap` and `DeserializeShrinkWrap`](https://github.com/vhrdtech/wire_weaver/blob/master/shrink_wrap/shrink_wrap/src/traits.rs)
(both work on borrowed data, no allocator required); types that also support `std`/`alloc` additionally implement the
`..Owned` counterparts, `SerializeShrinkWrapOwned`/`DeserializeShrinkWrapOwned`, which serialize into a growable
`BufWriterOwned` instead of a fixed slice and drop the `u16::MAX` size-per-object limit that plain `BufWriter`
currently has. `#[derive_shrink_wrap(..)]` generates all four for you, one pair per representation you ask for (see
[borrowed, owned, or both?](derive.md#borrowed-owned-or-both)).

Each of the four traits carries one associated constant, `ELEMENT_SIZE: ElementSize`, which is the single piece of
information the whole size-FIFO mechanism above is built on - it tells a _parent_ type, at compile time, whether a
field needs a size slot reserved for it at all:

| `ElementSize` variant   | Meaning                                                                                                                                                                                 |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Unsized`               | Size isn't known up front; a parent must reserve a FIFO slot for it. The default for structs/enums - fully evolvable.                                                                   |
| `UnsizedFinalStructure` | Also size-unknown, but flattened onto the parent instead of getting its own slot - it shares the parent's FIFO position. `Vec<T>`, `String` and `&str` are all `UnsizedFinalStructure`. |
| `SelfDescribing`        | No size is stored at all; a reader can tell where the value ends from the encoding itself (`UNib32`'s continuation bits, `Option`/`Result`'s presence flag, ...).                       |
| `Sized { size_bits }`   | Size is fixed and known at compile time; nothing is ever stored for it.                                                                                                                 |

`ElementSize::add` combines field sizes when the macro computes a struct's/enum's own `ELEMENT_SIZE` - the
combination is deliberately "sticky" upward (`Sized` + `SelfDescribing` = `SelfDescribing`, anything + `Unsized` =
`Unsized`, and `UnsizedFinalStructure` wins over everything), so a type can never end up silently claiming a
stronger guarantee than its fields actually provide; a compile-time assertion checks this for you.
[Size assumptions](derive.md#size-assumptions-final_structure-self_describing-sized) in the derive page covers how
to pick one of these for your own types via the `final_structure`/`self_describing`/`sized` directives, and the
[showcase](showcase.md#sub-byte-packing) has worked byte-level examples of each.

## `write`/`read` vs `ser_shrink_wrap`/`des_shrink_wrap`

There are two ways to serialize a value into a `BufWriter`, and they are **not** interchangeable - each has a
matching read-side counterpart, and mixing them up produces a reader that's misaligned with what was actually
written:

- **`value.ser_shrink_wrap(&mut wr)`** writes exactly the value's own bytes, with no size slot reserved for it in
  the FIFO - the caller is expected to already know (or not need) its extent. Read back with
  **`T::des_shrink_wrap(&mut rd)`**.
- **`wr.write(&value)`** additionally reserves a FIFO slot first, but only if `T::ELEMENT_SIZE` is `Unsized`, then
  calls `ser_shrink_wrap` and fills the slot in with the size once it's known. Read back with **`rd.read::<T>()`**,
  which does the matching thing: reads the size first and `split()`s a bounded sub-reader before deserializing, but
  again only for `Unsized` types - for anything else it just calls `des_shrink_wrap` directly.

In other words, `write`/`read` is what makes a value's size discoverable to whatever is _around_ it; use it
whenever you're serializing one value as a field of another (this is exactly what generated struct/enum code does
for each of its fields). `ser_shrink_wrap`/`des_shrink_wrap` is the right choice when nothing needs to be able to
skip over the value from the outside - most commonly for the _top-level_ value in a message, since its extent is
already implied by the buffer's own length and a FIFO slot for it would just waste space:

```rust
fn to_ww_bytes<'i>(&self, buf: &'i mut [u8]) -> Result<&'i [u8], Error> {
    let mut wr = BufWriter::new(buf);
    self.ser_shrink_wrap(&mut wr)?; // not `wr.write(self)` - no point sizing the whole message
    wr.finish_and_take()
}
```

This is precisely what the convenience methods below do.

## `to_ww_bytes`/`from_ww_bytes`

`SerializeShrinkWrap`/`DeserializeShrinkWrap` (and their `..Owned` counterparts) each provide one default method
that wraps the boilerplate of setting up a `BufWriter`/`BufReader` around `ser_shrink_wrap`/`des_shrink_wrap`, for
exactly the top-level-value case above:

```rust
let mut buf = [0u8; 64];
let bytes: &[u8] = my_value.to_ww_bytes(&mut buf)?;      // SerializeShrinkWrap::to_ww_bytes
let value = MyType::from_ww_bytes(bytes)?;               // DeserializeShrinkWrap::from_ww_bytes

// std/alloc: no scratch buffer to manage, and the u16::MAX-per-object limit no longer applies
let bytes: Vec<u8> = my_value.to_ww_bytes_owned()?;      // SerializeShrinkWrapOwned::to_ww_bytes_owned
let value = MyType::from_ww_bytes_owned(&bytes)?;        // DeserializeShrinkWrapOwned::from_ww_bytes_owned
```

These are what every example on this site actually calls; reaching for `BufWriter`/`BufReader` and `write`/`read`
directly only matters once you're implementing serdes for a hand-written type, building a streaming API on top of
`shrink_wrap` (see the [builder pattern](showcase.md#patching-a-discriminant-after-youve-already-started-writing-its-payload)
in the showcase), or otherwise need more control than one call to `ser_shrink_wrap` gives you.

## Size of the rest of a value: `TailSize<N>`

The FIFO of sizes puts a value's size at the _back_ of the buffer, which is exactly right when the buffer is the
value: the reader knows where the back is. It doesn't help when the value sits in a buffer longer than itself - a
header at the start of a file, a record at a fixed offset of a ring file, a message with padding after it - nor
when a reader wants to skip a value without parsing it. `TailSize<N>` covers that case from the inside: a field
that the writer fills in, once the value is serialized, with the number of bytes from the end of the field to the
end of the value (the value's own reverse lengths included), and that the reader uses to bound the rest of the
value.

On the wire it is a `UVlq32` right-justified in `N` bytes (1 to 5, 5 by default), padded with empty `0x80` groups
in front, so any `UVlq32` reader decodes it; `N` is the capacity: 127 bytes for `N = 1`, 16 KB for 2, 2 MB for 3,
256 MB for 4, 4 GB for 5, and serialization fails with `Error::LenTooLong` beyond it.

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Header<'i> {
    magic: u16,
    size: TailSize<2>, // filled in on write, ignored as an input
    name: &'i str,
    count: u32,
}
```

`#[derive_shrink_wrap(..)]` generates, around the fields after the slot, what you would write by hand with
`BufWriter::reserve_tail_size()` and `backfill_tail_size()`: reserve the slot, write the rest, encode the reverse
lengths those fields pushed (so they land inside the value's range, where they would have gone anyway), align to
byte, and fill the slot in. On read: read the slot, `split()` the reader to that many bytes, read the rest from the
split. So a buffer longer than the value stops at the value's end, a truncated one fails with
`Error::OutOfBoundsSplit` instead of reading garbage, and fields a newer writer appended after the ones this reader
knows are skipped - the usual evolution rules keep working inside the bounded range. Nested values each get their own
slot, and a value with a slot is still a plain `Unsized` value to whatever contains it.

The rules the macro checks: one `TailSize` per struct or enum variant, as a plain field (not inside `Option`, `Vec`,
tuples or arrays), only in `Unsized` types (a `sized`, `final_structure` or `self_describing` type has no end of its
own on the wire), and every field before it `Sized` or `SelfDescribing` - a `Vec`, `String` or `Unsized` field keeps
its length at the back of the buffer, which is not where the value ends when the buffer is longer than it. The slot,
its position and `N` are part of the layout: put it in the first version of a type. The
[showcase](showcase.md#bounding-a-value-from-the-inside-with-tailsize) has the bytes.

## Compressed sequences: `Delta`, `DeltaOfDelta` and `XorFloat`

A `Vec<T>` of readings taken one after another is mostly redundancy: the next value is usually the previous one
or close to it. `Delta<'i, T>`, `DeltaOfDelta<'i, T>` (integers `u8`..`u64`, `i8`..`i64`) and `XorFloat<'i, T>`
(`f32`, `f64`) are sequence types used exactly like `RefVec<'i, T>` - a field holding a slice to serialize, a
bounded reader after deserializing, decoded as you `.iter()` - whose elements are written bit by bit against the
element before. Their owned counterparts `DeltaOwned<T>`, `DeltaOfDeltaOwned<T>` and `XorFloatOwned<T>` wrap a
`Vec<T>` (`std`), and the derive macro maps one to the other like it does `RefVec` and `Vec` (see
[type mapping](derive.md#type-mapping)).

```rust
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
struct Block<'i> {
    ts: u32,
    size: TailSize<2>,
    stamps: DeltaOfDelta<'i, u64>, // timestamps with a near-constant step
    counts: Delta<'i, i16>,        // a counter, fixed-point readings
    temps: XorFloat<'i, f32>,      // floats
}
```

Around the elements the layout is that of `Vec<T>`: `UnsizedFinalStructure`, the element count as a reverse
length at the back of the enclosing value, nothing else. Inside:

- The first element is written at the full width of `T` through the bit cursor (most significant bit first, so a
  `u32` first element reads as big-endian in a hex dump, unlike a `u32` field), every further element as a prefix
  code against the previous one. There is no alignment in between or after: a `bool` written next shares the
  byte, a `u8` aligns itself as always.
- **`Delta<T>`**: the difference to the previous element, wrapped at the width `W` of `T` (so `u8` 0 after 255 is
  `+1`), zigzag-encoded to `z` and written as `0` for `z = 0`, else `10` and 7 bits (`z < 128`), `110` and 9 bits
  (`< 512`), `1110` and 12 bits (`< 4096`), `11110` and 20 bits (`< 2^20`), `111110` and 32 bits (`< 2^32`), where
  buckets at or above `W` are left out and the last bucket is `W` bits after a `1` for every bucket and no `0`:
  `11` + 8 bits for `u8`/`i8`, `1111` + 16 bits for 16-bit types, `11111` + 32 bits for 32-bit, `111111` + 64 bits
  for 64-bit types. A repeated value costs 1 bit, a step within `-64..=63` costs 9.
- **`DeltaOfDelta<T>`**: the same code applied to the change of the difference (the previous difference starts at
  0), the Gorilla timestamp code: a value that grows by the same step as the one before costs 1 bit.
- **`XorFloat<T>`**: the XOR of the bit patterns of the value and the previous one. `0` when it is zero (a repeated
  value); `10` and the bits inside the window of the previous explicitly coded XOR (its leading and trailing zero
  counts) when the new XOR fits in it; otherwise `11`, the leading zero count (5 bits for `f32`, 6 for `f64`), the
  number of significant bits minus one (5 / 6 bits) and those bits, which become the new window. Pure bit
  operations: NaN payloads, `-0.0` and infinities round-trip exactly.

These codes are part of the wire format and stay as they are, like the rest of this page. The
[showcase](showcase.md#compressed-sequences-delta-deltaofdelta-and-xorfloat) has the bytes.

What to expect: on an hour of 1 s telemetry from `tpm_mesh_dash` (34 series, mostly CPU percentages and byte
counters), `XorFloat<f32>` in blocks of 60 took 64% of the raw `f32`s (39% of the values repeat exactly, those
cost one bit), while the same values as tenths of a percent and whole bytes in `Delta<i32>` / `Delta<i64>` took
32%. Floats with full-precision noise (a CPU percentage computed from counters) XOR badly - every value changes
most mantissa bits; integers, fixed-point values, slowly changing or often repeated readings and timestamps
compress well. Pick the integer type by the data, not by the deltas: the first element and the widest bucket use
its width, the small buckets are the same for every type.

`cargo run --example time_series` (in `shrink_wrap/shrink_wrap`) prints the encoded sizes of synthetic timestamps,
a counter, a sine wave and a constant, raw vs `Delta`/`DeltaOfDelta`/`XorFloat`.

The decoders never panic on malformed input: every read is bounds-checked, a count larger than the bits left is
rejected before anything is allocated (`Error::MalformedSeries`), and the owned decoders push element by element
instead of trusting the count. Since the elements carry no length of their own, a truncated *top-level* buffer may
still decode (to other values) when the garbage happens to form valid codes; inside a struct, the enclosing size
slot or `TailSize` bounds the value and a truncation is caught there.

## Next step

Check out the [derive](derive.md) macro that generates all of the above from a plain struct/enum definition, and
the [showcase](showcase.md) for worked, byte-verified examples of the tricks this format enables, and the
[use cases](use_cases.md) for real files, frames and messages built with it.
