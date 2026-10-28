---
level: advanced
---

# Fixed and sparse layouts

Three derive directives that change how a type is laid out, for cases the default encoding doesn't serve well:

| Directive                                                   | For                                                                                       |
|-------------------------------------------------------------|-------------------------------------------------------------------------------------------|
| [`fixed_layout`](#fixed_layout-fields-at-fixed-offsets)     | every field at a constant offset: headers, flash records, the smallest possible reader    |
| [`head_tail`](#head_tail-a-fixed-head-and-a-sequential-tail) | lists and strings in a format a tiny C or Rust reader walks front to back                  |
| [`sparse`](#sparse-a-struct-of-optional-fields)              | a struct of many optional fields, where only the present ones are written                  |

`fixed_layout` keeps the regular `shrink_wrap` bytes and adds a fast path; `head_tail` and `sparse` are encodings of
their own. How an enum is laid out in memory, not on the wire, is [`niche`](#niche-an-enums-tag-in-memory)'s.

## `fixed_layout`: fields at fixed offsets

`shrink_wrap` writes a value field after field. When no field's size depends on its value, every field starts at
the same place in every value: a header, a record in flash, a descriptor that is signed as bytes. Reading a field is
then a load at a constant offset, and the macro generates that next to the usual serdes code (SW-44, SW-45).

```rust
#[derive_shrink_wrap(borrowed, fixed_layout)]
struct Header {
    magic: [u8; 4],
    urgent: bool,
    retries: U3,
    count: u32,
    inner: Inner,         // another fixed_layout type
    extra: Option<Extra>, // last: present when its flag bit is set
}

let header = Header::from_fixed(&bytes)?; // what a tiny bootloader links: loads at constant offsets
let header = Header::from_ww_bytes(&bytes)?; // the same value through BufReader
```

What gets generated:

- `ENCODED_LEN`: the longest encoding, the buffer to have ready. `MIN_LEN`: the shortest. `fixed_len()`: the
  length of this value. All three are the same unless the last field has a variable length (see below).
- An offset const per field: `COUNT_OFFSET` in bytes, or for a bit field `URGENT_BIT_OFFSET` and `URGENT_BITS`,
  counted from the most significant bit of the first byte. For an enum `TAG_LEN`, and per variant `<VARIANT>_TAG`,
  `<VARIANT>_LEN` and `<VARIANT>_<FIELD>_..` (unnamed fields by their index). An `Option`'s flag is `<FIELD>_FLAG`. A tuple struct's fields are `_0`, `_1`,
  ...: `_2_OFFSET`, in C `PX__2_OFFSET` and `px__2()` for `struct Px(u8, bool, u16)`.
- `from_fixed(&[u8])` and `to_fixed(&mut [u8])`: loads and stores (shifts and masks for bit fields) at those
  offsets, without a `BufReader` or `BufWriter`. The bytes are those of `ser_shrink_wrap`. A short buffer is an
  error, never a panic.
- The `FixedLayout` trait, whose `write_c` prints the C reader of the type: a define per offset and a
  `static inline` reader per field (`shrink_wrap::fixed_layout::write_c_header` writes a whole header).

On a Cortex-M0, decoding a header of eight integers takes 276 bytes of code this way and 484 through `BufReader`; a
bit-packed sample 276 instead of 652 ([code size](code_size.md#fixed-layout-fast-path)).

### What has a fixed offset

| Field                                                              | Rule                                                                                                                                                                  |
|--------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `u8`..`u64`, `i8`..`i64`, `f32`, `f64`, arrays of them             | start on a byte, little-endian, as `BufWriter` writes them                                                                                                            |
| `bool`, `U1`..`U32`, `I2`..`I32`, `Nibble`                          | packed most significant bit first; a `Nibble` starts on a 4-bit boundary; the next byte-aligned field starts a fresh byte                                              |
| another `fixed_layout` type                                        | starts on a byte; it can't share a byte with bit fields of the parent (a compile error asks for a byte-aligned field between them)                                    |
| `Option<T>`, `Result<T, E>`, an enum whose variants differ in length | only as the **last** field: the fields before it keep their offsets, nothing after it could. The payload starts on a byte; the flag is an ordinary bit field, also where `#[flag]` moves it |

Not: lists, strings, `MaxLen`, varints (`UNib32` and friends), `RefBox`, `TailSize`, tuples, ranges, 128-bit
numbers, bit fields wider than 32 bits, `#[default]` fields. A struct must be `sized`, `final_structure` or
`self_describing`: an evolvable struct may grow, so it has no fixed layout. An enum must be `sized` or
`self_describing` with a byte-aligned tag (`ww_repr = u8`, `u16` or `u32`).

The rule for variable length is the same everywhere: an `Option`, a `Result` and an enum of unequal variants are
each a value whose length depends on what it holds. As a whole type, or as the last field of one, that is fine:
`fixed_len()` says how long this value is. Before another field it is not, because that field would move.

### With and without the directive

Types that qualify get the fast path without asking. The `fixed_layout` directive makes it a requirement:

- a field that breaks it is a compile error that says why and what to use instead, for example "field `count`: a
  variable-length number takes more bytes for larger values, so the fields after it have no fixed offset. Use u32 /
  u64, or U1..U32 for fewer bits";
- only with the directive may a field be another fixed-layout type (the macro can't look inside a type it doesn't
  see; the compiler checks that it implements `FixedLayout`);
- on a struct it also picks the size assumption when none is given: `sized`, or `self_describing` when the last
  field is an `Option` or `Result`. A struct whose last field is an enum of unequal variants says `self_describing`
  itself.

## `head_tail`: a fixed head and a sequential tail

For types with lists, strings or options in the middle, where no fixed layout exists, `head_tail` picks a wire
format of its own (SW-46), made for the smallest reader on a microcontroller:

- the **head** has a length known at compile time and holds every fixed-width field at a constant offset, as
  `fixed_layout` places them, and for each variable field what is fixed about it: a `u16` count for a list or
  string, the flag bit of an `Option` or `Result`, the head of a nested `head_tail` or `fixed_layout` type;
- the **tail** follows the head and holds the variable parts in field order: list items, string bytes, option
  payloads, the tails of nested types.

An enum is its tag (`ww_repr = u8`, `u16` or `u32`), then the head and tail of its variant. Reading the head is
loads at constant offsets; the tail is read front to back by a cursor that only takes lengths it already knows
(`Tail`). There are no lengths at the back, no `BufReader`, no per-type generic reader.

```rust
use shrink_wrap::head_tail::List;

#[derive_shrink_wrap(head_tail)]
struct Utx<'i> {
    ch: u8,
    fmt: UartFormat,        // a fixed_layout type: in the head
    glitch: Option<Wave>,   // flag bit in the head, the Wave in the tail
    perr: List<'i, u16>,    // count in the head, the items in the tail
    name: &'i str,          // the same
    defer: bool,            // a bit in the head, after all of them
}

let utx = Utx::from_head_tail(&bytes)?;
let n = utx.to_head_tail(&mut buf)?;
```

- A list is a `shrink_wrap::head_tail::List<'i, T>`: `List::new(&items)` to write; read from the wire it keeps the
  bytes and reads an item when its iterator reaches it. Its items are numbers or `fixed_layout` types of one length.
- `Option` / `Result` payloads are numbers, arrays of them, or `head_tail` / `fixed_layout` types.
- Every `fixed_layout` type implements `HeadTail` (all head when it has one length, else all tail), so the types of
  a protocol mix freely.

It is one type with one format: a `head_tail` type has no `SerializeShrinkWrap` / `DeserializeShrinkWrap` (and no
owned version), so it can't be a field of an ordinary `shrink_wrap` type or a WireWeaver API argument.
`write_c_ht` prints the C reader: head defines and readers as for `fixed_layout`, and per type a `<name>_tail_t` and
a `<name>_tail()` that walks the tail and points at each variable part (`shrink_wrap::head_tail::write_c_header`
writes a whole header).

### Views and writers

For firmware that should not build the whole value first (SW-52):

- `Utx::view(&bytes)?` checks the lengths and walks the tail once (counts and flags are the only fields it reads)
  and gives a `UtxView<'_>`: a reference to the head whose accessors (`v.ch()`, `v.defer()`) read each field at its
  offset when called, and the variable parts found on the way (`v.perr()` a `List`, `v.glitch()` an
  `&Option<Wave>`, a nested `head_tail` type its own view).
- An enum's view is an enum of its variants' views: `RequestView::Stop(v)` with `v.mask()`; a tuple variant of one
  nested type holds that type's view (`RequestView::Utx(UtxView)`).
- Each enum variant also gets a writer, `Reply::write_run(&mut tx, run, started, mark, lead, mark_line)?`, which
  writes the same bytes as `to_head_tail` of that variant without making the value.

Both come from the code of `from_head_tail` / `to_head_tail` and are tested against it byte for byte. View types are
always `pub` (the `View` of the public `HeadTail` trait).

Enum variants may have a `#[cfg]` (SW-54): a variant compiled out keeps its tag, and a build without it reads that
tag as an unknown one. One protocol enum can then serve firmware images that each handle a part of it.

On busgen's request and reply enums the codec takes 4590 bytes on a Cortex-M0 through values and 3894 through views
and writers, a hand-written one 4408 and the derived `shrink_wrap` one 6652 ([code size](code_size.md#head-and-tail)).

## `sparse`: a struct of optional fields

`sparse` (DER-13) encodes a struct whose fields are all `Option<T>` as an
[`OptionAnyVec`](special_types.md#optionanyvec-sparse-slots-of-any-types): a presence header, then only the present
values. Each field is one slot, in declaration order; `#[slot = N]` places a field at slot `N` instead (slots
increase, gaps are allowed).

It is made for updates that carry any subset of many values, such as the per trait level `<Level>Update` of an
[observe request](../api/observing.md#wire-format), whose slots are the level's resource indices with methods and
streams in between:

```rust
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Default), sparse)]
struct MotorUpdate<'i> {
    speed: Option<u16>,    // slot 0
    name: Option<&'i str>, // slot 1
    #[slot = 5]
    current: Option<f32>,  // slot 5, slots 2..=4 are never present
    on: Option<bool>,      // slot 6
}

let update = MotorUpdate { speed: Some(1500), on: Some(true), ..Default::default() };
assert_eq!(update.to_ww_bytes(&mut buf)?, [0x17, 0x82, 0xdc, 0x05, 0x80]);
```

The bytes:

- `1` `7`: the dense header mode, 7 slots, then a bit per slot, `1000001` and a padding bit (`82`);
- `dc 05` is `speed`, the `80` bit `on`.

With all four fields present it is `17 c6 dc 05 4c 00 00 c0 3f 81`: the bits `1100011`, `speed`, `"L"`, `1.5f32`,
the `on` bit, and the length of `"L"` in the last nibble. One field present is three bytes
(`MotorUpdate { speed: Some(1500), .. }` is `01 dc 05`: the all mode, one slot), none is no bytes at all. The writer
picks the header mode with the fewest bits for the present set
([header modes](wire_format.md#optionanyvec)).

A field may be any type, including another `sparse` struct: a child level is then one slot holding its own sparse
struct, so one message carries any subset of a tree of levels. The borrowed and the owned type write the same bytes.

Rules, checked at compile time:

- every field is an `Option<T>`; `#[flag]` and `#[default = ..]` don't apply (any field can be absent, a field added
  later reads as `None` from older data);
- `sparse` is an encoding of its own: no `final_structure`, `self_describing`, `sized`, `fixed_layout` or
  `head_tail` with it, and no enums. The struct is `Unsized`;
- `#[slot = N]` is an integer literal past the previous field's slot, below `u32::MAX`, and only on `sparse` fields.

**Evolution**: fields are only appended, after the last slot (see [the rules](../evolution/rules.md#sparse-structs)).
An older reader reads the fields it knows and ignores the rest, a newer reader reads the new fields as `None`. A
reader whose struct has a gap where the data has a present value fails with `Error::OptionAnyVecUnknownSlot`: a gap
is never filled later.

## `niche`: an enum's tag in memory

The derive gives an enum the repr of its `ww_repr`'s native type, so a `ww_repr = unib32` enum has a `u32` tag in
memory: four bytes for a fieldless enum, eight for one with a one-byte field. `niche` leaves the repr out, so Rust can
put the tag in a field's spare values: `exact_units::Prefix` (29 named prefixes and `Exact` with a one-byte handle of
200 values) is one byte, and a quantity that holds one doesn't grow. The wire bytes don't change; see
[the derive reference](derive_reference.md#niche-enums-only) for the rules.
