# shrink_wrap changelog

shrink_wrap was developed inside the wire_weaver repo from May 2024, moved to its own repo
([romixlab/shrink_wrap](https://github.com/romixlab/shrink_wrap)) in December 2025, where 0.1.0 to 0.1.2 were released,
and moved back into wire_weaver in February 2026.

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
  from the `shrink_wrap` crate root. `ww_repr` is no longer in `shrink_wrap::prelude`, import it from the crate root.
- Wire format: `u4` is now 1-bit aligned like `u1`..`u3`, the 4-bit aligned type is `Nibble`; strings, tuples and arrays
  are `UnsizedFinalStructure`, same as `Vec<T>`.
- `StackVec` renamed to `AnyOnStack`.
- `Error::OutOfBoundsWriteUN`, `OutOfBoundsReadUN` and `OutOfBoundsSplit` no longer carry a `UNib32` (the bit count
  or length asked for), and `Error` is `#[repr(u32)]` with no payloads at all: a `Result<u32, Error>` then comes
  back in two registers instead of through memory on 32-bit CPUs (SW-34). Match on the bare variants.
- `BufWriter`: `write_raw_str()` → `write_str()`, `write_u4()` → `write_nib()` (or `write_nib_masked()` for a raw
  `u8`), `write_u16_rev()` / `update_u16_rev()` / `u16_rev_pos()` / `U16RevPos` → `write_rev_len()` /
  `update_rev_len()` / `rev_len_pos()` / `RevPos`, `encode_nib16_rev()` → `encode_len_fifo()`.
  `BufReader`: `read_raw_str(self)` → `read_str(&mut self)`, `read_unib32_rev()` → `read_rev_len()`.
- `Error::StrTooLong`, `VecTooLong` and `ItemTooLong` are merged into `Error::LenTooLong`.
- `defmt-extended` and `tracing-extended` features removed (trace logging of reads/writes, didn't work out and was
  broken); drop them from your `Cargo.toml`. `shrink_wrap` no longer depends on `tracing`.

- A `sized` enum whose variants differ in size no longer compiles (SW-35): `sized enum X: variants differ in size
  (A: 8 bits, B: 32 bits); use self_describing ..`. Replace `sized` with `self_describing` on it, and on every `sized`
  type that contains it: the bytes stay the same, nothing has to be done on the other end. Such an enum never had
  one size, its `ELEMENT_SIZE` was wrong.
- `ELEMENT_SIZE` of `sized` / `self_describing` types changes where it was wrong (SW-35), the wire format does not:
    - enums: `Sized { size_bits }` is the discriminant plus one variant, it was the sum of all variants without
      the discriminant (`ww_repr = u8` with `A(u8)` and `B(u8)`: 16 bits, was 16 by accident; `ww_repr = u2` without
      fields: 2 bits, was 0);
    - user types inside `[T; N]`, tuples, `Range<T>` and `Option<T>` / `Result<T, E>` fields are counted, they were
      left out: a `sized` type with an `Option<UserType>` field no longer compiles (it is `self_describing`);
    - `Range<T>` and `RangeInclusive<T>` are two `T`, not one.

### 🚀 Features

- No length FIFO code for types without lengths, and a smaller one for the others (SW-36), same wire format. On
  RV32IC an encoder of a struct of plain fields is 964 bytes instead of 1476, of four integers 462 instead of 996;
  one with a list 1142 instead of 1378. All numbers in `docs/serdes/code_size.md`.
    - New associated constant `SerializeShrinkWrap::PUSHES_LEN`: whether `ser_shrink_wrap` can leave a length
      (string, list, `RefBox`, the size slot of a nested `Unsized` value) for `finish()` to encode. It has a
      default (`true` unless the type is `Sized`), so existing hand-written impls compile and behave as before;
      set it to `false` in one only when no path pushes a length. `#[derive_shrink_wrap(..)]` computes it from
      the fields, also for evolvable (`Unsized`) types. New `const fn write_pushes_len::<T>()`: whether
      `wr.write::<T>()` pushes one, for writing that constant by hand.
    - New `BufWriter::finish_no_lens()` (latched error, align to a byte, the written bytes; the FIFO encoder is
      not linked), `finish_for::<T>()` and `finish_for_and_take::<T>()` (one or the other by `T::PUSHES_LEN`, at
      compile time). `to_ww_bytes()` uses `finish_for`. `finish()` and `finish_and_take()` are unchanged.
    - New `Error::LenFifoNotEmpty`: `finish_no_lens()` on a writer that does have lengths, i.e. a wrong
      `PUSHES_LEN = false`. A debug build panics there instead. Add an arm if you match on `Error` exhaustively.
    - `BufWriter::encode_len_fifo()`, `write_nib()` and `align_nibble()` are smaller; `align_byte()` is no longer
      inlined.
- `just size-bench` has a `thumbv7em` target (Cortex-M4F / M7, `thumbv7em-none-eabihf`) next to RV32 and Cortex-M0
  (TEST-17). Thumb-2 is about as large as Cortex-M0 for a whole codec and gained about as much from SW-34..SW-38:
  `docs/serdes/code_size.md`, "Larger MCUs".
- `ElementSize::one_of()` (size of an enum's payload: same size or `SelfDescribing`), `ElementSize::variants_differ()`
  and `ElementSize::repeat()` (`[T; N]`), all `const fn` (SW-35). The size assert of a `sized` type says why it failed.
- Smaller code on small CPUs (SW-34), same wire format: a derived request/reply codec of a softcore firmware is
  19 % smaller on RV32IC (11.3 → 9.2 KB) and 20 % on Cortex-M0. Numbers and what is left in
  `docs/serdes/code_size.md`, measured with `just size-bench` (TEST-17).
    - `BufReader::read_bool_latch()`, `read_u8_latch()`, `read_u16_latch()`, `read_u32_latch()`, `read_i8_latch()`,
      `read_i16_latch()`, `read_i32_latch()`, `read_f32_latch()` and the same `write_*_latch()` on `BufWriter` (and
      `BufWriterOwned`, where they cannot fail): no `Result`, the first error is kept until `latched()` returns
      it, a failed read gives 0. `BufWriter::finish()` returns a kept error too.
    - `#[derive_shrink_wrap(..)]` reads and writes plain fields, `Option` / `Result` flags and `u8` / `u16` /
      `u32` discriminants through them and checks once per struct or enum. Fields with `#[default = ..]`, lists,
      strings and nested types are handled as before. `BufReader` (and so `RefVec`) is 4 bytes larger.
    - The integer reads and writes up to 32 bits share one function each, and writing them no longer calls
      `memcpy`; `UNib32::len_nibbles()` no longer divides (430 bytes of division and `clz` routines on RV32I).
- `RefVec<T>` of `u8`..`u128`, `i8`..`i128`, `f32` and `f64` is decoded without reading every element (SW-37):
  the end of the list is one bounds check (`RefVec<u8>` of 4096 bytes: 10 µs → 6 ns on an x86-64 host). Same wire
  format, and for every input the same result, `Error` and reader position as before; lists of any other type
  (`bool`, `u1`..`uN`, `Nibble`, enums, structs, strings, ..) are decoded as before. It is not free in code: a
  decoder with such a list is 50 to 140 bytes larger (RV32IC `vec_de` 592 → 652), see `docs/serdes/code_size.md`.
  Nothing to change in user code; `DeserializeShrinkWrap` has a hidden constant with a default for it.
- `U1`..`U64` and `I2`..`I64` fields are read without an error check each too (SW-38), like `u8`..`u32`: a
  bit-packed struct of eight fields decodes in 846 bytes instead of 886 on RV32IC, 1276 instead of 1364 on RV32I.
  Same wire format, same result and `Error` as reading every field with `?`. New `BufReader::read_un8_latch()`,
  `read_un16_latch()`, `read_un32_latch()` and `read_un64_latch()`; `DeserializeShrinkWrap` /
  `DeserializeShrinkWrapOwned` have a hidden method with a default for it, nothing to change in user code.
- Compressed sequences for time series (SW-31, SW-32): `Delta<'i, T>` / `DeltaOwned<T>` and
  `DeltaOfDelta<'i, T>` / `DeltaOfDeltaOwned<T>` for `u8`..`u64` and `i8`..`i64`, `XorFloat<'i, T>` /
  `XorFloatOwned<T>` for `f32` / `f64`. Used as fields like `RefVec<'i, T>` / `Vec<T>`, same layout around the
  elements (`UnsizedFinalStructure`, count as a reverse length), elements written bit by bit against the previous
  one: a repeated value costs 1 bit, a small integer step 9, a regular timestamp tick 1 (Gorilla codes). Borrowed
  variants decode lazily with no allocation (`no_std`), decoders reject malformed input with the new
  `Error::MalformedSeries` instead of panicking or allocating from an untrusted count. Bit layout in
  `docs/serdes/shrink_wrap.md`, bytes in `docs/serdes/showcase.md`; `examples/time_series.rs` prints encoded sizes
  for synthetic timestamps/counter/float data, raw vs compressed.
- `#[derive_shrink_wrap(..)]` keeps the generic arguments of user field types (DER-10): `x: Twice<u8>` is emitted
  as written, `x: Wrapper<'i, T>` becomes `WrapperOwned<T>` in the owned variant. Before, the arguments were
  dropped and such a struct did not compile.
- `DeserializeShrinkWrapOwned` implemented for all built-in types, `#[derive_shrink_wrap(..)]` generates it too.
- `Range<T>` and `RangeInclusive<T>` support.
- Unit type `()` support (zero bits on the wire).
- `TailBytes`: byte slice that takes all the remaining bytes of the buffer, without a length on the wire. Used by
  generated code in place of byte slices in streams and dynamic calls.
- `BufWriterOwned` (std), writing into a growable buffer.
- `UnsizedBuilder` for builder-style serialization of Unsized objects, `BufWriter::save_state()` /
  `restore_state()` / `reset()` / `pos()`.
- `EitherAnyVec` and `EitherAnyVecBuilder`, to build dynamic arrays in stages on no_std.
- `RefVecU8Builder`.
- `#[derive_shrink_wrap(discriminants)]` for enums.
- `Option` and `Result` inside tuples.
- `BufReader::read_nib()`, `read_nib_value()`, `read_owned()`, `bits_left()`, `read_bytes()`.
  `BufWriter::write_bytes()`, `buf()`.
- `SerializeShrinkWrap` for `&T` and `&[u8]`.
- `From<u32> for UNib32` and `From<UNib32> for u32`; `PartialEq` and `Eq` for `ElementSize`.
- Optional `serde` support.
- `Display` and `std::error::Error` for `Error`, `Display` for `UNib32`.
- `UVlq32`: byte-based variable length `u32` (big endian VLQ, 1 to 5 bytes, byte-aligned), with
  `BufWriter::write_uvlq32()`, `BufWriterOwned::write_uvlq32()`, `BufReader::read_uvlq32()` and
  `Error::MalformedUVlq32`. Supported as a field type by `#[derive_shrink_wrap(..)]`.
- `UVlq32Backfill`: `UVlq32` always written as the full 5 bytes, so that its value can be filled in after
  serialization with `UVlq32Backfill::backfill()`, which returns the slice starting at the shortest encoding.
  Meant for values only known right before sending, like request sequence numbers.
  `#[derive_shrink_wrap(..)]` only accepts it as the first field of a struct, not nested in other types or in enums.
- `UVlq64` (SW-33): byte-based variable length `u64`, same VLQ encoding as `UVlq32` widened to 64 bits (big endian,
  1 to 10 bytes, byte-aligned), with `BufWriter::write_uvlq64()`, `BufWriterOwned::write_uvlq64()`,
  `BufReader::read_uvlq64()` and `Error::MalformedUVlq64`. Supported as a field type (`uvlq64`) by
  `#[derive_shrink_wrap(..)]`. No `UVlq64Backfill`.
- `#[derive_shrink_wrap(..)]` rejects `TailBytes` / `TailBytesOwned` that is not the last field of a struct or enum
  variant, or is nested in `Option`, `Vec`, tuples or arrays; such a field silently swallowed the fields after it.
- `TailSize<N>` (SW-18): a field the derive macro fills in with the size of the rest of the enclosing value once it is
  serialized, and bounds the value with when reading, so a value can be read from a longer buffer (a file region, a
  record at a fixed offset), skipped without parsing, or evolved with trailing fields old readers skip. On the wire a
  `UVlq32` right-justified in `N` bytes (1 to 5, 5 by default), valid for any `UVlq32` reader; serializing fails with
  `Error::LenTooLong` when the size does not fit the width. Allowed in any position of a struct or enum variant, one
  per struct or variant, in `Unsized` types only; the fields before it must be `Sized` or `SelfDescribing` (compile
  error, or a const assert for user types). `BufWriter::reserve_tail_size()` / `backfill_tail_size()` (same on
  `BufWriterOwned`) for hand-written serialization. Types without it are unchanged on the wire.
- `#[derive_shrink_wrap(..)]` accepts any const expression as an array length (`[f32; MAX_SERIES]`, `[u8; N * 2]`),
  not only integer literals; a `sized` type's `ELEMENT_SIZE` carries the length symbolically.
- Generated code names `shrink_wrap` items by path, so `use shrink_wrap::prelude::*` is no longer required for the
  macro and the names no longer clash with the user's own `BufReader`, `Error` and friends (e.g. `tokio::io::BufReader`
  in the same module). The crate is found in `Cargo.toml` (`::shrink_wrap`, also renamed, or
  `::wire_weaver::shrink_wrap` when only `wire_weaver` is a dependency), the new `crate_path(..)` directive overrides
  it, and when neither crate is a dependency the names stay unqualified as before. `shrink_wrap_derive` depends on
  `proc-macro-crate` for the lookup.

### 🐛 Fixes

- A derived type that mixes plain fields with lists, strings or nested types could return a later error instead
  of the first one (since the latched reads of SW-34): a `u32` that did not fit, then a list, gave the list's
  `OutOfBoundsRev` instead of `OutOfBoundsReadRawSlice` (SW-41). It was an error in every case. Now an error kept
  by a `_latch` read stops the reader: every `BufReader` read with a `Result` returns that error, the `_latch`
  reads give 0 and no read moves until `latched()` takes it; an enum without a variant 0 reports it instead of
  `EnumFutureVersionOrMalformedData`. Every derived decoder returns the error reading field by field with `?`
  gives. Hand-written code that reads with a `Result` after a failed `_latch` read gets the kept error now instead
  of reading on. 20 bytes more for a request/reply decoder on RV32IC, up to 48 for small ones
  (`docs/serdes/code_size.md`).
- Bit reads could take bits of a length in the last byte, and a length bits the forward reads had taken (SW-40).
  The last byte of a message can hold forward data in its high nibble and the first nibble of the lengths read from
  the back in its low one. `BufReader::read_un8()` / `read_un16()` / `read_un32()` / `read_un64()` (and so
  `U1`..`U64`, `I2`..`I64` and their `_latch` reads) read up to the end of that byte: with 4 bits left before a
  length, `read_un8(6)` returned 2 bits of the length as data instead of `Error::OutOfBoundsReadUN`. And reading a
  length (`read_rev_len()`, every list, string and nested `Unsized` value) took a nibble the forward side had
  already read past, instead of `Error::OutOfBoundsRev`. Both are errors now; `read_bool()`, `read_nib()` and the
  byte reads were right already. Only malformed input is affected: every message `BufWriter` / `BufWriterOwned`
  writes decodes as before (`shrink_wrap/tests/rev_nibble_bounds.rs`), and the writer never wrote into its lengths.
- `BufWriter::align_nibble()`, `write_nib()` and a length encoded after 5 to 7 bits of a byte left the last three
  bits of that byte as they were in the buffer instead of zero (SW-36). A reader skips those bits, so decoding is
  not affected; the bytes no longer depend on what the buffer held before.
- `BufReader::nibbles_left()` was wrong after reading from the back (`read_u4_rev()`, reversed `UNib32` lengths),
  and `read_u4()` could fail with `OutOfBoundsReadU4` or read past the reverse part in the last byte.
- `BufWriter::write_un8(8, ..)` and `BufReader::read_un8(8)` at a byte-aligned position panicked with shift overflow
  in debug builds; in release, `write_un8` silently wrote `0` instead of the value.
- Smaller flash footprint: `write_un8`/`write_un16` and `read_un8`/`read_un16` forward to the `u32` variants instead
  of each carrying its own copy of the bit loop (~300 bytes less on Cortex-M with `opt-level = "s"`).
- `#[derive_shrink_wrap(ww_repr = ..)]` did not check that enum discriminants fit into the representation: a
  discriminant too large for it was truncated on the wire and read back as another variant (e.g. the 5th variant of a
  `ww_repr = u2` enum was sent as `0`). It is now a compile error, same as with the `#[ww_repr]` attribute.
- An enum variant without an explicit discriminant following one with it (`A = 15, B`) got the same discriminant
  instead of the next one, failing to compile with a duplicate discriminant error.
- `#[derive_shrink_wrap(..)]` rejected a field type substituted from a macro_rules `$T:ty` fragment, e.g.
  `RefVec<'i, $T>` in a macro expanding to a struct (DER-11): rustc wraps such a fragment in an invisible group for
  hygiene (and a literal `(u16)` parses the same way), which the type transform had no arm for. The catch-all error
  for a still-unsupported type now shows it as written instead of a debug dump of the `syn` AST.

### 📚 Documentation

- New docs page `docs/serdes/use_cases.md`: real-world uses with code and measured numbers, ring files of a load
  history bounded by `TailSize` slots (SW-18), request/reply frames over streams (`UVlq32` length, size cap), gossip
  payloads under a 4 KiB cap (schema choice vs compression), and where SW-28 compression wrappers would pay (time
  series). `tests/use_cases.rs` asserts every size and byte sequence quoted.

## [0.1.2] - 2026-01-07 #2

Only `shrink_wrap` was released, `shrink_wrap_derive` and `shrink_wrap_core` stay at 0.1.1.

### 🐛 Bug Fixes 0.1.2

- No_std build
- Separate versions of workspace crates to avoid publishing duplicates

## [0.1.1] - 2026-01-07

`shrink_wrap`, `shrink_wrap_derive` and `shrink_wrap_core`.

### 🐛 Bug Fixes 0.1.1

- No_std build

### 📚 Documentation

- Add crates badge

## [0.1.0] - 2025-12-21

First release of `shrink_wrap`, `shrink_wrap_derive` and `shrink_wrap_core` as standalone crates.

### 🚀 Features

- Supported types:
    - Boolean (one-bit alignment): `bool`
    - Discrete numbers:
        - Signed (one-byte alignment): `i8`, `i16`, `i32`, `i64`, `i128`
        - Unsigned (one-byte alignment): `u8`, `u16`, `u32`, `u64`, `u128`
        - Unsigned (four-bit alignment): `u4`, `Nibble`
        - Signed and unsigned (one-bit alignment): `iN` and `uN` (`U1`, `U2`, `U3`, ... `U64`, `I2` ... `I64`)
    - Nibble-based variable length u32: `UNib32` (1 to 11 nibbles)
    - Dynamically sized numbers `UN` and `IN` (bit count is carried along)
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
    - `RefBox<T>` for self-referential types, `Box<T>` on std.
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
- `ElementSize` (`Sized`, `SelfDescribing`, `UnsizedFinalStructure`, `Unsized`) and `#[final_structure]`,
  `#[self_describing]`, `#[sized]` attributes to trade evolvability for wire size, checked by const asserts.
- `to_ww_bytes()` and `from_ww_bytes()` on the serdes traits.
- `defmt` feature.
- #[shrink_wrap_derive] attribute and #[derive(ShrinkWrap)] derive macro.
- #[owned = "feature"] attribute to generate TyOwned from Ty<'i> and serdes code for it.
- Handle #[default = None] on evolved types.
