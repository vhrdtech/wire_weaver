---
level: advanced
---

# Special types

Types of the `shrink_wrap` crate for cases the plain ones don't cover: a value that bounds itself inside a longer
buffer, owned strings and lists without an allocator, compressed time series, lists written in place, and sparse
sets of values. Each section starts with what the type is for and how to use it; the exact bits are in
[wire format internals](wire_format.md#encodings-of-the-special-types).

## `TailSize<N>`: the size of the rest of a value

The [FIFO of lengths](wire_format.md#the-fifo-of-lengths) puts a value's size at the _back_ of the buffer, which is
exactly right when the buffer is the value: the reader knows where the back is. It doesn't help when the value sits
in a buffer longer than itself (a header at the start of a file, a record at a fixed offset of a ring file, a
message with padding after it), nor when a reader wants to skip a value without parsing it.

`TailSize<N>` covers that from the inside. It is a field that the writer fills in, once the value is serialized,
with the number of bytes to the end of the value, and that the reader uses to bound the rest of it:

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Header<'i> {
    magic: u16,
    size: TailSize<2>, // filled in on write, ignored as an input
    name: &'i str,
    count: u32,
}
```

`N` is the number of bytes the slot takes (1 to 5, 5 by default), which caps the size: 127 bytes for `N = 1`,
16 KB for 2, 2 MB for 3, 256 MB for 4, 4 GB for 5. A bigger value fails with `Error::LenTooLong`.

What you get:

- a buffer longer than the value stops at the value's end;
- a truncated one fails with `Error::OutOfBoundsSplit` instead of reading garbage;
- fields a newer writer appended after the ones this reader knows are skipped: the usual evolution rules keep
  working inside the bounded range;
- nested values each get their own slot, and a value with a slot is still a plain `Unsized` value to whatever
  contains it.

The rules the macro checks:

- one `TailSize` per struct or enum variant, as a plain field (not inside `Option`, `Vec`, tuples or arrays);
- only in `Unsized` types: a `sized`, `final_structure` or `self_describing` type has no end of its own on the wire;
- every field before it is `Sized` or `SelfDescribing`. A `Vec`, `String` or `Unsized` field keeps its length at
  the back of the buffer, which is not where the value ends when the buffer is longer than it.

The slot, its position and `N` are part of the layout: put it in the first version of a type. The
[showcase](showcase.md#bounding-a-value-from-the-inside-with-tailsize) has the bytes.

## `ArrayString` / `ArrayVec`: owned values without an allocator

`&str` / `RefVec<'i, T>` borrow from the buffer they were read from, and `String` / `Vec<T>` need an allocator.
Neither fits a field that must own its bytes without one: a `value_on_changed` field of a string or list property in
a `no_alloc` server (which can't hold a `&'static str`), or a settings value kept in RAM between loads.

`ArrayString<N>` and `ArrayVec<T, N>` are the owned, no-alloc alternative: a fixed-size `[u8; N]` /
`[MaybeUninit<T>; N]` plus a length. They write the same bytes as `&str` / `String` and `Vec<T>`, so a field can
move between the borrowed, owned and array-backed forms without changing a single byte:

```rust
use shrink_wrap::prelude::*;

let mut s: ArrayString<8> = ArrayString::new();
s.push_str("abc").unwrap();
assert_eq!(s.as_str(), "abc");

let mut buf = [0u8; 16];
let bytes = s.to_ww_bytes(&mut buf).unwrap();
assert_eq!(bytes, "abc".to_ww_bytes(&mut [0u8; 16]).unwrap());
```

- A value longer than `N` is never cut: it fails with `Error::MaxLenExceeded`, whether in `push_str` / `push`,
  `ArrayString::try_from`, serializing, or deserializing a value too long for this reader's `N` (the same check
  [`MaxLen`](../types.md#bounded-lists-and-strings-maxlent-n) makes).
- Both have a `CAPACITY` const and `Deref` to `str` / `[T]`.
- Both set `MAX_SIZE` from `N`, so a type built out of them has a
  [compile-time worst-case size](../types.md#worst-case-size-at-compile-time) like any other bounded type.
- `ArrayVec<T, N>` drops its elements in place like a normal container; `T` does not need `Copy`.

They are Rust-side types: an API (`#[ww_trait]`) describes such a field as a string or a list.

## Compressed sequences of numbers: `Series<T>`

A list of readings taken one after another is mostly redundancy: the next value is usually the previous one or
close to it. `Series<'i, T>` (`SeriesOwned<T>` with `std`) writes such a list bit by bit against the value before,
picking per list the code that comes out smallest: for integers the difference to the previous value or the change
of that difference (timestamps with a near-constant step) in a prefix code, bit-packed blocks, a constant or the
plain values; for `f32` / `f64` the XOR with the previous value (the Gorilla code) or the plain values. It is used
like `RefVec<'i, T>`: a field holding a slice to serialize (`Series::new(&values)`), decoded as you `.iter()` after
deserializing.

```rust
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
struct Block<'i> {
    ts: u32,
    size: TailSize<2>,
    stamps: Series<'i, u64>, // timestamps with a near-constant step
    counts: Series<'i, i16>, // a counter, fixed-point readings
    temps: Series<'i, f32>,  // floats
}
```

It works in derived types and in every API position (arguments, returns, properties, stream items); the dynamic
client and Python read and write it as a list. `Series<T>` also takes structs and enums, a column per field: see
[Series](series.md) for all of it, the codecs and a fixed codec for a small MCU (`Series::with_policy`).

**What to expect.** `cargo run --example time_series` (in `shrink_wrap/shrink_wrap`) prints the encoded sizes of a
minute of 1 Hz synthetic data against the raw `Vec<T>`: timestamps with jitter 38 B instead of 481 B, a counter
31 B instead of 241 B, a sine wave in `f32` 70 B instead of 241 B, a constant 13 B. Floats with full-precision noise
(a CPU percentage computed from counters) XOR badly: every value changes most mantissa bits; the same values as
fixed-point integers compress well. A `Series` has a count, a 4-bit header per column and pads to a byte, so a list
of a handful of values gains little.

**Malformed input.** The decoder never panics: every read is bounds-checked, the item count is capped
(`SERIES_MAX_ITEMS`) and the columns are validated in one pass before an item is decoded; inside a struct, the
enclosing length slot or `TailSize` bounds the value and a truncation is caught there.

The codes: [wire format internals](wire_format.md#the-delta-and-xor-codes); the bytes:
[showcase](showcase.md#compressed-sequences-of-numbers-series).

## `RefVecBuilder`: lists written in place

`RefVec::Slice` needs the elements in a slice first. `RefVecBuilder<T>` writes a list straight into a `BufWriter`
instead, one element at a time, from an iterator or a loop reading a sensor, with no memory for the elements:

- `RefVecBuilder::<T>::new(&mut wr)` at the place of the list: the whole buffer, a field of a struct written by
  hand, an element of another list (a `RefVec` stores no size of its own, so it is the same call everywhere);
- then `push(&mut wr, &value)` per element, `extend(&mut wr, items)` for an iterator, or
  `push_with(&mut wr, |wr| ..)` to write an element by other means, e.g. with a nested builder;
- then `finish(&mut wr)`, which writes the element count.

The bytes are those of the same `RefVec::Slice`, so a reader can't tell. Three 12-bit samples between a byte and a
flag:

```rust
let mut wr = BufWriter::new(&mut buf);
wr.write_u8(7).unwrap();
let mut samples = RefVecBuilder::<U12>::new(&mut wr).unwrap();
for ch in 0..3 {
    samples.push(&mut wr, &adc_read(ch)).unwrap(); // 0x023, 0x123, 0x223
}
samples.finish(&mut wr).unwrap();
wr.write_bool(true).unwrap();
assert_eq!(wr.finish_and_take().unwrap(), hex!("07 02 31 23 22 38 03"));
```

The samples are bit-packed as in any list (`023` `123` `223`), the flag follows in the next bit (`3` `1` and
padding), and the count `3` is at the back. A list of lists takes a nested builder per element:

```rust
let groups: [&[&str]; 2] = [&["a", "bc"], &["d"]];
let mut outer = RefVecBuilder::<RefVec<&str>>::new(&mut wr).unwrap();
for group in groups {
    outer.push_with(&mut wr, |wr| {
        let mut inner = RefVecBuilder::<&str>::new(wr)?;
        inner.extend(wr, group)?;
        inner.finish(wr)
    }).unwrap();
}
outer.finish(&mut wr).unwrap();
assert_eq!(wr.finish_and_take().unwrap(), hex!("61 62 63 64 11 21 22"));
```

`push_with` writes the element as its type's `ser_shrink_wrap` would; for an evolvable element the builder adds its
size around it, as `wr.write` does.

- **A push that fails** (out of buffer space, an element that fails to serialize) returns the error and undoes what
  the element wrote: the list holds the elements pushed before and can still be finished, the count is never off.
- **`finish` must be called.** Until then the count is 0 while the elements are already in the buffer, and a reader
  sees garbage. The builder is `#[must_use]`, but that only catches one that is never used at all.
- **`MaxLen<RefVec<'i, T>, N>`**: start it with `RefVecBuilder::with_max_len(&mut wr, N)`, push `N + 1` fails with
  `Error::MaxLenExceeded` and writes nothing. A `BufWriter` keeps lengths as `u16` until they are encoded, so the
  65 536th push fails with `Error::LenTooLong`.

`RefVecBuilderOwned<T>` is the same builder for a `BufWriterOwned` and `SerializeShrinkWrapOwned` elements, with the
same bytes.

A list of bytes, `RefVecBuilder<u8>`, has two more ways to add elements, both mixing freely with `push`:

- `extend_from_slice(&mut wr, &bytes)` copies a whole slice at once, one element per byte;
- `write_bytes_with(&mut wr, |wr| ..)` counts every byte the closure writes, by any means: `write_raw_slice`,
  `write_u8`, or other values, making the list a byte blob that holds them. It aligns to a byte first, encodes the
  lengths the values inside pushed (so they are part of the blob) and aligns again; a closure that writes nothing
  adds nothing, not even the alignment. On an error the writer is back where it was, as for a push.

```rust
let mut bytes = RefVecBuilder::<u8>::new(&mut wr).unwrap();
bytes.write_bytes_with(&mut wr, |wr| {
    wr.write(&"hi")?;
    wr.write_u8(0x42)
}).unwrap();
bytes.finish(&mut wr).unwrap();
wr.write_u8(0x99).unwrap();
assert_eq!(wr.finish_and_take().unwrap(), hex!("68 69 42 02 99 04"));
```

`68 69` is `"hi"`, then `42`, then the length of `"hi"` (`2` after a padding nibble), all inside the blob of 4 bytes:
`99` follows it and the count `4` is at the back. Read it back as a `RefVec<u8>` and the blob with a `BufReader` of
its own.

## `OptionAnyVec`: sparse slots of any types

`shrink_wrap::option_any_vec::OptionAnyVec<'i>` is an array of `Option` slots whose values can be of any type
(numbers, structs, lists, strings, enums, another `OptionAnyVec`), where only the present values are written. It is
made for messages that carry "some of these N things": the properties of a trait level that changed since the last
update, a sparse set of readings.

It is low-level machinery: the type of each slot is known to both sides and not stored, and slot ids are `u32` from
0. For a struct with a field per slot, use a [`sparse` struct](layouts.md#sparse-a-struct-of-optional-fields)
instead, which is built on it.

```rust
use shrink_wrap::option_any_vec::{OptionAnyVec, OptionAnyVecWriter};

let mut buf = [0u8; 64];
// the present slots are given up front, in increasing order
let mut wr = OptionAnyVecWriter::new(&mut buf, [0u32, 2, 3]).unwrap();
wr.write(0, &0x2Au8).unwrap();
wr.write(2, &"hi").unwrap();
wr.write(3, &true).unwrap();
let bytes = wr.finish_and_take().unwrap();
assert_eq!(bytes, hex!("14 b0 2a 68 69 82"));

let vec = OptionAnyVec::new(bytes).unwrap(); // checks the header
let mut slots = vec.iter();
assert_eq!(slots.next::<u8>(), Ok(Some(0x2A)));
assert_eq!(slots.next::<u16>(), Ok(None));
assert_eq!(slots.next::<&str>(), Ok(Some("hi")));
assert_eq!(slots.next::<bool>(), Ok(Some(true)));
assert_eq!(slots.next::<u8>(), Ok(None)); // and every slot after the writer's last one
```

The bytes: `1` `4` is the header's mode (dense) and length, `1011` says slots 0, 2 and 3 are present, then `2a`,
`68 69`, the `true` bit, and the length of `"hi"` in the last nibble. The writer picks the smallest of four header
modes for the present set ([header modes](wire_format.md#optionanyvec)).

### Evolution

The header describes the writer's present set on its own, without either side's slot count, so the vec is read by
older and newer code alike as long as slots are only appended:

- a reader that knows fewer slots reads its own and stops: the values it doesn't know come after all of its slots;
- a reader that knows more slots sees the new ones as absent: they are not in the header.

A slot's type can't change once released, as for a struct field. Since values carry no length of their own, a
reader can't skip a present value it doesn't know the type of, which, with append-only slots, it never has to.

### Untrusted input

`OptionAnyVec::new` (and reading it as a field) parses the whole header and checks every count and length against
the bytes left before looping over it:

- a length past the data, an index list past `u32::MAX`, a two-level group listed without a slot in it is
  `Error::MalformedOptionAnyVec`;
- a truncated header is an out-of-bounds error, never a panic.

Reading the values is bounds-checked like any other read. Slot ids from the header are not bounded by any slot
count, so stop at the last slot you know (`iter().next()` does that by itself).

### Writing in place

`OptionAnyVecBuilder` writes into a `BufWriter` that holds other things too:

- `OptionAnyVecBuilder::new(&mut wr, present)` for a vec that is the whole buffer, `new_nested` for one that is a
  struct field or a slot of another vec (its size is stored, as for every `Unsized` value);
- then `write(slot, &value, &mut wr)` per present slot, or `start_slot(slot)` and a write of your own (e.g. a nested
  builder);
- then `finish(&mut wr)`, which fails with `Error::OptionAnyVecWrongSlot` if a present slot got no value.

The present set is any increasing iterator of slot ids that can be cloned; `SetBits::new(&[u32])` gives one from a
bitset. `new_with_mode` forces a header mode. A child level is one slot holding its own vec:

```rust
let mut wr = BufWriter::new(&mut buf);
let mut parent = OptionAnyVecBuilder::new(&mut wr, [0u32, 1]).unwrap();
parent.write(0, &7u8, &mut wr).unwrap();
parent.start_slot(1).unwrap();
let mut child = OptionAnyVecBuilder::new_nested(&mut wr, [2u32]).unwrap();
child.write(2, &0x1234u16, &mut wr).unwrap();
child.finish(&mut wr).unwrap();
parent.finish(&mut wr).unwrap();
assert_eq!(wr.finish_and_take().unwrap(), hex!("02 07 13 20 34 12 04"));
```

The bytes:

- `0` `2`: mode all, slots 0 and 1;
- `07`: slot 0;
- the child, 4 bytes: `1` `3` (dense, 3 slots), `001` and padding, `34 12`;
- `04`: the child's size in the parent's reverse lengths (after a padding nibble).

The same bytes come out of writing the child into a buffer of its own first and `parent.write(1, &child_vec, ..)`.
Reading: `iter.next::<OptionAnyVec>()` gives the child, `child.iter()` its slots; `next_present()` jumps over absent
slots, `present()` lists the present ids without reading values. `OptionAnyVecBuilderOwned` is the same builder for
a `BufWriterOwned`, the bytes are the same.

### Slots with gaps

A reader that knows slots 0, 1 and 5 but not 2..=4 reads them with `iter.read_slot::<T>(slot)`, which steps over the
absent slots in between. A present slot in such a gap can't be stepped over (its type, so its size, is unknown):
`read_slot` fails with `Error::OptionAnyVecUnknownSlot`. Gaps are for slots that are never present, as the methods
between the properties of a trait level, never for slots a newer version might fill.

### `EitherAnyVec`

`OptionAnyVec` is `Unsized`. `EitherAnyVec` is the counterpart for `Either` slots that are all present: each element
is `L` or `R`, the flags are in-line in groups of 8, and the element count is implied. It is used to answer
[multi requests](../api/multi_requests.md#the-reply-an-eitheranyvec-of-results), one `Either<Error, value>` per
index.
