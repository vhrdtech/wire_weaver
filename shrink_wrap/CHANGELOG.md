# shrink_wrap changelog

## [0.1.2] - 2026-01-07 #2

### 🐛 Bug Fixes 0.1.2

- No_std build
- Separate versions of workspace crates to avoid publishing duplicates

## [0.1.1] - 2026-01-07

### 🐛 Bug Fixes 0.1.1

- No_std build

### 📚 Documentation

- Add crates badge

## [0.1.0]

### 🚀 Features

- Supported types:
    - Boolean (one-bit alignment): `bool`
    - Discrete numbers:
        - Signed (one-byte alignment): `i8`, `i16`, `i32`, `i64`, `i128`
        - Unsigned (one-byte alignment): `u8`, `u16`, `u32`, `u64`, `u128`
        - Unsigned (four-bit alignment): `u4`
        - Signed and unsigned (one-bit alignment): `iN` and `uN` (`U1`, `U2`, `U3`, ... `U64`, `I2` ... `I64`)
    - Nibble-based variable length u32: `UNib32` (1 to 11 nibbles)
    - Floating point numbers: `f32`, `f64`
    - UTF-8 string `String`
    - Sequences:
        - Arrays:
            - Arbitrary length array: `Vec<T>`
            - Byte array: `Vec<u8>`
            - Arbitrary length array (no alloc): `RefVec<'i, T>`
            - Byte array (no alloc): `RefVec<'i, u8>`
            - Fixed sized array: `[T; N]`
    - `Option<T>` and `Result<T, E>`
    - `RefBox<T>` for self-referential types.
    - User-defined:
        - Struct
        - Enum with or without data variants
            - U1..=U63 and unib32 repr for enums.
        - Tuple
- `no_std` without allocator support (even with types like String and Vec, for both reading and writing)
- `std` support (standard Vec and String are used)
- Zero-copy deserialization
- StackVec for storing types with arbitrary sizes on stack
- Built-in mechanism for backwards and forwards compatibility
- #[shrink_wrap_derive] attribute and #[derive(ShrinkWrap)] derive macro.
- #[owned = "feature"] attribute to generate TyOwned from Ty<'i> and serdes code for it.
- Handle #[default = None] on evolved types.
