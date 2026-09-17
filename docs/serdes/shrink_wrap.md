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

## Next step

Check out the [derive](derive.md) macro that generates all of the above from a plain struct/enum definition, and
the [showcase](showcase.md) for worked, byte-verified examples of the tricks this format enables.
