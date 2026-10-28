---
level: advanced
---

# Derive reference

Every directive `#[derive_shrink_wrap(..)]` accepts, and the attributes it reads on fields and variants. Start with
[Derive](derive.md) for the basics.

The order of directives doesn't matter to the parser, but the codebase consistently writes them in this order (and
so do the examples):

```text
borrowed, owned, cfg_attr*, derive*, non_exhaustive, final_structure|self_describing|sized|sparse|transparent, ww_repr,
discriminants, display, validate, series|series(no_store)|series(cells), no_future_version|future_version
```

| Directive                                          | Section                                                                     |
|----------------------------------------------------|-----------------------------------------------------------------------------|
| `borrowed`, `owned`                                 | [Generated types](#generated-types)                                          |
| `cfg_attr..`, `derive..`, `non_exhaustive`          | [Attributes on the generated types](#attributes-on-the-generated-types)      |
| `final_structure`, `self_describing`, `sized`       | [Size assumptions](size_assumptions.md)                                      |
| `fixed_layout`, `head_tail`, `sparse`               | [Fixed and sparse layouts](layouts.md)                                       |
| `transparent`                                       | [`transparent`](#transparent)                                                |
| `ww_repr`, `discriminants`, `no_future_version`, `future_version` | [Enums](#enums)                                               |
| `display`, `validate`                               | [Messages and checks](#messages-and-checks)                                  |
| `series`, `series(no_store)`, `series(cells)`       | [`series`](#series)                                                          |
| generics (`struct Timed<T>`, `Buf<const N: usize>`) | [Generics](#generics)                                                        |
| `crate_path`                                        | [`crate_path`](#crate_pathpath)                                              |
| `#[wire(W)]` on a field                             | [`#[wire(W)]` field attribute](#wirew-field-attribute)                      |
| `#[default]`, `#[flag]`, `#[slot]`, `#[future_version]`, `#[cfg]` | [Field and variant attributes](#field-and-variant-attributes) |

## Generated types

Which types the macro generates follows from the fields: see
[borrowed, owned, or both?](derive.md#borrowed-owned-or-both).

### `borrowed`, `borrowed(<cfg predicate>)`

Requests the borrowed type. Only needed for an [ambiguous](derive.md#borrowed-owned-or-both) type (where it's
otherwise not generated at all), or to gate an implied-borrowed type behind a `cfg` predicate:

```rust
// Ambiguous type: `borrowed` is what makes generation happen at all.
#[derive_shrink_wrap(borrowed)]
struct Flags {
    a: bool,
    b: bool,
}

// Implied borrowed type: generated anyway; `borrowed(..)` restricts *when*.
#[derive_shrink_wrap(borrowed(feature = "defmt"), owned(feature = "std"))]
struct Name<'i> {
    value: &'i str,
}
```

### `owned`, `owned(<cfg predicate>)`

Generates the owned type, converting borrowed fields to their owned equivalents (see the
[type mapping](derive.md#type-mapping)). Bare `owned` generates it unconditionally; `owned(<cfg predicate>)` gates
it, most commonly behind a feature:

```rust
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug))]
pub struct FullVersion<'i> {
    pub crate_id: &'i str,
}
```

With `feature = "std"`, this additionally generates

```rust
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct FullVersionOwned {
    pub crate_id: String,
}
```

along with `SerializeShrinkWrapOwned` / `DeserializeShrinkWrapOwned` impls that produce and consume the exact same
bytes as the borrowed side. The borrowed type has a `MAX_SIZE` (worst-case serialized size, `Some` when every field
is bounded, see [`MaxLen`](../types.md#worst-case-size-at-compile-time)); the owned one doesn't have it yet.

## Attributes on the generated types

### `derive(..)`, `derive_owned(..)`, `derive_borrowed(..)`

Extra derives (paths, so `core::fmt::Debug` works too) for the base, owned-only or borrowed-only generated type,
exactly like Rust's own `#[derive(..)]`. `derive(..)` applies to whichever types are generated; use
`derive_owned` / `derive_borrowed` when a trait only makes sense (or only compiles) on one side, e.g. `Copy`, which
an owned type with a `String` could never implement:

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

### Outer attributes

Attributes written next to `#[derive_shrink_wrap(..)]` (`#[derive(..)]`, `#[cfg_attr(..)]`, `#[allow(..)]`) stay on
the generated type that has the item's own name: the borrowed one for `Type<'i>`, `TypeOwned` when that is what is
written. The owned mirror of a borrowed type gets its derives through `derive_owned(..)`. So the derive can sit behind
an optional feature with the plain derives written once:

```rust
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), derive_owned(Clone, Debug, PartialEq), final_structure)
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Counted<'i> {
    pub name: &'i str,
    pub exp: i8,
}
```

On an enum, `#[repr(..)]` is left out: `ww_repr` sets the enum's representation.

### `cfg_attr(..)`, `cfg_attr_owned(..)`, `cfg_attr_borrowed(..)`

Splice a `#[cfg_attr(condition, attr, ..)]` verbatim onto the base, owned-only or borrowed-only generated type. Can
be repeated. The contents are not interpreted by the macro, so any valid `cfg_attr` body works, e.g. for optional
trait impls:

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

`defmt::Format` ends up only on `Level` (and only with the `defmt` feature), `serde`'s traits only on `LevelOwned`
(and only with `feature = "serde"`).

### `non_exhaustive`

Puts `#[non_exhaustive]` on the generated types, so a `match` outside the defining crate needs a `_` arm. Use it
together with [`#[cfg]` on variants](#cfg-on-enum-variants).

## Enums

### `ww_repr = <repr>` (enums only)

Sets the wire representation of an enum's discriminant. **Required** for every enum; structs must not set it.

| `ww_repr`              | Discriminant                                 |
|------------------------|----------------------------------------------|
| `u1`..`u32`, `ub<N>`   | that many bits                               |
| `nib`                  | a 4-bit nibble                               |
| `u8`, `u16`, `u32`     | byte-aligned                                 |
| `unib32`               | `UNib32`, variable length, 1 or more nibbles |

Pick the smallest one that fits the number of variants you expect to ever need: it directly determines the
discriminant's size (and, with `sized`, the whole enum's size):

```rust
// Fits in 2 bits (up to 4 variants): a fifth variant would be a breaking change.
#[derive_shrink_wrap(borrowed, derive(Debug), ww_repr = u2, sized)]
enum Pull { None, Up, Down, UpDown }

// Variable length: room to grow past the initial variant count without a repr change.
#[derive_shrink_wrap(borrowed, derive(Debug), ww_repr = unib32, self_describing)]
enum ErrorCode { NotFound, Timeout, /* ... */ }
```

Every generated enum gets an inherent `fn discriminant(&self) -> <repr's native type>`, whether or not
[`discriminants`](#discriminants-enums-only) is used. How to pick a width, and when an `Option<bool>` beats an enum:
[showcase](showcase.md#picking-an-enums-discriminant-width-and-when-optionbool-beats-it).

### `no_future_version`, `future_version = <Name>` (enums only)

An evolvable enum gets `Unknown(u32)` for discriminants from a newer version (see
[enum variants from a newer version](derive.md#enum-variants-from-a-newer-version)). Which enums get it:

| Enum                                                                       | Gets the variant                                                                       |
|----------------------------------------------------------------------------|----------------------------------------------------------------------------------------|
| `Unsized` (the default) with a free discriminant in its `ww_repr`           | yes, as `Unknown(u32)`                                                                 |
| `no_future_version`                                                        | no: complete by nature (`Level`, `Parity`), or its readers must fail on a newer value |
| `future_version = Newer`                                                   | yes, as `Newer(u32)`: needed when `Unknown` is a variant of its own                    |
| every discriminant of the repr taken (`ww_repr = u1` with two variants)    | no                                                                                     |
| `final_structure`, `self_describing`, `sized`                              | only when written: they can't skip a newer variant's fields                            |
| written `#[future_version] X(u32)`                                         | as written                                                                             |

`no_future_version` leaves it out, for an enum that is complete by nature or whose readers must fail on a value they
don't know (a request kind a device can't act on). `future_version = <Name>` names the variant, when `Unknown` is
taken:

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

Rules for the variant, written or generated:

- it is last, has one `u32` field (the discriminant), no discriminant of its own and no `#[cfg]`;
- the enum's bytes, `ELEMENT_SIZE` and `MAX_SIZE` are the same as without it;
- new variants may carry data only in an `Unsized` enum: its size slot skips their fields. A `sized` or
  `self_describing` enum has no size slot, there only new unit variants can be read past;
- writing `Unknown(d)` writes the discriminant `d` without fields, so a newer reader gets the variant back if it has
  no fields. A `d` that doesn't fit `ww_repr` or is a known variant's gives `Error::SubtypeOutOfRange`.

Either directive is an error next to a written `#[future_version]` variant, and `future_version = <Name>` is an
error on an enum that can't get one (not `Unsized`, or every discriminant taken).

### `niche` (enums only)

The generated enum gets no `#[repr]`, so Rust is free to keep the tag in a niche of a variant's field. Without it,
the derive sets the enum's repr to the native type of its `ww_repr` (`u32` for `unib32`), which makes the tag that
wide in memory. The bytes on the wire are the same either way, and so is `discriminant()` (a `match` instead of a
read of the tag):

```rust
// a one-byte handle with spare values
#[derive_shrink_wrap(borrowed, sized, ww_repr = u4)]
enum Handle { A, B, C }

#[derive_shrink_wrap(borrowed, self_describing, ww_repr = unib32, niche)]
enum Prefix { Unit, Milli, Kilo, Exact(Handle) }

// without `niche`: a u32 tag and the handle, 8 bytes
assert_eq!(size_of::<Prefix>(), 1);
```

Rust fills a niche when one variant carries data and the others don't (or when the data of all of them fits around
the same niche); otherwise the enum is as large as a tagged one, never larger. Variants don't get explicit
discriminant values in Rust, so `as` casts of the enum don't compile; read the wire discriminant through
`discriminant()`. `exact_units::Prefix` uses it: a scale chosen at run time in one byte. Tests `tests/niche.rs`.

### `discriminants` (enums only)

Also generates a fieldless `{Name}Discriminants` enum, with only the variants (no payloads) and its own
`.discriminant()` method. It's for committing to _which_ variant you're writing before building that variant's
payload, e.g. a streaming or builder API that reserves space for the discriminant, keeps writing, and only decides
the final variant afterward:

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

This is how `ww_client_server`'s response builder writes a placeholder discriminant up front, then patches it once
the actual variant is known (see the
[showcase](showcase.md#patching-a-discriminant-after-youve-already-started-writing-its-payload)).

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

- **The wire format does not depend on it.** Discriminants are numbered over all variants: `U128` is 2 with or
  without `f32`, so two builds with different features agree on every variant they share.
- The variant, its code and its part of the size exist only where the predicate holds: without `u128` the enum
  above is 8 bytes in memory instead of 32, and no 128-bit code is linked.
- Reading a variant that is compiled out gives `Error::EnumVariantDisabled` (an unknown number is still
  `EnumFutureVersionOrMalformedData`).
- Introspection, API snapshots and type signatures list every variant whatever features are on: they are made from
  the source, not from the compiled type. A firmware built with a few variants and a host built with all of them
  have the same definition and connect (`tests/cfg_variants`); a call with a variant the device lacks fails there
  with `ArgsDesFailed`.
- A `sized` enum needs at least one variant without `#[cfg]`, so its size can't change with features.
- Cargo features are unified across a build: a crate that matches such an enum without a `_` arm stops compiling
  when another crate enables one more variant. [`non_exhaustive`](#non_exhaustive) makes the `_` arm mandatory
  outside the defining crate.

`ww_numeric::NumericValue` is built this way. `#[cfg]` on fields is not supported: it would change the bytes.

## Messages and checks

### `display`

Generates `core::fmt::Display` and `core::error::Error` (a bare `impl`, no `source()`) for whichever of the
borrowed and owned types are generated (DER-12). The message is the first doc line: a struct's own, or each
variant's own for an enum.

- The trailing period is dropped (Rust's convention for error messages, so it reads well inside a longer one); `...`
  is kept.
- `{field}` is filled in from the type's own field of that name; format specs work too (`{field:#x}`). A tuple
  struct's or tuple variant's fields are `{0}`, `{1}`, ...
- `#[display("..")]` overrides the doc line, used as written: for a tuple variant's fields, or when the doc line
  should say more than the message (error enums in APIs: [Errors](../api/errors.md)).

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
    assert_eq!(IoError::Unsupported.to_string(), "Not supported by this device");
    assert_eq!(IoError::InUse { by: "USART2" }.to_string(), "In use by USART2");
    assert_eq!(IoError::Fault { code: 0xBEEF }.to_string(), "Hardware fault 0xbeef");
    assert_eq!(IoError::Wrapped(7).to_string(), "wrapping: 7");
    // the owned side gets it too
    assert_eq!(IoErrorOwned::Unsupported.to_string(), "Not supported by this device");
}
```

A variant with neither a doc comment nor `#[display("..")]` is a compile error naming it, so a message is never
silently missing; the `#[future_version]` variant always has one, written or not. A field the message doesn't
mention is simply unused. On a device, `display` costs nothing if nothing calls it: the linker drops it, so there is
no `no_std` downside to turning it on for an API's error types.

### `validate = path`

Structs only (not `sparse`, `fixed_layout` or `head_tail`), SW-58. `path` is a function
`fn(&T) -> Result<(), shrink_wrap::Error>` called on every value just read, before it is returned; its error is the
result of the read. That includes every item read from a `Series` (with `series`).

Use it for a range or invariant that the field types can't express, while keeping the derived type, which is what the
API model describes to dynamic clients (`ww introspect`). With both a borrowed and an owned type generated, the
function is called with each, so it has to accept both (a type that is the same on both sides, like
`ww_date_time::Year`, needs one function). The API model parser ignores the directive.

```rust
#[derive_shrink_wrap(self_describing, validate = Year::validate)]
pub struct Year { shifted: UNib32 }

impl Year {
    fn validate(&self) -> Result<(), shrink_wrap::Error> { /* ... */ }
}
```

## `series`

SW-60. Also implements `shrink_wrap::series::SeriesColumns` for the type (borrowed and owned), so that it can be
the item of a `Series<T>` (a list written column by column) or a field of one. A struct's fields are columns in
declaration order, an enum is a run of its tags and then each data variant's fields. Fields of user types need
`series` on their type too.

```rust
#[derive_shrink_wrap(borrowed, owned, series, final_structure)]
pub struct Volt { pub prefix: Prefix, pub value: NumericValue }
```

What a `Series` doesn't take in its first version is a compile error naming the field: `RefBox` (recursive types),
`Result`, lists other than of `u8`, ranges, `TailSize`; `sparse` and tuple structs, `head_tail`. `#[series(opaque)]`
on a named field writes it as an opaque run of its own bytes instead, so that any serializable type fits (it then
needs `PartialEq`: a value is written again where it changed).

`series` also implements `shrink_wrap::series::SeriesStore` (SW-62), room for `N` items as an array per leaf field
([series.md](series.md#collecting-items-on-a-device-seriesstore)), and the size bounds `MAX_ITEM_BITS` /
`MAX_FIXED_BITS` ([series.md](series.md#size-bounds)). `series(no_store)` leaves the store out, for a type with a
store written by hand or with a field whose type has none; a type with a `#[series(opaque)]` field gets none anyway.
`series(cells)` also implements `SeriesCells`, the store as atomics for a writer in an interrupt
([series.md](series.md#cells-a-store-shared-with-an-interrupt-seriescells)); every field type needs it too.

## `transparent`

DER-18. A struct with one field that isn't zero bits (the others `PhantomData<..>` or `()`) is exactly that field on
the wire: a new type that adds meaning (a unit, an index, a checked range) for free. Its `ELEMENT_SIZE`, `PUSHES_LEN`
and `MAX_SIZE` are the field's, and serializing and deserializing call the field's own, so the bytes are the plain
field's wherever the struct is: in an evolvable or `sized` parent, in a `Vec`, `Option`, array or `Series`, or alone.

```rust
#[derive_shrink_wrap(borrowed, owned, transparent, series, derive(Debug, PartialEq, Clone, Copy))]
pub struct Count(pub u64);

#[derive_shrink_wrap(borrowed, owned, transparent, series, derive(Debug, PartialEq, Clone, Copy))]
pub struct Volts<U> { pub value: f32, pub _unit: PhantomData<U> }

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Ac; // a unit marker, no serdes

#[derive_shrink_wrap(borrowed, owned, sized, derive(Debug, PartialEq, Clone, Copy))]
pub struct Reading { pub on: bool, pub count: Count, pub volts: Volts<Ac> }
```

`Count` is `Sized { size_bits: 64 }` like a `u64`, so `Reading` can be `sized`, and `Count(u64::MAX)` alone is
`ff ff ff ff ff ff ff ff`. Without `transparent` a one-field struct is evolvable (`Unsized`, a length stored in its
parent), and `final_structure` keeps its size class `UnsizedFinalStructure` even around a `u64`. The
[conformance corpus](conformance.md) has every position (`transparent.*` cases), each with the bytes of its plain
twin.

- Borrowed and owned (`Label<'i>(&'i str)` gets `LabelOwned(String)`), generics (a parameter used only in
  `PhantomData` gets no bounds, as [above](#generics)), tuple and named structs, `validate` (a check of the value
  just read, a range say) and `display` go with it. `#[repr(transparent)]` on the struct is kept.
- With `series` / `series(cells)` the struct forwards `SeriesColumns`, `SeriesStore` and `SeriesCells` to the field:
  the same columns, store and cells, and a wrapped enum keeps its tag. The field's type needs `series` (or
  `series(cells)`) too.
- A compile error names the type for no field or two fields that aren't zero bits, for a size assumption
  (`sized`, `final_structure`, `self_describing`: the size is the field's), `sparse`, `fixed_layout`, `head_tail`,
  an enum, and for `#[default]` or `#[series(opaque)]` on the field.
- As a field of a derived type, a wrapper costs what its plain field costs (DER-20): the derive writes and reads it
  on the field's own latched path (`write_le32_latch`, one `latched()` per type for a `u32`), so the code is the
  plain field's, see [code size](code_size.md#transparent-fields). A wrapper with `validate` is latched too: a
  value out of range is kept as the error, and the type returns the first error as before.
- `transparent` is part of the type's wire contract, like a [size assumption](size_assumptions.md): turning it on or
  off for a released type changes its bytes wherever it isn't already `sized`.
- In the API model (`ww_self::ItemStructExtra::transparent`) the dynamic client, Python and the `ww` CLI read it the same
  way. A reader built before the flag existed (`ww_self` without `transparent`) sees a struct of one field: it reads
  the same bytes inside a parent, but not a value of a wrapper around an evolvable (`Unsized`) type on its own, where
  it expects a length the derive doesn't write.

## Generics

DER-14. Structs and enums can have type and const parameters, defaults and a where clause, next to their lifetime:

```rust
#[derive_shrink_wrap(borrowed, owned, series, final_structure)]
pub struct Timed<T> { pub t: Second, pub value: T }

#[derive_shrink_wrap(borrowed, owned)]
pub struct Label<'i, const N: usize> { pub text: MaxLen<&'i str, N> }

#[derive_shrink_wrap(owned, ww_repr = u8)]
pub enum Packet<T, const N: usize> where T: Copy + Default { Data([T; N]), Empty }
```

- Each generated impl bounds the type parameters by its own trait (`T: SerializeShrinkWrap +
  DeserializeShrinkWrap<'i>` for the borrowed side, the owned traits for the owned one, `SeriesColumns<'i>` with
  `series`). The bounds next to the parameters and the where clause are carried into the definition and every impl
  as written; what a field needs beyond that is yours to add (an array `[T; N]` is read into a default one, so
  `T: Default`).
- A type parameter the fields use only inside `PhantomData<..>` gets none of these bounds (DER-15), so a unit marker
  needs no impls of its own; `PhantomData<T>` is zero bits (`Sized { size_bits: 0 }`, like `()`) and has no column
  in a `Series`. A `Quantity<Volt>` writes the same bytes as the struct without the field:

    ```rust
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Volt; // a marker: no serdes, no SeriesColumns

    #[derive_shrink_wrap(borrowed, owned, series, final_structure, derive(Clone, Copy, Debug, PartialEq))]
    pub struct Quantity<U> { pub value: i32, pub _unit: PhantomData<U> }
    ```

    A parameter that a field also uses outside `PhantomData` keeps its bounds; a use the derive doesn't look into
    (inside a macro, a qualified path `<T as Tr>::X`, a function pointer) counts as a real one. The derive's own
    `derive(Clone, Copy, ..)` is Rust's and still asks `U: Clone, Copy`, which a marker derives anyway.
- The size of `T` and `N` is part of the type's: `Timed<u32>` is `final_structure`, `Timed` of an evolvable struct
  holds an `Unsized` field, a `sized` `Buf<const N: usize> { len: u8, data: [u8; N] }` is `8 + 8 * N` bits. A
  `sized` enum's variants must be one size for the arguments it is used with (`enum Either<T> { Left(T), Right(T) }`
  always is). The const asserts that check a `sized` / `final_structure` type next to its definition can't name a
  generic type, so they are left out: the check happens where the size is used.
- Const parameters can be of any type Rust takes (integers, `bool`, `char`) and go wherever a const goes: array
  lengths, `MaxLen<T, N>`, `TailSize<N>`, arguments of other types (`Buf<N>`).
- With a lifetime, `Tagged<'i, T>` gets the owned `TaggedOwned<T>`. A bound or where clause that names `'i` only
  fits the borrowed side; keep those on types that are only `borrowed`.
- `display` works on generic types: a field the message prints needs a `Display` bound, and `Error` is implemented
  where the type is `Debug`. `validate` takes a generic function (`validate = Ordered::check` with an
  `impl<T: PartialOrd> Ordered<T>`). The `discriminants` enum has unit variants only, so no parameters.
- `fixed_layout`, `head_tail` and `sparse` lay out one concrete type, and reject generics with an error naming the
  type.
- In an API (API-43) a generic type is used with its arguments, `fn sum(window: Window<4>)`, and is in the API model
  once with its parameters; const parameters there are integers or `bool` (`usize` is `u64`), a `char` one is an
  error naming the type. See [generic types in an API](../types.md#generic-types-in-an-api).

## `crate_path(<path>)`

Generated code refers to `shrink_wrap` items by path, so no import is needed for the macro, and the names it uses
(`BufReader`, `Error`, `ElementSize`, ...) never clash with the module's own. The macro finds the crate in
`Cargo.toml`: `::shrink_wrap` (or whatever it is renamed to), or `::wire_weaver::shrink_wrap` when only
`wire_weaver` is a dependency. `crate_path(..)` overrides that, for a crate that reaches `shrink_wrap` through some
other re-export:

```rust
#[derive_shrink_wrap(borrowed, crate_path(my_framework::shrink_wrap))]
struct Point { x: u8, y: u8 }
```

When neither crate is a dependency and nothing is given, the names are left unqualified and
`use shrink_wrap::prelude::*` is needed.

## `#[wire(W)]` field attribute

DER-19. A field keeps a plain Rust integer and is written as the wire type `W`: a narrower bit-sized one or a
variable-length one. The code that uses the struct sees `i8` and `u64`, the bytes are those of `I4` and `UNib32`:

```rust
#[derive_shrink_wrap(borrowed, owned)]
pub struct Wired {
    #[wire(I4)]
    pub small: i8,
    #[wire(UNib32)]
    pub count: u64,
    #[wire(U12)]
    pub raw: u16,
    pub last: bool,
}
```

`Wired { small: -7, count: 300, raw: 0xABC, last: true }` is the 4 bytes `9c d4 ab c8` (conformance case
`wire.struct`), where plain fields would take 12: the same bytes as a struct with fields of type `I4`, `UNib32`,
`U12` and `bool`, so a type can move between the two forms without a wire change.

- **Write** checks the value against `W`'s range: `small: 9` is `Err(Error::SubtypeOutOfRange)`, never a cut or
  wrapped value, and never a panic. **Read** widens `W` back to the field's type; a value of `W` that doesn't fit
  the field (an `UNib32` above `u16::MAX` into a `#[wire(UNib32)] u16`) is `Error::SubtypeOutOfRange` too.
- The field is a plain integer: `u8`..`u128`, `i8`..`i128`, `usize`, `isize`. `W` is an integer of 64 bits or
  less: `u8`..`u64`, `i8`..`i64`, `Nibble`, `U1`..`U64`, `I2`..`I64`, `UNib32`, `UVlq32`, `UVlq64`. Both are
  signed or both unsigned; anything else is a compile error naming the field. The conversion is the runtime trait
  `shrink_wrap::WireAs<T>` (`from_plain` / `into_plain`), implemented for these pairs.
- Works on struct fields, tuple struct fields and enum variant fields, with `#[default = ..]`, and in `sized` and
  `final_structure` types (the size is `W`'s).
- `#[cfg_attr(pred, wire(W))]` writes the field as `W` only where `pred` holds. Since that would change the bytes
  between builds, the derive adds a `compile_error!` for builds where `pred` doesn't hold: use it to keep
  `shrink_wrap` an optional dependency of a type crate (`cfg_attr(feature = "shrink_wrap", wire(I4))`), not to
  switch formats. The `wire(..)` must be alone in its `cfg_attr`.
- In a `series` type the field is a column of `W` (`shrink_wrap::series::Wired<W, T>`), while `SeriesStore` and
  `SeriesCells` hold the plain `T`; a value out of `W`'s range fails when the series is written.
- The API model (`ww_self`) records `W` as the field's type, so the dynamic client, `wire_weaver_py` and the C
  generator read and write it as `W` with nothing of their own; the walker also finds `wire(W)` inside `cfg_attr`.
- Not yet (DER-21): `#[wire(W)]` inside `Option`, arrays and lists, and in `fixed_layout` and `head_tail` types,
  which reject it with a compile error.

## Field and variant attributes

| Attribute               | On                       | What it does                                                                                                     |
|-------------------------|--------------------------|------------------------------------------------------------------------------------------------------------------|
| `#[default = expr]`     | struct fields            | added in a later version, read as `expr` from older data, also by a derived `serde::Deserialize` ([evolving types](derive.md#evolving-types)) |
| `#[flag]`               | a `bool` field           | moves the presence flag of the later `Option` / `Result` field of the same name here, to pack it with other small fields ([showcase](showcase.md#relocating-presence-flags-with-flag)) |
| `#[slot = N]`           | fields of a `sparse` struct | places the field at slot `N` ([sparse](layouts.md#sparse-a-struct-of-optional-fields))                       |
| `#[future_version]`     | the last enum variant    | holds discriminants from a newer version ([above](#no_future_version-future_version-name-enums-only))            |
| `#[cfg(..)]`            | enum variants            | compiles the variant out ([above](#cfg-on-enum-variants))                                                        |
| `#[display("..")]`      | variants, with `display` | the message instead of the doc line ([above](#display))                                                          |
| `#[series(opaque)]`     | named fields, with `series` | the field is an opaque run of its own bytes in a `Series` ([above](#series))                                  |
| `#[wire(W)]`            | struct and variant fields | the field keeps its Rust type, the bytes are `W`'s ([above](#wirew-field-attribute))                                |

### Behind `cfg_attr`, and attributes the derive doesn't read

rustc hands the item to the macro before it expands the `cfg_attr`s on its fields and variants, and a macro can't
evaluate a `cfg`. So:

- `#[cfg_attr(pred, default = expr)]` (or `flag`, `slot`, `since`, `display`, `common`, `future_version`,
  `series(..)`, `wire(W)`) is read as if `pred` holds, and the derive adds a `compile_error!` for builds where it
  doesn't: the bytes never change with a `cfg` the derive couldn't see. This is for a type crate that keeps
  `shrink_wrap` optional, with the derive under the same feature:
  `#[cfg_attr(feature = "shrink_wrap", derive_shrink_wrap(..))]` on the type and
  `#[cfg_attr(feature = "shrink_wrap", default = 1)]` on a field. These attributes go in a `cfg_attr` of their own,
  mixing them with other attributes is an error.
- Any other attribute on a field, a tuple variant's field or a variant (`#[serde(rename = "r")]`,
  `#[cfg_attr(feature = "defmt", defmt(Debug2Format))]`, a `cfg_attr` of those) stays on the generated field or variant
  as written, and rustc expands it there. It goes on the type with the item's own name only (the borrowed `Type<'i>`,
  or `TypeOwned` when that is what is written), like the item's own `derive`s: the owned mirror may not derive what
  the attribute belongs to.
- A name the derive doesn't know is a compile error from rustc ("cannot find attribute"), never silently nothing.

Special field types the macro handles itself: [`TailSize<N>`](special_types.md#tailsizen-the-size-of-the-rest-of-a-value)
(filled in with the size of the rest of the value; only in types without a size assumption, the same on the borrowed
and owned side) and `MaxLen<T, N>` (checked on serialize and deserialize, see
[bounded lists and strings](../types.md#bounded-lists-and-strings-maxlent-n)).

### `#[wire(W)]`: a field written as another type

`#[wire(W)] field: T` keeps the field a `T` in Rust and writes the bytes of `W`, the same as a field of type `W`
(DER-19). `W: shrink_wrap::WireAs<T>` converts both ways: `from_plain` when the value is written, `into_plain` when it
is read. A value the other side can't hold is `Error::SubtypeOutOfRange`, never cut and never a panic. The API model
shows the field as `W`, so the dynamic client, Python and C read it as a `W` field.

- **An integer as a narrower or variable-length integer**: `#[wire(I4)] offset: i8`, `#[wire(UNib32)] count: u64`.
  `W` is `u8`..`u64`, `i8`..`i64`, `Nibble`, `U1`..`U64`, `I2`..`I64`, `UNib32`, `UVlq32` or `UVlq64`, of the
  field's signedness; shrink_wrap implements `WireAs` for every plain integer.
- **A handle as the value it stands for** (DER-23): `W` is a user type without a lifetime that implements
  `WireAs<T>` for the field's type: a one-byte index in Rust that writes the full value it stands for. `T` is
  cloned to be written (free for a `Copy` handle).
- **A checked type as an integer**: `W` is an integer type from the first item and `T` a user type without a
  lifetime, with `impl WireAs<T> for W`. `exact_units::Prefix::Pow10(#[wire(i8)] Pow10Exp)` writes a plain `i8` and
  turns an exponent that isn't allowed (an SI prefix's) into `Error::SubtypeOutOfRange` when it is read.

```rust
use shrink_wrap::{Error, prelude::*};

#[derive_shrink_wrap(borrowed, owned, self_describing, derive(Debug, PartialEq, Clone, Copy))]
pub struct Scale { num: u32, den: u32 }

static SCALES: [Scale; 2] = [Scale { num: 1, den: 1 }, Scale { num: 1, den: 65535 }];

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct ScaleId(u8);

impl WireAs<ScaleId> for Scale {
    fn from_plain(id: ScaleId) -> Result<Self, Error> {
        Ok(SCALES[id.0 as usize])
    }
    fn into_plain(self) -> Result<ScaleId, Error> {
        let i = SCALES.iter().position(|s| *s == self).ok_or(Error::SubtypeOutOfRange)?;
        Ok(ScaleId(i as u8))
    }
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
struct Reading {
    #[wire(UNib32)]
    count: u64,
    #[wire(Scale)]
    scale: ScaleId,
}
```

In a `Series` an integer `W` is its numeric column (`series::Wired<W, T>`); a user `W` is an opaque run of its bytes
(`series::WiredOpaque<W, T>`), the same bytes as a `#[series(opaque)]` field of type `W`, each changed value built with
`from_plain` as it is written (the API model marks the field series-opaque, so the dynamic client reads it the same
way). Both keep `T` in a store and in cells, so a one-byte handle stays one byte per row: `T` implements
`SeriesStore` (and `SeriesCells` for `series(cells)`), `T: Clone + PartialEq`, and `W` needs no `series` of its own.

When the handles stand for only small `W` values, the impl says so with `const PLAIN_MAX_SIZE: Option<MaxSize>`
(`None` by default: `W::MAX_SIZE`): the field's `MAX_SIZE` and its series bound (`Series::max_bytes`) are then sized
by it, not by the largest `W`. `exact_units::Prefix` gives the 20 B of its largest computed scale instead of a full
`Magnitude`'s 97 B. It is a promise the impl's tests check: a value past it doesn't fit a buffer sized by it
(`Error::OutOfBounds`).

`#[cfg_attr(pred, wire(W))]` works too (rustc leaves a field's `cfg_attr` to the derive), for crates with shrink_wrap
optional; a compile error when `pred` is off while the type is serialized, so the bytes never depend on a cfg. Not
yet: inside `Option`, arrays and lists, `fixed_layout` and `head_tail` (DER-21, a derive error says so).
