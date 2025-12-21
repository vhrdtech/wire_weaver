# Built-in types

* Boolean (one-bit alignment): `bool`
* Discrete numbers:
    * Signed (one-byte alignment): `i8`, `i16`, `i32`, `i64`, `i128`
    * Unsigned (one-byte alignment): `u8`, `u16`, `u32`, `u64`, `u128`
    * Unsigned (four-bit alignment): `u4`
    * Signed and unsigned (one-bit alignment): `iN` and `uN` (`U1`, `U2`, `U3`, ... `U64`, `I2` ... `I64`)
* Nibble-based variable length u32: `UNib32` (1 to 11 nibbles)
* Floating point numbers: `f32`, `f64`
* Textual:
    * UTF-8 string `String`
    * TODO: With max bounded length
* Sequences:
    * Arrays:
        * Arbitrary length array: `Vec<T>`
        * Byte array: `Vec<u8>`
        * Arbitrary length array (no alloc): `RefVec<'i, T>`
        * Byte array (no alloc): `RefVec<'i, u8>`
        * Fixed sized array: `[T; N]`
        * TODO: Max bounded
        * TODO: Fixed length array: `[T; N]`
* `Option<T>` and `Result<T, E>`
* `RefBox<T>` for self-referential types.
* User-defined:
    * Struct
    * Enum with or without data variants
        * U1..=U63 (1-bit aligned) and unib32 discriminants.
    * Tuple

* Not yet supported or not decided whether to support:
    * Tuple
    * Unicode character: `char` (4B)
    * ASCII character `c_char` (1B) (ASCII) and string: `c_str`
    * Map

# Library types

There are a lot more types as a part of a standard library (date, time, version, numbers, SI units, etc.).
See the [overview](std_library/overview.md).

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

## TODO: Subtypes (bounded numbers and array lengths)

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
