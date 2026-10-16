---
level: basic
---

# Built-in types

Every type on this page implements the `shrink_wrap` traits: it can be a field of a
[`#[derive_shrink_wrap]`](serdes/derive.md) type, and (unless noted) an argument, return value, property or stream
item of an API. Sizes below are for the value on its own; how values share bytes is on the
[shrink_wrap](serdes/shrink_wrap.md#how-it-stays-small) page.

## At a glance

| Type                                         | On the wire                                              | Alignment |
|----------------------------------------------|----------------------------------------------------------|-----------|
| `bool`                                       | 1 bit                                                    | bit       |
| `U1`..`U64`, `I2`..`I64`                     | exactly that many bits                                   | bit       |
| `Nibble`                                     | 4 bits                                                   | nibble    |
| `u8`..`u128`, `i8`..`i128`                   | 1 to 16 bytes, little-endian                             | byte      |
| `f32`, `f64`                                 | 4 / 8 bytes, IEEE 754, little-endian                     | byte      |
| `UNib32`                                     | 1 to 11 nibbles, small values short                      | nibble    |
| `UVlq32`, `UVlq64`                           | 1 to 5 / 1 to 10 bytes, small values short               | byte      |
| `&str`, `String`                             | UTF-8 bytes, length at the back                          | byte      |
| `RefVec<'i, T>`, `Vec<T>`                    | the elements, count at the back                          | byte      |
| `MaxLen<T, N>`                               | same as `T`, at most `N` elements                        | as `T`    |
| `[T; N]`                                     | the elements, no count                                   | as `T`    |
| `Option<T>`, `Result<T, E>`                  | a 1-bit flag, then the value                             | bit       |
| tuples (2 to 8), `Range<T>`, `RangeInclusive<T>`, `()` | the parts one after another (`()` is nothing)  | as parts  |
| `RefBox<'i, T>`, `Box<T>`                    | the value and its size                                   | byte      |
| structs and enums                            | the fields; an enum's discriminant in 1 bit to 4 bytes   | as fields |

The [special types](serdes/special_types.md) (`TailSize`, `ArrayString` / `ArrayVec`, compressed sequences,
`OptionAnyVec`) are listed [at the end](#special-types).

## Booleans and numbers

**`bool`** takes one bit. Bit-sized values pack back to back; the next byte-sized value starts a fresh byte.

**`u8`..`u128`, `i8`..`i128`** are written at full width, little-endian, starting on a byte: `0x1234u16` is
`34 12`. **`f32`, `f64`** are their IEEE 754 bits, little-endian: `1.5f32` is `00 00 c0 3f`.

**`U1`..`U64`, `I2`..`I64`** (`shrink_wrap::un`, in the prelude; `u7`, `ub8`, `i12`... in an API) take exactly as
many bits as their name says. In Rust each is backed by the smallest standard integer, with checked constructors
(`U7::new(x)`, `U7::try_from(x)`) and `.value()` / `u8::from(..)`. A `(U3, U7)` of `(5, 100)` is 10 bits:
`b9 00`. Note that `u8` is byte-aligned, while `U8` is bit-aligned. They work as struct fields, enum variant
payloads and API arguments: an `enum I2cAddress { Seven(U7), Ten(U10) }` with `ww_repr = u1` takes 8 or 11 bits
(see the [showcase](serdes/showcase.md#bit-sized-fields-and-variant-payloads-a-7-or-10-bit-i2c-address)).

**`Nibble`** (`nib` in an API) is 4 bits on a 4-bit boundary.

**Variable-length numbers** take fewer bytes for small values:

| Type     | Encoding                                                     | Range     | `300` is   |
|----------|--------------------------------------------------------------|-----------|------------|
| `UNib32` | 3 bits per nibble, the top bit says "more follows"            | `u32`     | `cd 4`     |
| `UVlq32` | big-endian VLQ, 7 bits per byte, the top bit says "more follows" | `u32`  | `82 2c`    |
| `UVlq64` | the same as `UVlq32`, 1 to 10 bytes                          | `u64`     | `82 2c`    |
| `UN`, `IN` | the bit width (3 to 6 bits), then that many bits           | `u64`, `i64` |         |

`UNib32` is what `shrink_wrap` uses for lengths and for `ww_repr = unib32` discriminants; values below 8 take one
nibble ([showcase](serdes/showcase.md#unib32-variable-length-numbers-by-the-nibble)). `UVlq32Backfill` is a
`UVlq32` always written as 5 bytes, so its value can be filled in after the rest is written.

## Strings and lists

**`&'i str`** (borrowed, zero-copy) and **`String`** (`std`) are UTF-8 bytes, with the length at the back of the
buffer as a `UNib32` ([why at the back](serdes/shrink_wrap.md#how-it-stays-small)): `"abc"` on its own is
`61 62 63 03`. Invalid UTF-8 fails to read with `Error::MalformedUtf8`.

**`RefVec<'i, T>`** (borrowed, no allocator) and **`Vec<T>`** (`std`) are the elements one after another, with the
count at the back. A `RefVec` read from a buffer points into it and decodes elements as you iterate; to write one,
make it from a slice (`RefVec::Slice { slice: &items }`, or `RefVec::new_bytes(&[1, 2, 3])` for bytes). The derive macro maps `RefVec` to `Vec` and `&str`
to `String` for the owned type ([type mapping](serdes/derive.md#type-mapping)). Without a slice, e.g. elements
coming from a sensor loop, [`RefVecBuilder`](serdes/special_types.md#refvecbuilder-lists-written-in-place) writes a
list into the buffer one element at a time, with the same bytes.

**`[T; N]`** is `N` elements with no count. `N` can be a literal or any const expression (`[f32; MAX_SERIES]`).

To own a string or list without an allocator, use [`ArrayString<N>` / `ArrayVec<T, N>`](serdes/special_types.md#arraystring-arrayvec-owned-values-without-an-allocator):
the same bytes, stored in a fixed-size array.

## Bounded lists and strings: `MaxLen<T, N>`

`MaxLen<T, N>` wraps a list or a string (`RefVec<'i, T>`, `Vec<T>`, `&'i str` or `String`) and allows at most `N`
elements (bytes for a string). It works everywhere a type can appear: struct and enum fields, method arguments and
return types, properties, `stream!` and `sink!` items, and nested inside other lists
(`MaxLen<RefVec<'i, MaxLen<&'i str, 8>>, 4>`).

```rust
pub const MAX_PIECE: usize = 256;

#[ww_trait]
trait Stage {
    fn write(offset: u32, last: bool, data: MaxLen<RefVec<'i, u8>, MAX_PIECE>);
    property!(rw label: MaxLen<&'i str, 32>);
}
```

In a `#[ww_trait]`, `N` is an integer literal, `{literal}`, or the name of a `const` defined in the API crate's
`lib.rs`. In a plain `#[derive_shrink_wrap]` type it is any const expression.

- **Checked both ways, never truncated.** `MaxLen::new` refuses a value that is too long. Serializing one (e.g.
  after changing it through `DerefMut`) fails with `Error::MaxLenExceeded`, and so does deserializing one, before
  any element is read. A server answers an over-long argument with an error, a client refuses to send one.
- **Same bytes as the unbounded type.** The bound only exists in the code on both ends. That keeps it evolvable: a
  bound can be added, raised, lowered or removed without a new wire format, and the
  [evolution rules](evolution/rules.md#maximum-lengths-maxlent-n) say which of those old peers accept.
- **Visible in introspection.** `ww_self` describes it as `Type::MaxLen { max_len, ty }`, so the dynamic client and
  the Python module check the bound too (Python raises `ValueError: ... at most N ...`).

`MaxLen` bounds the length of one value. The number of resources in a resource array (`stream!(adc[]: u16)`) is a
different thing, see [arrays](api/arrays.md).

### Worst-case size at compile time

Every type has a `SerializeShrinkWrap::MAX_SIZE: Option<MaxSize>`:

- `Some` when every part of it is bounded: numbers, arrays, `Option` / `Result`, structs and enums of them, and
  `MaxLen` lists and strings of bounded elements;
- `None` as soon as anything can grow without a limit: a plain `Vec`, `RefVec`, `String`, `RefBox`, compressed
  sequences.

`MaxSize::buffer_len_of(T::MAX_SIZE)` is a buffer length any value of `T` always serializes into. It is an upper
bound, not the exact worst case: every length is counted as at least four nibbles and every alignment as needed.
Owned types don't have `MAX_SIZE` yet (SW-42).

A generated `no_alloc` server also gets, per method, `<METHOD>_REQUEST_MAX_LEN` and `<METHOD>_RESPONSE_MAX_LEN:
Option<usize>`: the longest request message that calls it and the longest reply, `None` when an argument or the
return type is unbounded. A firmware can check at compile time that its message buffer is big enough:

```rust
const MAX_MESSAGE_LEN: usize = 512;
const _: () = assert!(matches!(
    api_server::WRITE_REQUEST_MAX_LEN,
    Some(len) if len <= MAX_MESSAGE_LEN
));
```

## `Option` and `Result`

**`Option<T>`** is a 1-bit flag (`1` for `Some`), then the value: `Some(5u8)` is `80 05`, `None` is one zero bit.
**`Result<T, E>`** is the same with `1` for `Ok` and `0` for `Err`, followed by the `T` or the `E`. Both work inside
lists, as fields and in every API position. Where the flag goes can be moved with `#[flag]`
([showcase](serdes/showcase.md#relocating-presence-flags-with-flag)), and why a flag beats an enum tag is in the
[showcase](serdes/showcase.md#optiont-and-resultt-e-a-flag-not-a-tag) too.

## Tuples, ranges and `()`

**Tuples** of 2 to 8 elements, **`Range<T>`** and **`RangeInclusive<T>`** are their parts one after another, as
if they were a struct's fields. **`()`** takes no bytes, for a method that returns nothing.

## Self-referential types: `RefBox`

A type that contains itself (a tree, an expression) needs indirection. **`RefBox<'i, T>`** gives it without an
allocator: read from a buffer it points into it and decodes on `.read()`; its owned counterpart is **`Box<T>`**.
On the wire it is the value plus its size, so a reader can jump over it. See the
[showcase](serdes/showcase.md#self-referential-types-with-refbox).

## Structs and enums

Your own types, with [`#[derive_shrink_wrap]`](serdes/derive.md):

- **Structs** are their fields in order. By default a struct is evolvable: fields can be appended later.
  Tuple and unit structs work too, see below.
- **Enums**, with or without data in their variants, are a discriminant followed by the variant's fields. The
  discriminant takes what `ww_repr` says: `u1`, `u2`, ... (bits), `nib`, `u8` / `u16` / `u32` (bytes), or
  `unib32` (variable length).

### Tuple structs

A tuple struct is written exactly like a struct with the same fields named: `Celsius(21.5)` is the `f32` alone,
`00 00 ac 41`, and `Rgb(1, 2, 3)` is `01 02 03`.

A unit struct (`struct Marker;`) has no data, but inside a parent it records its length like any evolvable
struct, a nibble that is usually shared with a neighbour: `(1, Marker, 2)` is `01 02 00`. That is what makes it
useful in an API: `fn arm() -> Armed` can later become `Armed { at: Timestamp }`, and older code still reads it.
On its own (a whole message) nothing follows it, so it takes no bytes at all. A unit struct with `sized`,
`final_structure`, `self_describing` or `fixed_layout` would be zero bits that can never change, so the derive
refuses it: leave the size assumption out, or give it reserved bits to grow into (e.g. a `U4` field).

```rust
#[derive_shrink_wrap(borrowed, owned, derive(Debug, Clone, Copy, PartialEq))]
pub struct Celsius(pub f32);

#[derive_shrink_wrap(borrowed, owned, sized, derive(Debug, Clone, Copy, PartialEq))]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive_shrink_wrap(owned, derive(Debug, Clone, PartialEq))]
pub struct Pair<'i>(pub u8, pub &'i str); // and PairOwned(u8, String)
```

Everything a named struct can do works the same way: size assumptions, `fixed_layout`, `head_tail`, `sparse`,
`validate`, `display` (with `{0}`, `{1}`), and new fields with `#[default = ..]`, only at the end. Where the
fields need a name (layout constants such as `Rgb::_2_OFFSET`, C names, error messages) they are `_0`, `_1`, ...
The one thing that needs names is [`#[flag]`](serdes/showcase.md#relocating-presence-flags-with-flag), which pairs a
relocated flag with its field by name: name the fields to use it. Dynamic clients see the fields unnamed, Python as
a tuple (a single field as its value, a unit struct as `None`).

How much a nested struct or enum costs on top of its fields depends on its
[size assumption](serdes/size_assumptions.md): an evolvable one adds its length (usually a nibble), a `sized` one
nothing.

## Special types

For particular jobs, on their own page:

| Type                                          | For                                                                      |
|-----------------------------------------------|--------------------------------------------------------------------------|
| [`TailSize<N>`](serdes/special_types.md#tailsizen-the-size-of-the-rest-of-a-value) | a value that bounds itself inside a longer buffer (file headers, records) |
| [`ArrayString<N>`, `ArrayVec<T, N>`](serdes/special_types.md#arraystring-arrayvec-owned-values-without-an-allocator) | owned strings and lists without an allocator              |
| [`Delta`, `DeltaOfDelta`, `XorFloat`](serdes/special_types.md#delta-deltaofdelta-and-xorfloat-compressed-sequences) | compressed time series of integers, timestamps and floats |
| [`OptionAnyVec`, `EitherAnyVec`](serdes/special_types.md#optionanyvec-sparse-slots-of-any-types) | sparse sets of values of any types; a `sparse` struct is built on it |
| `TailBytes`                                   | bytes that take up the rest of the buffer, no length; generated API code uses it for byte slices |

These are for Rust types and generated code; an API definition (`#[ww_trait]`) doesn't name them. The compressed
sequences are the exception: they work in every API position.

## Library types

The [standard library](std_library/overview.md) has many more types built from these: date and time, versions,
numbers with units, SI quantities, and the types of its I/O traits.

## Not supported yet

- `char` and maps (`HashMap`, `BTreeMap`; for now a `Vec` of `(K, V)`, SW-26).
- **Numbers restricted to a range or a set** (SW-20), like `u16` in `1..=512` or `u8` in `0..=8, 12, 16`, checked
  on both ends. Until then, a struct can check its fields with
  [`validate`](serdes/derive_reference.md#validate-path).
- **Units** on numbers (`f32` in amperes) as a hint for code generation and tools, not sent over the wire. The
  [`ww_si`](std_library/ww_si.md) crate has typed SI quantities today.
