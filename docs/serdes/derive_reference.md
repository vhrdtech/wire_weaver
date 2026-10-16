---
level: advanced
---

# Derive reference

Every directive `#[derive_shrink_wrap(..)]` accepts, and the attributes it reads on fields and variants. Start with
[Derive](derive.md) for the basics.

The order of directives doesn't matter to the parser, but the codebase consistently writes them in this order (and
so do the examples):

```text
borrowed, owned, cfg_attr*, derive*, non_exhaustive, final_structure|self_describing|sized|sparse, ww_repr,
discriminants, display, validate, no_future_version|future_version
```

| Directive                                          | Section                                                                     |
|----------------------------------------------------|-----------------------------------------------------------------------------|
| `borrowed`, `owned`                                 | [Generated types](#generated-types)                                          |
| `cfg_attr..`, `derive..`, `non_exhaustive`          | [Attributes on the generated types](#attributes-on-the-generated-types)      |
| `final_structure`, `self_describing`, `sized`       | [Size assumptions](size_assumptions.md)                                      |
| `fixed_layout`, `head_tail`, `sparse`               | [Fixed and sparse layouts](layouts.md)                                       |
| `ww_repr`, `discriminants`, `no_future_version`, `future_version` | [Enums](#enums)                                               |
| `display`, `validate`                               | [Messages and checks](#messages-and-checks)                                  |
| `crate_path`                                        | [`crate_path`](#crate_pathpath)                                              |
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
  variant's fields are `{0}`, `{1}`, ...
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
result of the read.

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

## Field and variant attributes

| Attribute               | On                       | What it does                                                                                                     |
|-------------------------|--------------------------|------------------------------------------------------------------------------------------------------------------|
| `#[default = expr]`     | struct fields            | added in a later version, read as `expr` from older data ([evolving types](derive.md#evolving-types))             |
| `#[flag]`               | a `bool` field           | moves the presence flag of the later `Option` / `Result` field of the same name here, to pack it with other small fields ([showcase](showcase.md#relocating-presence-flags-with-flag)) |
| `#[slot = N]`           | fields of a `sparse` struct | places the field at slot `N` ([sparse](layouts.md#sparse-a-struct-of-optional-fields))                       |
| `#[future_version]`     | the last enum variant    | holds discriminants from a newer version ([above](#no_future_version-future_version-name-enums-only))            |
| `#[cfg(..)]`            | enum variants            | compiles the variant out ([above](#cfg-on-enum-variants))                                                        |
| `#[display("..")]`      | variants, with `display` | the message instead of the doc line ([above](#display))                                                          |

Special field types the macro handles itself: [`TailSize<N>`](special_types.md#tailsizen-the-size-of-the-rest-of-a-value)
(filled in with the size of the rest of the value; only in types without a size assumption, the same on the borrowed
and owned side) and `MaxLen<T, N>` (checked on serialize and deserialize, see
[bounded lists and strings](../types.md#bounded-lists-and-strings-maxlent-n)).
