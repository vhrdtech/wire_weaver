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
        * TODO: Max bounded
        * TODO: Fixed length array: `[T; N]`
* `Option<T>` and `Result<T, E>`
* User-defined:
    * Struct
    * Enum with or without data variants

* Not yet supported or not decided whether to support:
    * Tuple
    * Unicode character: `char` (4B)
    * ASCII character `c_char` (1B) (ASCII) and string: `c_str`
    * Map
