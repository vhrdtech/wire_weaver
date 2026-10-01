# shrink_wrap changelog

## Unreleased

Covers `shrink_wrap` 0.2.0 and `shrink_wrap_derive` 0.2.0.

### ⚠️ Breaking

- `#[derive_shrink_wrap(..)]` takes all options as arguments instead of separate attributes: `#[owned = "std"]`
  becomes `owned(feature = "std")`, `#[final_structure]` / `#[self_describing]` / `#[sized]` become arguments of the
  same name, `#[ww_repr(u2)]` becomes `ww_repr = u2`, and `derive(..)`, `cfg_attr_borrowed(..)` / `cfg_attr_owned(..)`
  replace separate `#[derive]` / `#[cfg_attr]`. See `docs/serdes/derive.md`.
- Naming scheme: borrowed and plain types carry no postfix, allocating ones end with `Owned`.
  New `DeserializeShrinkWrapOwned` and `SerializeShrinkWrapOwned` (std) traits for the owned variants.
- `shrink_wrap_core` is merged into `shrink_wrap_derive`; `derive_shrink_wrap` and `ww_repr` are also re-exported
  from the `shrink_wrap` crate root.
- Wire format: `u4` is now 1-bit aligned like `u1`..`u3`, the 4-bit aligned type is `Nibble`; strings, tuples and arrays
  are `UnsizedFinalStructure`, same as `Vec<T>`.
- `StackVec` renamed to `AnyOnStack`, `RawSlice` to `TailBytes`.
- `BufWriter`: `write_raw_str()` → `write_str()`, `write_u4()` → `write_nib()`, `write_u16_rev()` /
  `update_u16_rev()` / `U16RevPos` → `write_rev_len()` / `update_rev_len()` / `RevPos`.
  `BufReader`: `read_raw_str()` → `read_str()`, `read_unib32_rev()` → `read_rev_len()`.
- `Error::StrTooLong`, `VecTooLong` and `ItemTooLong` are merged into `Error::LenTooLong`.
- `defmt-extended` and `tracing-extended` features removed (trace logging of reads/writes, didn't work out and was
  broken); drop them from your `Cargo.toml`. `shrink_wrap` no longer depends on `tracing`.

### 🚀 Features

- `BufWriterOwned` (std), writing into a growable buffer.
- `UnsizedBuilder` for builder-style serialization of Unsized objects, `BufWriter::save_state()` /
  `restore_state()` / `reset()` / `pos()`.
- `EitherAnyVec` and `EitherAnyVecBuilder`, to build dynamic arrays in stages on no_std.
- `RefVecU8Builder`.
- `#[derive_shrink_wrap(discriminants)]` for enums.
- `Option` and `Result` inside tuples.
- `BufReader::read_nib()`, `read_owned()`, `bits_left()`, `read_bytes()`.
- `SerializeShrinkWrap` for `&T` and `&[u8]`.
- Optional `serde` support.
- `Display` and `std::error::Error` for `Error`, `Display` for `UNib32`.
- `UVlq32`: byte-based variable length `u32` (big endian VLQ, 1 to 5 bytes, byte-aligned), with
  `BufWriter::write_uvlq32()`, `BufWriterOwned::write_uvlq32()`, `BufReader::read_uvlq32()` and
  `Error::MalformedUVlq32`. Supported as a field type by `#[derive_shrink_wrap(..)]`.
- `UVlq32Backfill`: `UVlq32` always written as the full 5 bytes, so that its value can be filled in after
  serialization with `UVlq32Backfill::backfill()`, which returns the slice starting at the shortest encoding.
  Meant for values only known right before sending, like request sequence numbers.
  `#[derive_shrink_wrap(..)]` only accepts it as the first field of a struct, not nested in other types or in enums.

### 🐛 Fixes

- `BufWriter::write_un8(8, ..)` and `BufReader::read_un8(8)` at a byte-aligned position panicked with shift overflow
  in debug builds; in release, `write_un8` silently wrote `0` instead of the value.
- Smaller flash footprint: `write_un8`/`write_un16` and `read_un8`/`read_un16` forward to the `u32` variants instead
  of each carrying its own copy of the bit loop (~300 bytes less on Cortex-M with `opt-level = "s"`).
- `#[derive_shrink_wrap(ww_repr = ..)]` did not check that enum discriminants fit into the representation: a
  discriminant too large for it was truncated on the wire and read back as another variant (e.g. the 5th variant of a
  `ww_repr = u2` enum was sent as `0`). It is now a compile error, same as with the `#[ww_repr]` attribute.
- An enum variant without an explicit discriminant following one with it (`A = 15, B`) got the same discriminant
  instead of the next one, failing to compile with a duplicate discriminant error.

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
