---
level: basic
---

# ShrinkWrap

![ShrinkWrap logo](../assets/shrink_wrap_logo.svg#only-light){ width="420" }
![ShrinkWrap logo](../assets/shrink_wrap_logo-dark.svg#only-dark){ width="420" }
{ style="text-align:center" }

ShrinkWrap is the wire format of WireWeaver: every method argument, return value, property and stream item goes
through it. It is dense (values are packed down to single bits), it works on a microcontroller without an allocator,
and it lets newer and older versions of a type read each other's data.

It lives in its own crate, `shrink_wrap`, with no dependency on the rest of WireWeaver. If all you need is a compact
format for files, messages or flash records, you can use it on its own.

## A first example

Put `#[derive_shrink_wrap(..)]` on a struct or enum and it gets everything it needs to go on the wire:

```rust
use wire_weaver::prelude::*; // or `use shrink_wrap::prelude::*;` for the crate on its own

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct Sample<'i> {
    id: u8,
    ok: bool,
    label: &'i str,
    error: Option<u16>,
}

fn main() {
    let sample = Sample { id: 7, ok: true, label: "temp", error: None };

    // no_std, no allocator: serialize into a buffer you own
    let mut buf = [0u8; 64];
    let bytes = sample.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, &[0x07, 0x80, 0x74, 0x65, 0x6d, 0x70, 0x04]);

    // read it back without copying: `label` points into `bytes`
    let read = Sample::from_ww_bytes(bytes).unwrap();
    assert_eq!(read, sample);

    // with std: `SampleOwned` has a `String` instead of `&str`, same bytes
    let owned = SampleOwned::from_ww_bytes_owned(bytes).unwrap();
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
}
```

Seven bytes for the whole value:

| Bytes         | What                                                                              |
|---------------|-----------------------------------------------------------------------------------|
| `07`          | `id`                                                                              |
| `80`          | `ok`: one bit, the rest of the byte is padding before the next byte-aligned field |
| `74 65 6d 70` | `"temp"`                                                                          |
| `04`          | `error`'s `None` flag (one bit), padding, and the length of `"temp"` (4)          |

Two things stand out: a `bool` or an `Option` flag costs one bit, and the string's length is not in front of the
string but at the end of the buffer. Both are explained below.

## What you get

- **Small.** Values are packed at 1-bit, 4-bit or 1-byte alignment, lengths take a nibble when they are small, and
  nothing is stored that the reader already knows (no field names, no type tags).
- **`no_std` and no allocator,** also for strings, lists and nested types, for reading and writing. On the host,
  the same type gets an owned twin (`Sample` → `SampleOwned`) with `String` and `Vec`, writing the same bytes.
- **Zero-copy reading.** A `&str` or a list read from the buffer points into it.
- **Evolvable.** By default a type can get new fields later; old and new code keep reading each other's data
  ([derive: evolving types](derive.md#evolving-types)).
- **Many types out of the box:** integers of any bit width, variable-length numbers, strings and lists with an
  optional maximum length, `Option`, `Result`, tuples, arrays, compressed time series and more
  ([built-in types](../types.md)).

## How it stays small

**Bits are packed.** A `bool` takes one bit, a `U3` three bits, an enum with four variants can take two. Byte-sized
values (`u8`, `u32`, `f32`, strings) start on a fresh byte, the unused bits before them stay zero. Those zero bits
are not wasted forever: a later version of a type can put a new `bool` or `Option` flag there, and old data reads
as `false` or `None`.

**Lengths go to the back.** A string, a list or a nested type whose size isn't known in advance needs its length
stored somewhere. Most formats write it in front of the value, which means knowing the size before writing it.
ShrinkWrap writes the value first and puts its length at the end of the buffer, as a variable-length number of
nibbles. That keeps serialization a single pass with no copying, and small lengths cost half a byte. The reader
picks the lengths up from the back while it walks the data from the front.

**Nothing extra by default, less if you ask.** A struct is evolvable by default, which costs a length nibble or
two when it is nested in another type. A type that will never change can say so (`sized`, `final_structure`,
`self_describing`) and then costs exactly its fields ([size assumptions](size_assumptions.md)).

For the exact bytes, bit by bit, see [wire format internals](wire_format.md).

## Next steps

| Page                                     | Level    | What's there                                                                          |
|------------------------------------------|----------|---------------------------------------------------------------------------------------|
| [Derive](derive.md)                      | Basic    | `#[derive_shrink_wrap]`: borrowed and owned types, evolving types, enums               |
| [Built-in types](../types.md)            | Basic    | Every type that goes on the wire and what it costs                                     |
| [Size assumptions](size_assumptions.md)  | Advanced | `sized`, `final_structure`, `self_describing`: smaller, but frozen                     |
| [Derive reference](derive_reference.md)  | Advanced | Every directive of the macro                                                           |
| [Special types](special_types.md)        | Advanced | `TailSize`, `ArrayString`/`ArrayVec`, compressed sequences, `OptionAnyVec`             |
| [Fixed and sparse layouts](layouts.md)   | Advanced | `fixed_layout`, `head_tail` and `sparse`: other encodings for special cases            |
| [Showcase](showcase.md)                  | Advanced | Worked, byte-verified tricks                                                           |
| [Use cases](use_cases.md)                | Advanced | Real files, frames and messages built with ShrinkWrap                                  |
| [ShrinkWrap in C](../c.md)               | Advanced | The C reader and writer                                                                |
| [Wire format internals](wire_format.md)  | Nomicon  | The FIFO of lengths, `BufWriter`/`BufReader`, `ElementSize`, exact encodings           |
| [Code size on small CPUs](code_size.md)  | Nomicon  | What ShrinkWrap costs in flash and why                                                 |
