---
level: basic
---

# Built-in types

A field of a `#[derive_shrink_wrap]` type can be a number, a flag, a string, a list, an `Option`, a `Result`, a
tuple, or another type of your own. This chapter goes through the types ShrinkWrap knows, what each one costs on the
wire, and which one to pick. [Structs and enums](structs_and_enums.md) then covers your own types.

## Every type at a glance

The table shows one value of each type, written on its own with `to_ww_bytes`. The test `book_output_built_in_types`
in [`examples/sensor_packet/tests/it/book.rs`] writes the table, so every byte in it comes from a real run:

--8<-- "docs/generated/sensor_packet/built_in_types.md"

A value inside a struct takes the same bits as on its own, with one difference: bit-sized values of neighbouring
fields share a byte. `(true, 0xaa)` shows this: the `bool` takes the first bit, and the `u8` starts on the next byte.
The sections below explain each group of rows.

## Flags and bit-sized numbers

A `bool` takes one bit. ShrinkWrap packs bit-sized values one after another, so eight flags fit in one byte. The
bits left over before the next byte-sized value are [[padding]], and they are zero.

`U1` to `U64` and `I2` to `I64` are integers of exactly that many bits. A value that never exceeds 5 fits a `U3`,
and a pair of a `U3` and a `U7` takes 10 bits, not two bytes. Build one with `U3::new(5)`, which returns `None` for
a value that needs more bits. `Nibble` is a 4-bit value that starts on a 4-bit boundary.

## Whole-byte numbers

`u8` to `u128`, `i8` to `i128`, `f32` and `f64` take their full width, start on a fresh byte, and store the low byte
first. `0x1234` is `34 12`, and `-2` as an `i16` is `fe ff`. A float is its IEEE 754 bits: `1.5` is `00 00 c0 3f`.

A number that is usually small but sometimes large can take a variable-length type instead. `UNib32` stores a `u32`
in nibbles, three bits of the number per nibble, so 0 to 7 take half a byte. 9 takes two nibbles, and 300 takes
three. ShrinkWrap stores its own lengths this way. `UVlq32` and `UVlq64` do the same in whole bytes, seven bits per
byte.

A number that has a unit, such as a temperature or a time, is a `Fixed` or a `Point` from `ww_si`, as the sensor
packet's `Temperature` is. On the wire it is its plain number. [Numbers that know their unit](../std_library/ww_si.md)
explains them.

## Strings and lists

A string is its UTF-8 bytes, with its length at the back of the message: `"abc"` is `61 62 63` and then the length 3.
A list is its elements one after another, with the number of elements at the back.

Each has a borrowed and an owned form, as [Borrowed and owned](borrowed_and_owned.md) explains:

| What        | Borrowed: no allocator   | Owned: needs `std` |
|-------------|--------------------------|--------------------|
| a string    | `&'i str`                | `String`           |
| a list      | `RefVec<'i, T>`          | `Vec<T>`           |

A `RefVec` read from a buffer points into it and decodes each element as you iterate. To write one, make it from a
slice: `RefVec::new_bytes(&[1, 2, 3])` for bytes, or `RefVec::Slice { slice: &items }` for any elements. Bytes
that are not valid UTF-8 fail to read as a string, with the error `MalformedUtf8`.

An array `[T; N]` has a length that never changes, so its bytes are only the elements, with no length: `[1, 2, 3]`
is `01 02 03`.

## Strings and lists with a maximum

`MaxLen<T, N>` wraps a string or a list and allows at most `N` elements, or `N` bytes for a string. A sensor's name
of at most 8 bytes is a `MaxLen<&'i str, 8>`. Its bytes are those of the plain string, so the maximum costs nothing
on the wire.

The maximum is checked at both ends. `MaxLen::new` refuses a value that is too long, and so does reading one from
bytes, with the error `MaxLenExceeded`. A value is never cut to fit.

## `Option` and `Result`

An `Option<T>` is a one-bit flag, 1 for `Some`, followed by the value when there is one: `Some(5)` is `80 05`, and
`None` is the single bit 0. A `Result<T, E>` is the same, with 1 for `Ok` followed by the `T`, and 0 for `Err`
followed by the `E`. The sensor packet's `humidity` is an `Option`, and its flag is the bit before the padding in
[the layout of a reading](types_on_the_wire.md#where-each-bit-goes).

## Tuples and `()`

A tuple of 2 to 8 elements is its elements one after another, as if they were the fields of a struct. `Range<T>` and
`RangeInclusive<T>` are written the same way, as their two ends. `()` takes no bytes at all.

## Types ShrinkWrap doesn't have

`char` and maps (`HashMap`, `BTreeMap`) can't be fields. Use a `&str` or a `u32` for a character, and a list of
pairs, `RefVec<'i, (K, V)>` or `Vec<(K, V)>`, for a map.

## Where to go next

- [Structs and enums](structs_and_enums.md) puts these types together into your own types.
- [Special types](../serdes/special_types.md) covers types for particular jobs: owned strings without an allocator,
  compressed series of numbers, and values that bound their own size.
