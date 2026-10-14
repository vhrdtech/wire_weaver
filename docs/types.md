# Built-in types

* Boolean (one-bit alignment): `bool`
* Discrete numbers:
    * Signed (one-byte alignment): `i8`, `i16`, `i32`, `i64`, `i128`
    * Unsigned (one-byte alignment): `u8`, `u16`, `u32`, `u64`, `u128`
    * Unsigned (four-bit alignment): `nib`
    * Signed and unsigned (one-bit alignment): `u1`, `u2`, `u3`, ... `ub64`, `i2` ... `ib64`
        * `u8` is byte-aligned, while `ub8` is bit-aligned
        * In Rust: `U1`..`U64` and `I2`..`I64` (`shrink_wrap::un`, in the prelude), backed by the smallest standard
          integer, with checked constructors (`U7::new(x)`, `U7::try_from(x)`) and `.value()` / `u8::from(..)`.
          Usable as struct fields, enum variant payloads and method arguments, returns and properties: an
          `enum I2cAddress { Seven(U7), Ten(U10) }` with `ww_repr = u1` takes 8 or 11 bits
          (see [showcase](serdes/showcase.md#bit-sized-fields-and-variant-payloads-a-7-or-10-bit-i2c-address))
* Nibble-based variable length u32: `unib32` (1 to 11 nibbles, four-bit alignment)
* Byte-based variable length u32: `uvlq32` (big endian VLQ, 1 to 5 bytes, one-byte alignment)
    * `UVlq32Backfill`: always written as 5 bytes, value can be filled in later and leading bytes dropped
    * `TailSize<N>`: right-justified in `N` bytes, filled in with the size of the rest of the enclosing value, which
      readers bound the value with (see [wire format](serdes/shrink_wrap.md#size-of-the-rest-of-a-value-tailsizen))
* Byte-based variable length u64: `uvlq64` (same VLQ encoding as `uvlq32`, 1 to 10 bytes, one-byte alignment)
* Floating point numbers: `f32`, `f64`
* Textual:
    * UTF-8 string `String` / `&'i str`
    * With a maximum length in bytes: `MaxLen<String, N>` / `MaxLen<&'i str, N>`
      (see [bounded lists and strings](#bounded-lists-and-strings-maxlent-n))
* Sequences:
    * Arrays:
        * Arbitrary length array: `Vec<T>`
        * Byte array: `Vec<u8>`
        * Arbitrary length array (no alloc): `RefVec<'i, T>`
        * Byte array (no alloc): `RefVec<'i, u8>`
        * Fixed sized array: `[T; N]`, `N` a literal or any const expression
        * With a maximum length: `MaxLen<Vec<T>, N>` / `MaxLen<RefVec<'i, T>, N>`
          (see [bounded lists and strings](#bounded-lists-and-strings-maxlent-n))
    * Compressed sequences, written bit by bit against the previous element
      (see [wire format](serdes/shrink_wrap.md#compressed-sequences-delta-deltaofdelta-and-xorfloat)):
        * Integers as differences: `Delta<'i, T>` / `DeltaOwned<T>`, `T` any of `u8`..`u64`, `i8`..`i64`
        * Integers as differences of differences (timestamps): `DeltaOfDelta<'i, T>` / `DeltaOfDeltaOwned<T>`
        * Floats as XOR with the previous value: `XorFloat<'i, T>` / `XorFloatOwned<T>`, `T` is `f32` or `f64`
* `Option<T>` and `Result<T, E>`
* `RefBox<T>` for self-referential types.
* User-defined:
    * Struct
    * Enum with or without data variants, multiple discriminant options:
        * Bit-aligned: `u1`, `u2`, ...
        * Nibble-aligned: `nib`
        * Byte-aligned: `u8`, `u16`, `u32`
        * Variable length `unib32`.
    * Tuple

* Not yet supported or not decided whether to support:
    * Unicode character: `char` (4B)
    * ASCII character `c_char` (1B) (ASCII) and string: `c_str`
    * Map

# Library types

There are a lot more types as a part of a standard library (date, time, version, numbers, SI units, etc.).
See the [overview](std_library/overview.md).

# Bounded lists and strings: `MaxLen<T, N>`

`MaxLen<T, N>` wraps a list or a string (`RefVec<'i, T>`, `Vec<T>`, `&'i str` or `String`) and allows at most `N`
elements (bytes for a string). It works everywhere a type can appear: struct and enum fields, method arguments and
return types, properties, `stream!` and `sink!` items, and nested inside other lists (`MaxLen<RefVec<'i, MaxLen<&'i
str, 8>>, 4>`).

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

* **Checked both ways, never truncated.** `MaxLen::new` refuses a value that is too long, serializing one (e.g. after
  changing it through `DerefMut`) fails with `Error::MaxLenExceeded`, and so does deserializing one, before any
  element is read. A server answers an over-long argument with an error, a client refuses to send one.
* **Same wire form as the unbounded type.** Not a single byte changes: the bound only exists in the code on both
  ends. That is what keeps it evolvable: a bound can be added, raised, lowered or removed without a new wire format,
  and the [evolution rules](evolution/rules.md#maximum-lengths-maxlent-n) decide which of those old peers accept.
* **Visible in introspection.** `ww_self` describes it as `Type::MaxLen { max_len, ty }`, so the dynamic client and
  the Python module check the bound too (Python raises `ValueError: ... at most N ...`).
* **Worst-case size at compile time.** Every type has a `SerializeShrinkWrap::MAX_SIZE: Option<MaxSize>`, `Some` when
  every part of it is bounded (numbers, arrays, `Option`/`Result`, structs and enums of them, and `MaxLen` lists and
  strings of bounded elements) and `None` as soon as anything can grow without a limit (a plain `Vec`, `RefVec`,
  `String`, `RefBox`, compressed sequences). `MaxSize::buffer_len_of(T::MAX_SIZE)` is a buffer length any value of
  `T` always serializes into. It is an upper bound, not the exact worst case: every length is counted as at least
  four nibbles and every alignment as needed.

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

`MaxLen` bounds the length of one value. The number of resources in a resource array (`stream!(adc[]: u16)`) is a
different thing, see [arrays](api/arrays.md).

# Self-referential types

Self-referential types are supported through the `RefBox<T>`, providing similar semantics to Rust `Box<T>` type, but
without using heap allocation on `no_std`.

# Alignment

Some types are one- or four-bit aligned and the rest are one-byte aligned. Dense packing is used to
save space, including in enum discriminants (which can even be U1).
Byte arrays, strings, and Unsized objects are
all one-byte
aligned to limit code complexity and computations required. Unused bits are set to zero and can be reclaimed when
evolving a
type.

For example:

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

In a future version while older one is still in use, it was decided to add some more data:

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

Older code can still read new data and will skip the Option, and newer code can read old data, yielding None.
All the while, the serialized size didn't even change (it could have though, it's just an example).

## TODO: Subtypes (bounded numbers)

Lengths of lists and strings are bounded with [`MaxLen`](#bounded-lists-and-strings-maxlent-n).

Simple checked numbers where only a range of values is allowed:

* `u16<{1..=512}>`

Set of allowed values:

* `u8<{0..=8}, 12, 16, 20, 24, 32, 48, 64>`

Numbers are checked before serialization and after deserialization.

## TODO: SI support

Specify SI unit for any number:

* current: `f32<"A">`
* velocity: `f32<"m/s">`

Units are not transmitted over the wire, used as a hint for code generation and in UI tool.
