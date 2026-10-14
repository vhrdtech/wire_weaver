# Derive

Writing out serializing and deserializing code by hand would be very tedious and error-prone. So a procedural macro
attribute, `#[derive_shrink_wrap(..)]`, is provided that generates all of it from a struct or enum definition.

Besides `SerializeShrinkWrap` and `DeserializeShrinkWrap` (and their `..Owned` counterparts), the same attribute can:

- Generate a borrowed (zero-copy) representation, an owned (`alloc`/`std`-based) representation, or both, from a
  single field/variant definition (`&str` becomes `String`, `RefVec<'i, T>` becomes `Vec<T>`, etc).
- Support `#[default = ..]` field attributes so newer and older versions of a type stay wire-compatible.
- Let you trade away some forward compatibility for a smaller wire size, via `final_structure` / `self_describing` /
  `sized`.

This page goes through every argument the attribute accepts. All examples were checked against the current macro
implementation, so they compile and run as shown.

## Prerequisites

All examples below assume that wire_weaver dependency is added in Cargo.toml: `wire_weaver = "0.5.0"`.

## Quick start

```rust
use wire_weaver::prelude::*;

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct CoordV1 {
    x: u8,
    y: u8,
}

fn derive_on_struct() {
    let mut buf = [0u8; 64];
    let coord = CoordV1 { x: 0xAA, y: 0xCC };
    let bytes = coord.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, &[0xAA, 0xCC]);

    let des = CoordV1::from_ww_bytes(bytes).unwrap();
    assert_eq!(des, coord);
}
```

Two things to notice, both explained in detail below:

- Regular derives (`Debug`, `PartialEq`, ...) are requested with a `derive(..)` _argument_ to `derive_shrink_wrap`,
  not with a separate `#[derive(..)]` attribute. That's because `derive_shrink_wrap` can emit more than one item (a
  borrowed struct and an owned struct) from one definition; a plain `#[derive(..)]` attribute would only ever decorate
  whichever single item follows it in the source, not each generated type.
- `CoordV1` has no lifetime and no `alloc` fields, so the macro cannot tell whether you meant a borrowed or an owned
  type - that's why `borrowed` is spelled out explicitly here. See [Borrowed, owned, or both?](#borrowed-owned-or-both)
  below.

## Evolving types

`#[default = ..]` on a field marks it as added in a later version: newer and older versions of the type read each
other's data. Fields with a default go after all the others.

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct CoordV1 {
    x: u8,
    y: u8,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct CoordV1_1 {
    x: u8,
    y: u8,
    #[default = None]
    z: Option<u8>,
}

fn evolved_struct() {
    let mut buf = [0u8; 64];
    let coord = CoordV1_1 { x: 0xAA, y: 0xCC, z: Some(0xFF) };
    let bytes = coord.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, &[0xAA, 0xCC, 0x80, 0xFF]);

    // newer type from older data
    let coord: CoordV1_1 = CoordV1_1::from_ww_bytes(&[0xAA, 0xCC]).unwrap();
    assert_eq!(coord, CoordV1_1 { x: 0xAA, y: 0xCC, z: None });

    // older type from newer data
    let old_coord: CoordV1 = CoordV1::from_ww_bytes(bytes).unwrap();
    assert_eq!(old_coord, CoordV1 { x: 0xAA, y: 0xCC });
}
```

This works for as long as the type stays `Unsized` (the default - see [Size assumptions](#size-assumptions-final_structure-self_describing-sized)
below).

A field with a default reads as its default only when it starts at the end of the data and its read runs out of data.
A field that starts with data left is read as usual: a truncated or malformed value is an error, never replaced with
the default. The default can be any expression of the field's type (`#[default = 7]`, `#[default = Vec::new()]`,
`#[default = Default::default()]`).

!!! note "Padding bits read as 0"
    The last byte of older data can have unused bits (the rest of a byte after a `bool`, or the nibble next to a size
    at the back of the buffer). A new bit-sized field there (`bool`, nibble, the flag of an `Option` or `Result`) is
    read from them as 0, newer data packs it the same way. So give such fields their zero value as the default:
    `false`, `None`. An `Option` with `#[default = Some(..)]` reads as `None` or the default depending on where the
    old data ends.

### Enum variants from a newer version

A reader that gets a discriminant it doesn't know fails with `Error::EnumFutureVersionOrMalformedData`. An enum that
will grow (errors, modes) ends with a `#[future_version]` variant instead: it holds any discriminant this version
doesn't know, so an older host reports a newer device's error instead of failing to decode the reply, and can pass
the value on.

Every `Unsized` enum (the default) gets it without writing it (SW-47): the derive adds `Unknown(u32)` as if it were
written last. So this

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = unib32)]
enum IoError {
    InUse,
    Refused,
}
```

is the same type as

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = unib32)]
enum IoError {
    InUse,
    Refused,
    /// A variant from a newer version of this type, with its discriminant.
    #[future_version]
    Unknown(u32),
}
```

| Enum | Gets the variant |
|---|---|
| `Unsized` with a free discriminant in its `ww_repr` | yes, as `Unknown(u32)` |
| [`no_future_version`](#no_future_version-future_version-name-enums-only) | no: complete by nature (`Level`, `Parity`), or its readers must fail on a newer value |
| `future_version = Newer` | yes, as `Newer(u32)`: needed when `Unknown` is a variant of its own |
| every discriminant of the repr taken (`ww_repr = u1` with two variants) | no |
| `final_structure`, `self_describing`, `sized` | only when written: they can't skip a newer variant's fields |
| written `#[future_version] X(u32)` | as written |

- The variant is last, has one `u32` field (the discriminant), no discriminant of its own and no `#[cfg]`. It takes
  no number on the wire: the next version adds `NoIoLevel` as 2 before it, and an older reader gets `Unknown(2)`.
- The enum's bytes, `ELEMENT_SIZE` and `MAX_SIZE` are the same as without it, so it can be added in any version.
- New variants may carry data only in an `Unsized` enum (the default): its size slot skips their fields. A `sized` or
  `self_describing` enum has no size slot, there only new unit variants can be read past.
- Writing `Unknown(d)` writes the discriminant `d` without fields, so a newer reader gets the variant back if it has
  no fields. A `d` that doesn't fit `ww_repr` or is a known variant's gives `Error::SubtypeOutOfRange`.
- `discriminant()` returns the wire discriminant, also for `Unknown(d)`. See [evolution rules](../evolution/rules.md) for the full picture, and the
[evolution checker tool](../evolution/checker_tool.md) for automatically verifying compatibility across versions.

## Borrowed, owned, or both?

Before looking at the individual arguments, it helps to know how `derive_shrink_wrap` decides what to generate,
since several arguments (`borrowed`, `owned`, `derive_borrowed`, `cfg_attr_owned`, ...) only make sense in that
context.

The macro looks at the fields/variants of the type it's attached to and classifies it into one of three kinds:

| Kind             | Detected when                                                                                                                   | What gets generated                                                                                                                                                                                  |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Implied borrowed | Any field has a lifetime (`&'i str`, `RefVec<'i, T>`, `UserType<'i>`, ...) and no `alloc` type (`String`, `Vec`, `Box`) is used | The type itself, as written, is the borrowed type. If `owned` is also requested, `{Name}Owned` is generated by mapping borrowed types to their owned equivalents (see [table](#type-mapping) below). |
| Explicit owned   | The type's name ends in `Owned` and no field has a lifetime                                                                     | The type itself, as written, is the owned type. If `borrowed` is also requested, `{Name}` (with the `Owned` suffix stripped) is generated by mapping owned types back to borrowed ones.              |
| Ambiguous        | Every field is a plain type (`u8`, `bool`, another plain type, ...) - no lifetimes, no `alloc` types                            | Exactly **one** type is emitted (no renaming). You must say whether it should implement the borrowed or the owned traits (or both) via the `borrowed`/`owned` arguments.                             |

Mixing the two styles (a lifetime _and_ an `alloc` type, or an `Owned`-suffixed name that still has a lifetime) is a
compile error asking you to pick one, since it would otherwise be ambiguous which type is "the" borrowed/owned one.

```rust
// Implied borrowed: has a lifetime, no `owned` requested -> only `Coords<'i>` is generated.
#[derive_shrink_wrap]
struct Coords<'i> {
    points: RefVec<'i, u8>,
}

// Ambiguous: no lifetime, no alloc type -> must say which trait(s) to derive.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct CoordV1 {
    x: u8,
    y: u8,
}
```

The [zero-copy and owned types](#zero-copy-and-owned-types) section below has a full example of the borrowed+owned
case.

## Attribute arguments

`#[derive_shrink_wrap(directive, directive, ..)]` accepts a comma separated list of directives. Order doesn't matter
to the parser, but the codebase consistently writes them in this order (and so do the examples below):

```text
borrowed, owned, cfg_attr*, derive*, non_exhaustive, final_structure|self_describing|sized, ww_repr, discriminants,
display, no_future_version|future_version
```

### `borrowed`, `borrowed(<cfg predicate>)`

Requests generation of the borrowed type. Only needed for an [Ambiguous](#borrowed-owned-or-both) type (where it's
otherwise not generated at all), or to gate an implied-borrowed type's generation behind a `cfg` predicate:

```rust
// Ambiguous type - `borrowed` is what makes generation happen at all.
#[derive_shrink_wrap(borrowed)]
struct Flags {
    a: bool,
    b: bool,
}

// Implied borrowed type - already generated unconditionally; `borrowed(..)` restricts *when*.
#[derive_shrink_wrap(borrowed(feature = "defmt"), owned(feature = "std"))]
struct Name<'i> {
    value: &'i str,
}
```

### `owned`, `owned(<cfg predicate>)`

Enables generation of the owned type, converting borrowed fields to their owned equivalents (see the
[type mapping](#type-mapping) table). Bare `owned` generates it unconditionally; `owned(<cfg predicate>)` gates it
behind an arbitrary `cfg` predicate, most commonly a feature flag:

```rust
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug))]
pub struct FullVersion<'i> {
    pub crate_id: &'i str,
}
```

behind `feature = "std"`, this additionally generates:

```rust
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct FullVersionOwned {
    pub crate_id: String,
}
```

along with matching `SerializeShrinkWrapOwned`/`DeserializeShrinkWrapOwned` impls that produce/consume the exact same
bytes as the borrowed side.

### `cfg_attr(..)`, `cfg_attr_owned(..)`, `cfg_attr_borrowed(..)`

Splice a `#[cfg_attr(condition, attr, ..)]` verbatim onto the generated type(s) - the base one, the owned one only,
or the borrowed one only, respectively. Can be repeated. The contents are captured as-is and not interpreted by
`derive_shrink_wrap` itself, so any valid `cfg_attr` body works, e.g. for optional trait impls:

```rust
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    cfg_attr_owned(feature = "serde", derive(serde::Serialize, serde::Deserialize)),
    ww_repr = nib,
    sized
)]
pub enum Level {
    Low,
    High,
}
```

`defmt::Format` only ends up on `Level` (and only when the `defmt` feature is on), `serde`'s traits only on
`LevelOwned` (and only behind `feature = "serde"`).

### `derive(..)`, `derive_owned(..)`, `derive_borrowed(..)`

Extra derives (paths, so `core::fmt::Debug` works too) added to the base / owned-only / borrowed-only generated type,
exactly like Rust's own `#[derive(..)]`. `derive(..)` applies to whichever type(s) are actually generated; use
`derive_owned`/`derive_borrowed` when a trait only makes sense (or only compiles) on one side - e.g. `Copy`, which a
`String`/`Vec`-carrying owned type could never implement:

```rust
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Debug, PartialEq),
    derive_borrowed(Copy, Clone),
    final_structure
)]
struct Version<'i> {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
    pub label: Option<&'i str>,
}

fn borrowed_is_copy() {
    let v = Version { major: 1, minor: 2, patch: 3, label: None };
    let v2 = v; // fine, `Version` is `Copy` (via `derive_borrowed`)
    let _ = v; // still usable
    let _ = v2;
}
```

### Size assumptions: `final_structure`, `self_describing`, `sized`

By default, every generated type is `Unsized`: its serialized size isn't known up front, so when it's nested inside
another struct/enum, the parent stores a reverse-length for it (nibble-based, similar to UNib32, but in reverse order). This is what makes the
[evolution example](#evolving-types) above work - fields can be added or removed later without breaking anyone.

The three directives below (mutually exclusive - specifying more than one is a compile error) opt a type out of that
flexibility in exchange for a smaller wire size. They map directly onto `shrink_wrap`'s `ElementSize`:

| Directive         | `ElementSize`           | Meaning                                                                                                                                                              | Trade-off                                                                                                                 |
| ----------------- | ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| _(none)_          | `Unsized`               | Size unknown, stored as a reverse length in the parent.                                                                                                              | Fully evolvable.                                                                                                          |
| `final_structure` | `UnsizedFinalStructure` | Size still unknown, but the type is flattened onto its parent - no reverse length is stored for it at all; parent and this type share the same size-marker "budget". | Cannot be evolved, only the very first evolution can be marked `final_structure`, and it cannot be removed later.         |
| `self_describing` | `SelfDescribing`        | No size is stored; a reader can tell where the value ends from the encoding itself (e.g. `UNib32`'s own variable-length encoding).                                   | No overhead, but only applicable to types whose encoding is inherently self-delimiting.                                   |
| `sized`           | `Sized { size_bits }`   | Size is fixed and known at compile time (computed automatically from the fields - you don't specify a bit count). Nothing is stored for it, ever.                    | Cannot evolve at all: adding/removing/resizing a field changes the layout for every existing reader. Most compact option. |

!!! tip

    Even though a type can be `fnial_structure` or `sized`, it can still be evolved. But only by reusing previously unused gaps, which by default are set to 0. For example if `bool` is followed by `u8`, 7 bits are left unused and skipped by readers, which can later be reclaimed.

For an enum, only one variant is on the wire, so its size is the discriminant (see
[`ww_repr`](#ww_repr-repr-enums-only) below) plus the fields of _one_ variant, not of all of them:

- `sized` needs every variant to take the same number of bits (unit variants take 0), and a fixed size discriminant
  (anything but `unib32`). `ELEMENT_SIZE` is then `Sized { size_bits: discriminant + variant }`.
- Variants of different sizes make the enum `self_describing`: the discriminant tells how long the value is. Asking
  for `sized` is a compile error that names the variants and their sizes:

    ```text
    sized enum Reading: variants differ in size (Short: 8 bits, Long: 32 bits); use self_describing instead of
    sized, or pad the variants to one size
    ```

`sized` and `self_describing` produce the same bytes, for the type and for whatever contains it, so switching
between the two is not a wire change. Note that pairing an enum with `ww_repr` alone does **not** make it compact - without one of these three
directives it stays `Unsized`, exactly like a struct:

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u2)]
enum SpeedUnsized { Slow, Medium, Fast, Turbo }

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u2, sized)]
enum SpeedSized { Slow, Medium, Fast, Turbo }

// Wraps a `Sized` field - its 2 bits are known up front, so a field placed right after it
// can carry on packing into the very same byte.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct PayloadWithSized {
    speed: SpeedSized,
    after: bool,
}

// Wraps an `Unsized` field - its size isn't known up front, so the struct reserves a
// reverse-length marker for it, and everything that follows starts fresh on the next byte.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct PayloadWithUnsized {
    speed: SpeedUnsized,
    after: bool,
}

fn compact_vs_not() {
    let mut buf = [0u8; 4];
    // Sized, so (Slow, Turbo) packs into 2+2 = 4 bits of a single byte:
    let bytes = (SpeedSized::Slow, SpeedSized::Turbo).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, &[0x30]);

    // Unsized, standalone, needs no reverse length here since nothing follows it -
    // but as soon as it's nested next to another field, an extra size marker appears.
    let mut buf2 = [0u8; 4];
    let bytes2 = SpeedUnsized::Turbo.to_ww_bytes(&mut buf2).unwrap();
    assert_eq!(bytes2, &[0xC0]);

    // `SpeedSized` is 2 bits wide, so `after`'s bit packs right in after it, same byte:
    // 0b11_1_00000 - Turbo (11), then `after` (1), then unused padding.
    let mut buf3 = [0u8; 4];
    let sized_payload = PayloadWithSized { speed: SpeedSized::Turbo, after: true };
    let bytes3 = sized_payload.to_ww_bytes(&mut buf3).unwrap();
    assert_eq!(bytes3, &[0xE0]);

    // `SpeedUnsized` gives no such guarantee, so its byte gets padded on its own, and `after`
    // starts a new byte (with a reverse-length marker for `speed` sharing its low bits):
    let mut buf4 = [0u8; 4];
    let unsized_payload = PayloadWithUnsized { speed: SpeedUnsized::Turbo, after: true };
    let bytes4 = unsized_payload.to_ww_bytes(&mut buf4).unwrap();
    assert_eq!(bytes4, &[0xC0, 0x81]);
    // bool and reverse length end up in the same byte, and there are even 3 bits left, neat!
}
```

Whichever assumption you pick, the macro inserts a compile-time assertion checking that the type's _actual_ generated
`ELEMENT_SIZE` matches the one you asked for - if a field turns out to make the type `Unsized` after all, the build
fails instead of silently producing an incompatible layout. User types are counted wherever they are in a field
(`Point`, `[Point; 3]`, `(Point, u8)`, `Option<Point>`, `Range<Point>`): the macro can't see their size, so it emits a
const expression over their `ELEMENT_SIZE` and the compiler folds it.

`size_bits` is the sum of the fields' own bits. Alignment padding between fields depends on where the value starts
in the buffer and is not part of it: `struct { a: bool, b: u8 }` is `Sized { size_bits: 9 }` and takes 2 bytes
on its own.

### `fixed_layout`: fields at fixed offsets, and the fast path

shrink_wrap writes a value field after field. When no field's size depends on its value, every field starts at the
same place in every value: a header, a record in flash, a descriptor that is signed as bytes. Reading a field is
then a load at a constant offset, and the macro generates that next to the usual serdes code (SW-44, SW-45):

- `ENCODED_LEN`: the longest encoding, the buffer to have ready. `MIN_LEN`: the shortest. `fixed_len()`: the length
  of this value. All three are the same unless the last field has a variable length (see below);
- an offset const per field: `COUNT_OFFSET` in bytes, or for a bit field `URGENT_BIT_OFFSET` and `URGENT_BITS`,
  counted from the most significant bit of the first byte; for an enum `TAG_LEN`, and per variant `<VARIANT>_TAG`,
  `<VARIANT>_LEN` and `<VARIANT>_<FIELD>_..` (unnamed fields by their index); an `Option`'s flag is `<FIELD>_FLAG`;
- `from_fixed(&[u8])` and `to_fixed(&mut [u8])`: loads and stores (shifts and masks for bit fields) at those
  offsets, without a `BufReader` or `BufWriter`. The bytes are those of `ser_shrink_wrap`. A short buffer is an
  error, never a panic;
- the `FixedLayout` trait, whose `write_c` prints the C reader of the type: a define per offset and a `static inline`
  reader per field (`shrink_wrap::fixed_layout::write_c_header` writes a whole header).

What has a fixed offset:

| field | rule |
| --- | --- |
| `u8`..`u64`, `i8`..`i64`, `f32`, `f64`, arrays of them | start on a byte, little-endian, as `BufWriter` writes them |
| `bool`, `U1`..`U32`, `I2`..`I32`, `Nibble` | packed most significant bit first; a `Nibble` starts on a 4-bit boundary; the next byte-aligned field starts a fresh byte |
| another `fixed_layout` type | starts on a byte; it cannot share a byte with bit fields of the parent (a compile error asks for a byte-aligned field between them) |
| `Option<T>`, `Result<T, E>`, an enum whose variants differ in length | only as the **last** field: the fields before it keep their offsets, nothing after it could. The payload starts on a byte; the flag is an ordinary bit field, also where `#[flag]` moves it |

Not: lists, strings, `MaxLen`, varints (`UNib32` and friends), `RefBox`, `TailSize`, tuples, ranges, 128-bit numbers,
bit fields wider than 32 bits, `#[default]` fields. A struct must be `sized`, `final_structure` or `self_describing`;
an evolvable struct may grow, so it has no fixed layout. An enum must be `sized` or `self_describing` with a
byte-aligned tag (`ww_repr = u8`, `u16` or `u32`).

The rule for variable length is the same everywhere: an `Option`, a `Result` and an enum of unequal variants are
each a value whose length depends on what it holds. As a whole type, or as the last field of one, that is fine:
`fixed_len()` says how long this value is. Before another field it is not, because that field would move.

Types that qualify get the fast path without asking. The `fixed_layout` directive makes it a requirement: a field
that breaks it is a compile error that says why and what to use instead, for example "field `count`: a
variable-length number takes more bytes for larger values, so the fields after it have no fixed offset. Use u32 /
u64, or U1..U32 for fewer bits". Only with the directive may a field be another fixed-layout type (the macro cannot
look inside a type it does not see; the compiler checks that it implements `FixedLayout`). On a struct the directive
also picks the size assumption when none is given: `sized`, or `self_describing` when the last field is an `Option`
or `Result`. A struct whose last field is an enum of unequal variants says `self_describing` itself.

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

On a Cortex-M0, decoding a header of eight integers takes 276 bytes of code this way and 484 through `BufReader`; a
bit-packed sample 276 instead of 652 ([code size](code_size.md#fixed-layout-fast-path)).

### `head_tail`: a fixed head and a sequential tail

For types with lists, strings or options in the middle, where no fixed layout exists, `head_tail` picks a wire
format of its own (SW-46), made for the smallest reader on a microcontroller:

- the **head** has a length known at compile time and holds every fixed-width field at a constant offset, as
  `fixed_layout` places them, and for each variable field what is fixed about it: a `u16` count for a list or
  string, the flag bit of an `Option` or `Result`, the head of a nested `head_tail` or `fixed_layout` type;
- the **tail** follows the head and holds the variable parts in field order: list items, string bytes, option
  payloads, the tails of nested types.

An enum is its tag (`ww_repr = u8`, `u16` or `u32`), then the head and tail of its variant. Reading the head is loads
at constant offsets; the tail is read front to back by a cursor that only takes lengths it already knows (`Tail`).
There are no lengths at the back, no `BufReader`, no per-type generic reader.

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

A list is a `shrink_wrap::head_tail::List<'i, T>`: `List::new(&items)` to write, and read from the wire it keeps the
bytes and reads an item when its iterator reaches it. Its items are numbers or `fixed_layout` types of one length.
`Option` / `Result` payloads are numbers, arrays of them, or `head_tail` / `fixed_layout` types. Every
`fixed_layout` type implements `HeadTail` (all head when it has one length, else all tail), so the types of a
protocol mix freely.

It is one type with one format: a `head_tail` type has no `SerializeShrinkWrap` / `DeserializeShrinkWrap` (and no
owned version), so it cannot be a field of an ordinary shrink_wrap type or a ww API argument. `write_c_ht` prints
the C reader: head defines and readers as for `fixed_layout`, and per type a `<name>_tail_t` and a
`<name>_tail()` that walks the tail and points at each variable part
(`shrink_wrap::head_tail::write_c_header` writes a whole header).

**Views and writers** (SW-52), for firmware that should not make the whole value first. `Utx::view(&bytes)?` checks
the lengths and walks the tail once (counts and flags are the only fields it reads) and gives a `UtxView<'_>`: a
reference to the head whose accessors (`v.ch()`, `v.defer()`) read each field at its offset when called, and the
variable parts found on the way (`v.perr()` a `List`, `v.glitch()` an `&Option<Wave>`, a nested `head_tail` type
its own view). An enum's view is an enum of its variants' views: `RequestView::Stop(v)` with `v.mask()`, a tuple
variant of one nested type holds that type's view (`RequestView::Utx(UtxView)`). Each enum variant also gets a
writer, `Reply::write_run(&mut tx, run, started, mark, lead, mark_line)?`, which writes the same bytes as
`to_head_tail` of that variant without making the value. Both come from the code of `from_head_tail` /
`to_head_tail` and are tested against it byte for byte. View types are always `pub` (the `View` of the public
`HeadTail` trait).

Enum variants may have a `#[cfg]` (SW-54): a variant compiled out keeps its tag, and a build without it reads that
tag as an unknown one. One protocol enum can then serve firmware images that each handle a part of it.

On busgen's request and reply enums the codec takes 4590 bytes on a Cortex-M0 through values and 3894 through views
and writers, a hand-written one 4408 and the derived shrink_wrap one 6652 ([code size](code_size.md#head-and-tail)).

### `ww_repr = <repr>` (enums only)

Sets the wire representation of an enum's discriminant. **Required** for every enum - structs must not set it.
Accepted values: `u1`..`u32` (or `ub<N>` for arbitrary bit widths), `nib` (4-bit nibble), `unib32` (`UNib32`,
variable-length, 1 or more nibbles), and the byte-aligned `u8`/`u16`/`u32`. Pick the smallest one that fits the number
of variants you expect to ever need - it directly determines the discriminant's on-wire size (and, combined with
`sized`, the whole enum's size):

```rust
// Fits in 2 bits (up to 4 variants) - future variants beyond 4 would be a breaking change.
#[derive_shrink_wrap(borrowed, derive(Debug), ww_repr = u2, sized)]
enum Pull { None, Up, Down, UpDown }

// Variable-length - room to grow past the initial variant count without repr changes.
#[derive_shrink_wrap(borrowed, derive(Debug), ww_repr = unib32, self_describing)]
enum ErrorCode { NotFound, Timeout, /* ... */ }
```

Every generated enum also gets an inherent `fn discriminant(&self) -> <repr's native type>` method regardless of
whether [`discriminants`](#discriminants-enums-only) is used.

### `#[cfg(..)]` on enum variants

A variant can be compiled out, typically behind a cargo feature, when only some targets need it: firmware enables
the few it uses, a host enables all of them.

```rust
#[derive_shrink_wrap(borrowed, non_exhaustive, self_describing, ww_repr = unib32)]
pub enum Number {
    U8(u8),
    #[cfg(feature = "f32")]
    F32(f32),
    #[cfg(feature = "u128")]
    U128(u128),
}
```

- **The wire format does not depend on it.** Discriminants are numbered over all variants, `U128` is 2 with or
  without `f32`, so two builds with different features agree on every variant they share.
- The variant, its serializing and deserializing code and its part of the size exist only where the predicate
  holds: without `u128` the enum above is 8 bytes in memory instead of 32, and no 128-bit code is linked.
- Reading a variant that is compiled out gives `Error::EnumVariantDisabled` (an unknown number is still
  `EnumFutureVersionOrMalformedData`).
- A `sized` enum needs at least one variant without `#[cfg]`, so its size cannot change with features.
- Cargo features are unified across a build: a crate that matches such an enum without a `_` arm stops compiling
  when another crate enables one more variant. The `non_exhaustive` argument puts `#[non_exhaustive]` on the
  generated types and makes the `_` arm mandatory outside the defining crate.

`ww_numeric::NumericValue` is built this way. `#[cfg]` on fields is not supported: it would change the bytes.

### `crate_path(<path>)`

Generated code refers to `shrink_wrap` items by path, so neither `use shrink_wrap::prelude::*` nor any other
import is needed for the macro, and the names it uses (`BufReader`, `Error`, `ElementSize`, ...) never clash with
the module's own. The macro finds the crate in `Cargo.toml`: `::shrink_wrap` (or whatever it is renamed to), or
`::wire_weaver::shrink_wrap` when only `wire_weaver` is a dependency. `crate_path(..)` overrides that, for a crate
that reaches `shrink_wrap` through some other re-export:

```rust
#[derive_shrink_wrap(borrowed, crate_path(my_framework::shrink_wrap))]
struct Point { x: u8, y: u8 }
```

When neither crate is a dependency and nothing is given, the names are left unqualified and the prelude import is
needed, as it was before.

### `no_future_version`, `future_version = <Name>` (enums only)

An `Unsized` enum gets `Unknown(u32)` for discriminants from a newer version (see
[Enum variants from a newer version](#enum-variants-from-a-newer-version)). `no_future_version` leaves it out, for an
enum that is complete by nature or whose readers must fail on a value they don't know (a request kind a device can't
act on). `future_version = <Name>` names the variant, when `Unknown` is taken:

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u2, no_future_version)]
enum Parity {
    None,
    Even,
    Odd,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u8, future_version = Newer)]
enum Reason {
    Unknown,
    User,
    // Newer(u32) is added
}
```

Either is an error next to a written `#[future_version]` variant, and `future_version = <Name>` on an enum that can't
get one (not `Unsized`, or every discriminant taken).

### `discriminants` (enums only)

Additionally generates a fieldless `{Name}Discriminants` enum, carrying only the variants (no payloads) plus its own
`.discriminant()` method. It's for the case where you need to commit to _which_ variant you're writing before you've
built that variant's payload - e.g. a streaming/builder API that reserves space for the discriminant, keeps writing,
and only decides the final variant afterward:

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Eq), ww_repr = nib, discriminants)]
enum EventKind<'i> {
    Value { data: RefVec<'i, u8> },
    Written,
}

fn discriminant_ahead_of_time() {
    // `EventKindDiscriminants` can be produced (and its wire discriminant read) without ever
    // constructing an `EventKind::Value`'s `data` field.
    let placeholder = EventKindDiscriminants::Value;
    let full = EventKind::Value { data: RefVec::new_bytes(&[1, 2, 3]) };
    assert_eq!(placeholder.discriminant(), full.discriminant());
}
```

This is exactly how `ww_client_server`'s response builder writes a placeholder discriminant up front, then goes back
and patches it in once the actual variant being sent is known.

### `display` (DER-12)

Generates `core::fmt::Display` and `core::error::Error` (a bare `impl`, no `source()`) for the borrowed side, the
owned side, or both, whichever `owned`/`borrowed` generate. The message is the first doc line: a struct's own, or
each variant's own for an enum. `{field}` is filled in from the type's own field of that name, format specs work
too (`{field:#x}`); a tuple variant's fields are `{0}`, `{1}`, ... `#[display("..")]` overrides the doc line, for a
tuple variant's fields or when the doc line should say more than the message:

```rust
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), display, ww_repr = unib32)]
pub enum IoError<'i> {
    /// Not supported by this device.
    Unsupported,
    /// In use by {by}.
    InUse { by: &'i str },
    /// Hardware fault {code:#x}.
    Fault { code: u32 },
    #[display("wrapping: {0}")]
    Wrapped(u8),
}

fn display_message() {
    assert_eq!(IoError::Unsupported.to_string(), "Not supported by this device.");
    assert_eq!(IoError::InUse { by: "USART2" }.to_string(), "In use by USART2.");
    assert_eq!(IoError::Fault { code: 0xBEEF }.to_string(), "Hardware fault 0xbeef.");
    assert_eq!(IoError::Wrapped(7).to_string(), "wrapping: 7");
    // the owned side gets it too
    assert_eq!(IoErrorOwned::Unsupported.to_string(), "Not supported by this device.");
}
```

A variant with neither a doc comment nor `#[display("..")]` is a compile error naming it, so a message is never
silently missing; the `#[future_version]` variant (SW-47) always has one, written or not. A field not mentioned by
the message is simply unused, not an error. `display` is unused on the device if nothing calls it (no `Display` or
`Error` method is ever invoked there), so the linker drops it - there is no `no_std` downside to turning it on for
an API's error types.

## Zero-copy and owned types

Often there is a need to serialize an owned type into a buffer and deserialize it as a borrowed type on `no_std`
without allocation, or vice versa. Typing out two definitions by hand - one using borrowed data (`RefVec<'i, T>`,
`&'i str`, etc.) and one owned - would be very error-prone. `owned(<cfg predicate>)` (see
[above](#owned-ownedcfg-predicate)) generates the owned definition and its serdes code automatically.

Pseudo-code usage example:

```rust
fn round_trip() {
    // on no_std, no allocator
    let v = FullVersion { .. };
    let bytes = v.to_ww_bytes(&mut buf).unwrap();

    // on host, with allocator
    let v_owned = FullVersionOwned::from_ww_bytes_owned(bytes).unwrap();
    assert_eq!(v.to_owned(), v_owned);

    let bytes_from_owned = v_owned.to_ww_bytes_owned().unwrap();
    assert_eq!(bytes, bytes_from_owned);

    // again on no_std
    let v_ref = FullVersion::from_ww_bytes(bytes_from_owned).unwrap();
    assert_eq!(v, v_ref);
}
```

### Type mapping

| Borrowed type    | Owned equivalent |
| ---------------- | ---------------- |
| `RefVec<'i, u8>` | `Vec<u8>`        |
| `&'i str`        | `String`         |
| `RefBox<'i, T>`  | `Box<T>`         |
| `MaxLen<RefVec<'i, T>, N>` | `MaxLen<Vec<T>, N>` |
| `MaxLen<&'i str, N>` | `MaxLen<String, N>` |
| `UserType<'i>`   | `UserTypeOwned`  |
| `UserType<'i, T>` | `UserTypeOwned<T>` |

Generic arguments of a user type are emitted as written (`Twice<u8>` stays `Twice<u8>` on both sides); only a
leading lifetime is replaced by the generated type's and dropped in the owned variant. The borrowed/owned pairs
of `shrink_wrap` itself follow this scheme too (`Delta<'i, T>` / `DeltaOwned<T>`, `XorFloat<'i, T>` /
`XorFloatOwned<T>`).

Array lengths can be any const expression (`[f32; MAX_SERIES]`, `[u8; N * 2]`), not only literals; a `sized` type's
`ELEMENT_SIZE` carries them symbolically.

`MaxLen<T, N>` (a list or string of at most `N` elements, `N` any const expression) has the same bytes as `T` and is
checked on serialize and deserialize, see [bounded lists and strings](../types.md#bounded-lists-and-strings-maxlent-n).
The borrowed type gets a `MAX_SIZE` (worst-case serialized size, `Some` when every field is bounded) next to
`ELEMENT_SIZE`; the owned one does not have it yet.

`TailSize<N>` is the same on both sides: a slot the macro fills in with the size of the rest of the value, see
[the wire format page](shrink_wrap.md#size-of-the-rest-of-a-value-tailsizen). It is only allowed in types without a
size assumption.

# Next step

Check out [API overview](../api/overview.md).
