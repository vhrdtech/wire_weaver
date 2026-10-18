# shrink_wrap changelog

shrink_wrap was developed inside the wire_weaver repo from May 2024, moved to its own repo
([romixlab/shrink_wrap](https://github.com/romixlab/shrink_wrap)) in December 2025, where 0.1.0 to 0.1.2 were released,
and moved back into wire_weaver in February 2026.

## Unreleased

Covers `shrink_wrap` 0.2.0 and `shrink_wrap_derive` 0.2.0.

### 🚀 Features

* `Series` written straight from a `SeriesStore` (SW-64), column by column, the same bytes as the item walk:
  `SeriesStore::columnar` (whether rows of a store can be, `false` by default) and `SeriesStore::write_store`, and
  `series::write_store_series` for a whole `Series` (it writes nothing and returns `false` when a tag changes in the
  rows). Numbers, floats, bools, `u128` / `i128`, `Option`, arrays, tuples and `PhantomData` have them, the derive
  generates them for structs (an evolvable one's head included) and enums (one variant over the rows); strings,
  byte lists and `UN` / `IN` don't. Delta packed is one pass over each column. For hand-written stores:
  `series::StoreWord`, `write_numeric_column`, `write_numeric128_column`, `write_float_column`. One FOC level batch of the motor demo (21 x `Series<T>` and the
  clock, n = 32) takes 74.9 k instructions on a Cortex-M3 instead of 143.5 k with `DeltaPacked`, 100 k cycles on an
  STM32H725 instead of 312 k (60 k against 125 k at `opt-level = 3`).
* `series::SeriesCells` (API-39, observe cells): the `SeriesStore` struct of arrays as atomics, `put` with relaxed
  stores from one context (an interrupt), `copy_rows` into a `SeriesStore` from another, no lock and no
  `static mut`; `unsafe` to implement because all-zero bytes must be valid (`empty_cells()`, `const` constructors
  through `core::mem::zeroed()`). Impls for numbers, floats, `bool`, `Nibble`, `UNib32`, `UVlq32/64`, `U1`..`U64`,
  `I2`..`I64`, `UN`, `IN`, `Option`, arrays, tuples and `PhantomData`; `series::CellWord` for hand-written ones.
  `#[derive_shrink_wrap(.., series(cells))]` derives it for structs and enums (opt-in: a type with a hand-written
  store inside would otherwise stop compiling).
* `shrink_wrap::series::dynamic` writers (CLT-29), the write side of its readers for code that knows `T` only at
  run time: `write_count` / `write_count_owned`, `write_numeric`, `write_numeric128`, `write_float`, `write_run`,
  `write_opaque_run`, `write_opaque_field` and `write_struct_head` (a nested group length from a measuring closure).
  The same bits the generated `SeriesColumns` impls write for the same items and codecs.
- Compile-time codec policies (SW-63): `series::SeriesPolicy` is how a writer picks the codecs, implemented by
  `CodecPolicy` (at run time, links every codec and the trials) and by every type with `series::FixedCodecs`
  (`const NUMERIC`, `const FLOAT`), which links only its own codecs; `series::DeltaPacked` is one (delta and packed
  residuals, XOR floats). `CodecPolicy: From<F: FixedCodecs>`. A `Series<Timed<Ampere>>` encoder takes 15.0 KB of
  flash with `DeltaPacked` on a Cortex-M3 (9.8 KB on a Cortex-M4F), 25.4 KB with a `CodecPolicy` (17.6 KB), against
  90.8 KB (68.5 KB) before.
- `series::RawPlain` (SW-63), a second predefined `FixedCodecs` type: every number and float at full width, no
  prediction or residual coding, the cheapest writer and the biggest bytes, for comparing against `DeltaPacked` or
  where CPU matters more than bytes.
- Faster `Series` writing (SW-63), the same bytes. The packed coder keeps a block's residuals instead of running
  the predictor ahead to find the block's width. The predictor works on left-aligned values (no masking or sign
  extension per item), in a `u32` for leaves up to 32 bits. `CodecPolicy::SMALLEST` sizes all four coders of a
  predictor in one pass instead of writing each into a `BitCounter`. Column writers are structs stepped by a loop
  over the items, linked once per word and sink instead of once per field. A struct's field restarts compare the
  tags of only the fields that can have one (new `SeriesColumns::TAGGED`, `true` by default, `false` for numbers,
  `bool`, strings, tuples, arrays and derived structs; new `series::tag_changed`). An enum writes only the
  variants its tag run saw, instead of walking every item once per variant to find the empty ones. A
  non-evolvable struct no longer instantiates its group writer for a `BitCounter`. A `Series` writes its columns
  with the open byte in a register, whole bytes straight into the buffer. `BufWriter::write_un32` / `write_un64`
  write a byte (or the rest of one) per step instead of masking 64-bit values, and a write that doesn't fit writes
  nothing. `Series<Timed<Ampere>>` of 32 items on a Cortex-M3: 63 359 → 10 922 instructions with
  `fixed(delta, packed)`, 213 233 → 31 014 with `SMALLEST`.
- `SeriesStore` (SW-62): room for `N` items as a struct of arrays (`type Store<const N: usize>`, `empty_store`,
  `set`, `get`), what a device keeps a batch in before writing it as one `Series`. Implemented for numbers, `bool`,
  `Nibble`, `UNib32`, `UVlq32/64`, `U1..U64`, `I2..I64`, `UN`, `IN`, floats, strings and byte lists, `Option`
  (flags and the value's store), tuples, arrays and `PhantomData` (nothing); `#[derive_shrink_wrap(.., series)]`
  implements it for structs (their fields' stores, no padding between fields) and enums (an array of discriminants
  and the stores of every data variant's fields), as a hidden `__<Name>SeriesStore` type. A type with a
  `#[series(opaque)]` field gets none; `series(no_store)` leaves it out to write one by hand.
- `Series::encoded_len()` (API-41): the exact bytes a `Series` takes, by a counting pass over its items (nothing
  written), so a batch can be measured before the room for it is made.
- Series size bounds (SW-62): `SeriesColumns::MAX_ITEM_BITS` and `MAX_FIXED_BITS` (`Option<usize>`, `None` for
  strings, byte lists and opaque fields, and the default for hand-written impls), the worst case per item and per
  series under any `CodecPolicy`, implemented for every built-in type and by the derive; `Series::max_bytes(n)` puts
  them together. The helpers they are made of are in `series::bound`.
- `PhantomData<T>` fields (DER-15): `PhantomData<T>` serializes to zero bits (`ElementSize::Sized { size_bits: 0 }`
  and `MaxSize::ZERO`, like `()`) borrowed and owned, and is a `SeriesColumns` with no columns, for any `T` (no
  bounds). `#[derive_shrink_wrap]` leaves a type parameter that the fields only use inside `PhantomData<..>` without
  the serdes and `SeriesColumns` bounds it puts on the other parameters (DER-14), so `struct Quantity<U> { value:
  i32, _unit: PhantomData<U> }` works with a bare unit marker `U` and writes the same bytes as without the field.
- `series::dynamic` (CLT-29): the column readers of `Series` for decoders that know `T` only at run time
  (`read_count`, `read_numeric`, `read_numeric128`, `read_float`, `read_run` with `RunKind`, `read_opaque_run`,
  `read_struct_head`), one whole column per call, validating as the typed readers do. `ww_self`'s dynamic decoder is
  built on them.
- `RefVecBuilder<T>` (SW-61): writes a `RefVec<'i, T>` in place, element by element, into a `BufWriter` that may
  hold other data too, without a slice of the elements: `new(&mut wr)`, `push(&mut wr, &value)`,
  `extend(&mut wr, items)`, `push_with(&mut wr, |wr| ..)` for an element written by other means (a nested builder,
  a struct written by hand), `finish(&mut wr)` writes the count. Same bytes as `RefVec::Slice` with the same
  elements, as the whole buffer, a struct field or an element of another list. A push that fails (out of buffer, an
  element that fails to serialize, a `write_*_latch` error inside `push_with`) undoes what the element wrote, so the
  list can still be finished with the elements before it. `with_max_len(&mut wr, N)` for a
  `MaxLen<RefVec<..>, N>` fails the `N + 1`th push with `Error::MaxLenExceeded`. `RefVecBuilderOwned<T>` is the
  same for `BufWriterOwned`; both are exported at the crate root and in the prelude. A `RefVecBuilder<u8>` (and its
  owned twin) also has `extend_from_slice(&mut wr, &bytes)`, one copy of a whole slice, and
  `write_bytes_with(&mut wr, |wr| ..)`, where every byte the closure writes is an element (a byte blob holding other
  values, their lengths encoded inside it); both mix with `push`, and a failed call is undone like a failed push.
- A struct without fields (`struct Marker;`, `struct A {}`) with `sized`, `final_structure`, `self_describing` or
  `fixed_layout` is a compile error: it would be zero bits that can never change. Without one it is evolvable: it
  records its length inside a parent, so fields can be added later (SW-59).
- `Series<'i, T>` and `SeriesOwned<T>` (SW-60), in `series` (also at the crate root): a list of `T` written column by
  column, one column per leaf field, on the column codecs below. `Series::new(&items)` (each column's smallest codec)
  or `Series::with_policy(&items, CodecPolicy)` to serialize, `iter()` / `try_iter()` after deserializing;
  `SeriesOwned<T>(Vec<T>)` with `as_borrowed()`. `ElementSize::Unsized`, `n` as `UNib32` capped at
  `SERIES_MAX_ITEMS` (65 535: `LenTooLong` writing, `MalformedSeries` reading), nothing in the length FIFO,
  padded to a byte. Deserializing validates every column in one pass with no allocation (work bounded by the items
  times the columns) and keeps a cursor per column; iteration steps them all in parallel, zero-copy for `&str` /
  `&[u8]`; `SeriesOwned` grows its `Vec` as it decodes. A decoded `Series` serializes to its own bytes again.
- Trait `SeriesColumns<'i>` (SW-60): how a type writes (`write_group`) and reads (`skim`, `next`) its group of
  columns, with impls for `u8`..`u128`, `i8`..`i128`, `U1`..`U64`, `I2`..`I64`, `Nibble`, `UNib32`, `UVlq32`,
  `UVlq64`, `f32`, `f64`, `bool`, `Option<X>`, tuples up to 8, `[X; N]`, `&str`, `String`, `&[u8]`, `Vec<u8>` and
  `RefVec<u8>`; and the pieces a struct or enum impl is made of (`Level`, `SeriesCtx` with `RootCtx` /
  `VariantCtx` / `SiblingCtx`, `FieldItems`, `SiblingTags`, `EnumTags`, `write_struct_head` / `skim_struct_head`,
  `BitCounter`), which the `series` directive of the derive generates. Columns restart on entering an enum variant
  (or `Some`) and where an enum field (also an `Option`) declared earlier changed. Evolvable structs write their
  field count (outermost) or group length (nested), so fields appended with a default read both ways. New fuzz
  target `shrink-wrap-series-type`. `BitSink` is public, with `bit_pos()`.
- `series` directive of `#[derive_shrink_wrap]` (SW-60): implements `SeriesColumns` for the struct (named fields)
  or enum (unit, tuple and named variants; tag run, then each data variant's fields in discriminant order), borrowed
  and owned, so it can be the item of a `Series` or a field of one; nested user types through their own impls. An
  evolvable struct writes its field count or group length, a field without `#[default = ..]` missing from older data
  is `MalformedSeries`; `#[future_version]` items can't be written (`EnumFutureVersionOrMalformedData`), a variant
  compiled out is `EnumVariantDisabled` on reading. What `Series` v1 doesn't take is a compile error naming the
  field: `RefBox` (recursive types), `Result`, lists other than of `u8`, ranges, `TailSize`, sparse and tuple
  structs, `head_tail`. `#[series(opaque)]` on a named field writes it as an opaque run of its own `to_ww_bytes`
  instead (a change where the value isn't `==` the previous one), e.g. a `RefVec<'i, Point>`; without allocation on
  the borrowed side: the value is serialized into the writer's free space (`BitSink::put_serialized`,
  `BitSink::scratch`, `BitCounter::with_scratch`), `OpaqueField` / `OpaqueReader` are the generated code's helpers.
  `write_struct_head` hands its closure a `BitCounter` to measure the fields with. The design's golden bytes and
  size tables come out the same through the derive as through the hand-written impls before.
- Generics on derived structs and enums (DER-14): type parameters (`struct Timed<T> { t: Second, value: T }`,
  `enum Reading<'i, T> { Value(T), Labeled { label: &'i str, value: T } }`), const parameters of any type Rust takes
  (`struct Buf<const N: usize> { data: [u8; N] }`, `MaxLen<&'i str, N>`, `const STRICT: bool`), defaults
  (`T = u8`) and where clauses, also next to a lifetime (borrowed `Tagged<'i, T>`, owned `TaggedOwned<T>`). Each
  impl bounds the type parameters by its trait (`T: SerializeShrinkWrap + DeserializeShrinkWrap<'i>` for the
  borrowed side, the owned traits for the owned one, `SeriesColumns<'i>` with `series`) on top of the bounds and the
  where clause as written, which every generated impl carries; the size of `T` and `N` is part of the type's (a
  `sized` `Buf<3>` is 32 bits). Works with `series`, `display` (the message's fields need a `Display` bound, `Error`
  needs `Debug`), `validate` and `discriminants` (which has no parameters); `fixed_layout`, `head_tail`
  and `sparse` reject generics with an error naming the type, and the const size asserts of `final_structure` /
  `sized` are left out for a generic type. See `tests/generic_items.rs`.
- `SeriesColumns` for `UN` and `IN` (SW-60), by the enum rule: a tag run, then each variant's `bit_count` and
  `value` columns, so `ww_numeric::NumericValue` with `un` / `in` can be a `Series` field.
- Column codecs of `Series<T>` (SW-60), crate internal: numeric columns of 1- to 64-bit integers (a 4-bit header: predictor raw / delta / delta of delta,
  `11` an escape that is `MalformedSeries` for now; coder SW-31 prefix code / bit-packed blocks of 8 with a width per
  block / constant / plain; segments at restarts), 128-bit columns (raw + plain only), `f32` / `f64` columns (plain
  or the SW-32 XOR code, its window reset at a restart), runs of enum tags, bools, `Option` tags and opaque bytes
  (a first value and one entry per change). Writers choose each column's codec by counting the bits of every
  candidate (no scratch memory, no allocation) or use a fixed `CodecPolicy`; readers step one item at a time from a
  `BufReader` copy, and malformed data is an error, never a panic or work beyond the items stepped. The codes are
  checked bit for bit against the design's reference encoder. New fuzz target `shrink-wrap-series-columns`.
- Tuple and unit structs in `#[derive_shrink_wrap]` (SW-59): `struct Celsius(f32);`, `struct Pair<'i>(u8, &'i str);`
  and `struct Marker;` are written exactly like a struct with the same fields named, in every layout (the size
  assumptions, `fixed_layout`, `head_tail`, `sparse` with `#[slot = N]` on positional fields), with `owned`,
  `derive(..)`, `validate`, `display` (`{0}` in the message) and `#[default = ..]` on fields appended at the end.
  Where a name is needed (consts such as `Px::_2_OFFSET`, C names `px__2`, error messages) the fields are `_0`,
  `_1`, ... `#[flag]` on a field of a tuple struct is a compile error naming the struct: the relocated flag pairs
  with its field by name. Before, the derive panicked on a tuple struct. See `docs/types.md`.
- `validate = path` directive of `#[derive_shrink_wrap]` (SW-58): a function called on every struct value just read, its
  error fails the read. See `docs/serdes/derive.md`.

- `ArrayString<N>` / `ArrayVec<T, N>` (SW-57), new modules `array_string` and `array_vec`: owned, no-alloc values
  backed by a fixed-size `[u8; N]` / `[MaybeUninit<T>; N]`, same bytes on the wire as `&str`/`String` and
  `Vec<T>`, for places an owned value is needed without an allocator (a `value_on_changed` field of a string or
  list property in a `no_alloc` server, a settings value). `ArrayString::new`/`push_str`/`as_str`,
  `TryFrom<&str>`, `Deref<Target = str>`; `ArrayVec::new`/`push`/`as_slice`, `Deref<Target = [T]>`; both have a
  `CAPACITY` const and fail with `Error::MaxLenExceeded` (serializing, deserializing, `push_str`/`push`,
  `TryFrom`) rather than cutting a value that is too long. Re-exported at the crate root and in the prelude. See
  `docs/serdes/shrink_wrap.md#arraystring-arrayvec-owned-no-alloc-values`.
- `EitherAnyVecIter::next_with()` (CLT-15): reads the next item's flag and hands the caller a bounded `BufReader`
  for whichever side it is (already `rd.split`-bounded to its own size when unsized), for item types not known at
  compile time (e.g. a value deserialized according to a runtime type descriptor), which `next()`/`next_owned()`
  can't express as a static `L`/`R`. Used by `ww_self::ValueOwned::des_shrink_wrap_field_dyn()`.
- `OptionAnyVec` (SW-55), new module `option_any_vec`: a zero-copy, no-alloc array of `Option` slots of any types
  (numbers, structs, lists, strings, a nested `OptionAnyVec`), only the present values written, in slot order
  without padding between them. A presence header first, in the smallest of four modes for the set (all, dense
  bits, two-level bits, index list), self-delimiting so that readers with fewer or more slots than the writer read
  it; mode nibbles `4..=15` are reserved (SW-56 tri-state slots). `OptionAnyVecWriter` (own buffer) and
  `OptionAnyVecBuilder` (in place: `new`, `new_with_mode`, `new_nested`, `write`, `start_slot`, `finish`) take the
  present slots up front, e.g. `SetBits` over a bitset; `OptionAnyVec::new` checks the header, `iter()` reads slot by
  slot (`next`, `next_owned`, `next_present`), `present()` lists the present ids. New errors
  `Error::MalformedOptionAnyVec` (a header that can't have been written) and `Error::OptionAnyVecWrongSlot` (writer
  misuse), also in the C runtime's `sw_error` (28, 29). See `docs/serdes/shrink_wrap.md`.
- Sparse structs (DER-13): `#[derive_shrink_wrap(sparse, ..)]` on a struct whose fields are all `Option<T>`
  (borrowed and owned) writes it as an `OptionAnyVec`, one slot per field in declaration order, or at
  `#[slot = N]` (increasing, gaps allowed); only the present fields take space, a nested sparse struct is one slot,
  fields are only appended. A non-`Option` field, `#[flag]`, `#[default]`, an enum or a combination with
  `final_structure`, `sized`, `self_describing`, `fixed_layout` or `head_tail` is a compile error. See
  `docs/serdes/derive.md` ("sparse") and `docs/evolution/rules.md` ("Sparse structs").
- `OptionAnyVec` additions for sparse structs: `OptionAnyVecIter::read_slot` / `read_slot_owned` read a slot past
  absent ones (for a reader whose slots have gaps), `OptionAnyVecBuilderOwned` builds into a `BufWriterOwned` (same
  bytes), and `Error::OptionAnyVecUnknownSlot` (C: `SW_ERR_OPTION_ANY_VEC_UNKNOWN_SLOT`, 30) for a present slot in a
  reader's gap. `OptionAnyVec`, `OptionAnyVecBuilder` and `OptionAnyVecBuilderOwned` are re-exported at the crate
  root and in the prelude.
- `OptionAnyVecIter::read_slot_with()` (TOOL-11): `read_slot` with a closure given the bounded reader instead of a
  compile-time `T`, for a value deserialized according to a runtime type descriptor (a dynamic client decoding a
  `sparse` struct, e.g. a level's `PropertyUpdate`, from introspection alone), mirroring `EitherAnyVecIter::next_with`
  above.
- `RefVec::new_bytes` is a `const fn`, for byte vectors in `const` items (API-36).
- Bit-sized numbers (SW-48): `I16` (bit-aligned, 16 bits; `I15` and `I17` existed, `I16` was missing), and
  `From<U7> for u8` / `TryFrom<u8> for U7` (`Error::SubtypeOutOfRange` when it doesn't fit) for every `U1`..`U64`
  and `I2`..`I64` and its backing integer. Their use as enum variant payloads next to a sub-byte discriminant is
  tested byte for byte (`tests/sub_byte.rs`, `docs/serdes/showcase.md`).
- `#[common]` on an enum variant (STD-16): `#[common] Common(CommonError<'i>)` gets `From<CommonError>` for the
  enum and an implementation of `ww_error::HasCommon` (`e.common()`), for the borrowed and the owned type. One per
  enum, one unnamed field; the generated code reaches `ww_error` directly or through `wire_weaver`.
- Enum variants from a newer version (SW-16): an enum that ends with `#[future_version] Unknown(u32)` (any name,
  one `u32` field, no discriminant) reads a discriminant it doesn't know into that variant instead of failing with
  `EnumFutureVersionOrMalformedData`, and writes it back. An `Unsized` enum (the default) skips the newer variant's
  fields with its size slot, so new variants may carry data; a `sized` or `self_describing` enum only survives new
  unit variants. The variant takes no discriminant on the wire and doesn't change `ELEMENT_SIZE` or `MAX_SIZE`.
  `discriminant()` returns the wire discriminant also for it; writing one that doesn't fit the repr or belongs to a
  known variant gives `Error::SubtypeOutOfRange`. See `docs/serdes/derive.md`.
- `Error::is_read_eob()`: whether a read ran out of data.
- `head_tail` views and writers (SW-52): `Type::view(buf)` checks the lengths, walks the tail and gives a
  `TypeView<'_>` whose accessors read each field of the head at its offset when called (lists, strings, options and
  nested views found on the way); an enum's view is an enum of its variants' views. Every enum variant gets
  `Enum::write_<variant>(buf, fields..)`, the bytes of `to_head_tail` without making the value. `HeadTail` gains
  `type View` and `view_ht` (fixed-layout types: the value), `head_tail::view`. busgen's codec through them: 4988
  bytes on RV32IC, 3894 on Cortex-M0, against 5762 / 4408 hand-written (`size_bench` case `busgen_view`).
- Numbers through `fixed_layout::Le` (SW-53): the generated code of the fixed layout and of `head_tail` reads and
  writes every multi-byte number with `Le::ld` / `Le::st` on the bytes at its offset (`fixed_layout::at` /
  `at_mut`), one out-of-line function per type on RISC-V (no unaligned loads: four byte loads at every field
  otherwise), inline elsewhere. Bit fields' bytes are cleared once, not per field. `busgen_ht` on RV32IC 6004 →
  5658, `bits_fixed_ser` 698 → 458; a type written once grows (`fixed_ser` 498 → 612).
- `#[cfg]` on the variants of a `head_tail` enum (SW-54): a variant compiled out keeps its tag and reads as unknown.
- Head+tail encoding (SW-46): the `head_tail` directive gives a type a wire format of its own, every fixed-width
  field at a constant offset in a head (counts of lists and strings, flags of `Option` / `Result`, heads of nested
  types included) and the variable parts in field order in a tail. New module `head_tail`: the `HeadTail` trait (in
  the prelude), `List<'i, T>` for lists, the `Tail` / `TailWr` cursors, `read` / `write`, `write_c_header` and
  `C_TAIL_PRELUDE` for the generated C tail walker. Derived types get `HEAD_LEN`, offset consts, `from_head_tail` /
  `to_head_tail` / `head_tail_len`. Every fixed-layout type now implements `HeadTail`; `u8`..`u64`, `i8`..`i64`,
  `f32`, `f64` implement `FixedLayout` (as items of a `List`).
- Fixed-layout fast path for bit fields and a variable last field (SW-45): `bool`, `U1`..`U32`, `I2`..`I32` and
  `Nibble` at compile-time bit offsets (`<FIELD>_BIT_OFFSET`, `<FIELD>_BITS`), `Option` / `Result` / unequal enums as
  the last field, flags (also relocated with `#[flag]`) as bit fields; `MIN_LEN` and `fixed_len()` next to
  `ENCODED_LEN`; `FixedLayout` gains `MIN_LEN`, `LEADS_WITH_BITS` and `TRAILS_WITH_BITS`; the C prelude gains
  `sw_fixed_bits` / `sw_fixed_sbits`. Refusals say why and what to use instead. `U1`..`U64` / `I2`..`I64` gain hidden
  `__to_wire` / `__from_wire` for the generated code.
- Fixed-layout fast path (SW-44): a derived type whose encoding is a fixed little-endian byte layout gets
  `ENCODED_LEN`, an offset const per field (`<FIELD>_OFFSET`, `<VARIANT>_<FIELD>_OFFSET`, `<VARIANT>_TAG`,
  `<VARIANT>_LEN` for enums) and `from_fixed` / `to_fixed` working on the slice directly, plus the new
  `fixed_layout::FixedLayout` trait (also in the prelude) with `write_c` for the matching C reader. Detected for
  `sized` / `final_structure` structs and `sized` / `self_describing` byte-tagged enums of fixed-width numbers and
  arrays of them; the new `fixed_layout` directive requires it, allows nested fixed-layout types and names the field
  that breaks it. Same bytes as `ser_shrink_wrap`. The new inherent items can clash with items of the same name a
  type already defines.
- C99 runtime (SW-43): `shrink_wrap/c/shrink_wrap.h` and `shrink_wrap.c`, `sw_reader` / `sw_writer` with the same
  bytes as `BufReader` / `BufWriter`, for firmware written in C. No `malloc`, the first error is kept as in SW-41,
  works with a 16-bit `char`. Checked against the Rust crate by the new `shrink_wrap_c` test crate (not published).
  See `docs/c.md`.
- Bounded lists and strings (SW-19): `MaxLen<T, N>` wraps a `RefVec`, `Vec`, `&str` or `String` with at most `N`
  elements (bytes for a string). Same bytes on the wire as `T` alone. A longer value is an error, never cut:
  serializing it and deserializing it (checked before the elements are read) give the new `Error::MaxLenExceeded`,
  `MaxLen::new` too. Derefs to `T`; `#[derive_shrink_wrap]` maps `MaxLen<RefVec<'i, T>, N>` to
  `MaxLen<Vec<T>, N>` (and `&str` to `String`) on the owned side. `N` can be a literal, a const or `{ expr }`.
- Worst-case serialized size (SW-19, first step of API-18): `SerializeShrinkWrap::MAX_SIZE: Option<MaxSize>`, `Some`
  when every part of a type is bounded (numbers, `bool`, arrays, tuples, `Option`, `Result`, `MaxLen`, derived
  structs and enums of them), `None` otherwise and by default. `MaxSize::buffer_len()` is a buffer length
  `to_ww_bytes` always fits into, usable in a `const`. `#[derive_shrink_wrap]` sets it for borrowed types;
  `MaxSize::{seq, one_of, repeat, list, unsized_value, of_write}` and `write_max_size::<T>()` build it in
  hand-written impls. `UNib32::len_nibbles` is a `const fn`.
- `#[cfg(..)]` on enum variants (DER-8): the variant, its serdes code and its part of `ELEMENT_SIZE` exist only where
  the predicate holds. Discriminants are numbered over all variants, so the wire format does not depend on features;
  reading a variant that is compiled out gives the new `Error::EnumVariantDisabled`. Before, the attribute was
  silently dropped and the variant always generated. A `sized` enum needs at least one variant without `#[cfg]`.
- `non_exhaustive` argument of `#[derive_shrink_wrap(..)]`, puts `#[non_exhaustive]` on the generated types.
- `display` directive (DER-12): writes `core::fmt::Display` and `core::error::Error` (borrowed and owned, no
  `source()`) from the first doc line of each variant/struct without its trailing period, `{field}` filled in from the type's own field
  (format specs work too, e.g. `{code:#x}`); `#[display("..")]` overrides it, needed for a tuple variant's fields
  (`{0}`, `{1}`, ...). A variant or struct with neither a doc comment nor `#[display("..")]` is a compile error.
  The `#[future_version]` variant (SW-47) always has one. Unused code is dropped by the linker on a device that
  never calls it. See `docs/serdes/derive.md`.

### ⚠️ Breaking

* `Delta`, `DeltaOfDelta` and `XorFloat` (SW-31, SW-32) are removed, with `DeltaOwned`, `DeltaOfDeltaOwned`,
  `XorFloatOwned`, `DeltaIter`, `DeltaOfDeltaIter`, `XorFloatIter` and `DeltaInt`: `Series<T>` (SW-60) covers them
  with the same codes (its numeric columns' prefix coder is the SW-31 code with the delta or delta of delta
  predictor, its float columns' XOR codec is the SW-32 code) plus packed, constant and plain coders and the choice of
  the smallest per column. Migrate a field `Delta<'i, T>` / `DeltaOfDelta<'i, T>` / `XorFloat<'i, T>` to
  `Series<'i, T>` (`..Owned<T>` to `SeriesOwned<T>`, `::new(&values)` stays); a fixed codec where a small MCU wants
  one pass is `Series::with_policy(&values, DeltaPacked)`. The bytes differ (a `Series` has a count, a 4-bit header
  per column and pads to a byte): data written with the old types must be rewritten. `XorBits` stays (float
  columns). The prelude exports `Series` / `SeriesOwned` in their place.
- `SeriesColumns::write_group` takes the codec policy as a type parameter, `policy: &P` with
  `P: SeriesPolicy + ?Sized`, instead of `&CodecPolicy` (SW-63): a hand-written impl adds `P` to its generics and
  the bound to its where clause, and passes `policy` on as before (`policy.numeric()` / `policy.float()` where it
  read the fields). Derived impls follow by themselves.
- `Series<'i, T, P = CodecPolicy>` has the policy as a type parameter (SW-63); `Series<'i, T>` is the same type as
  before. `Series::with_policy` and `SeriesOwned::as_borrowed_with` take any `SeriesPolicy`.
- `#[derive_shrink_wrap(.., series)]` also implements `SeriesStore` (SW-62), which needs every field's type to have
  one: a `series` type with a field of a type that has none (a type with a `#[series(opaque)]` field, a hand-written
  `SeriesColumns` impl) fails to compile until it says `series(no_store)`.
- `RefVecU8Builder` is removed, folded into `RefVecBuilder<u8>` (SW-61). Replace
  `let b = RefVecU8Builder::new(&mut wr)?; /* writes */ b.finish(&mut wr)?;` with
  `let mut b = RefVecBuilder::<u8>::new(&mut wr)?; b.write_bytes_with(&mut wr, |wr| { /* writes */ })?;
  b.finish(&mut wr)?;`. Same bytes, except that an empty blob after bit-sized fields no longer aligns to a byte
  first: it is now the same as an empty `RefVec<u8>`, as readers always expected.
- Enum variants from a newer version by default (SW-47): `#[derive_shrink_wrap]` adds `#[future_version]
  Unknown(u32)` to every `Unsized` enum (the default) that has a free discriminant in its `ww_repr` and doesn't have
  such a variant already. Same wire, `ELEMENT_SIZE` and `MAX_SIZE`, but a new Rust variant: exhaustive matches need
  an `Unknown(_)` arm, fieldless enums can't be cast with `as` anymore, `Copy` enums grow. Opt out with the
  `no_future_version` argument (an enum that is complete by nature, or one whose readers must fail on a newer value),
  name it with `future_version = <Name>` when `Unknown` is a variant of its own. `final_structure`,
  `self_describing` and `sized` enums only get it when written. See `docs/serdes/derive.md`.
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

- Decoding a recursive type (`Box<T>`, `RefBox<T>`, `Vec<T>` or `RefVec<T>` of a type that contains itself, like
  `ww_self`'s `TypeOwned` and `ValueOwned`) had no nesting limit: 38 KB of untrusted data with 10 000 nested lists
  overflowed the stack and aborted the reading process (P2401). `BufReader` now counts the levels: `read()` /
  `read_owned()` of a value that is not `Sized` or `SelfDescribing` and `split()` go one level deeper, and past the
  limit the read fails with the new `Error::NestingTooDeep` (C: `SW_ERR_NESTING_TOO_DEEP`, 31). Values read later
  keep the depth they were read at (`RefBox`, `RefVec`, the slots of a `sparse` struct, `EitherAnyVec` elements).
  The limit is `BufReader::DEFAULT_MAX_DEPTH` = 128 for `BufReader::new`, `from_ww_bytes` and
  `from_ww_bytes_owned`; `BufReader::with_max_depth(n)` sets another one, `depth_left()` reads what is left. Same
  wire format; only data nested deeper than the limit, far deeper than real data, no longer decodes. See
  `docs/serdes/wire_format.md` ("Nesting depth"). New fuzz target `shrink-wrap-nesting`.
- `RefVec == RefVec` was `true` when one list is a prefix of the other (an empty one equal to any): the lengths are compared too now.
- Fields with `#[default = ..]` (SW-15) read as their default only when they start at the end of the data (written
  by an older version) and their read runs out of data. Before, any error in such a field gave the default, so a
  malformed or truncated value was silently replaced. The flag of an evolved `Option` or `Result` falls back with
  it, and the value is the `#[default]` given (an `Option` was always `None`). `#[default]` is accepted on fields of
  any type, not only `Option` and `Vec` (as `docs/evolution/rules.md` already said), except `TailSize`,
  `TailBytes` and `UVlq32Backfill`. Same wire format.
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

- New logo in the README: a heatshrink sleeve shrunk onto wire_weaver's twisted pair, light and dark; replaces the
  old one with the Rust gear
- Logo wordmark reads `ShrinkWrap`, centred in a sleeve with round, gently narrowing ends, the twisted pair showing
  through it as a soft relief; generated by `docs/assets/src/logos.py` (`just logos`), the old one is kept as
  `docs/assets/shrink_wrap_logo_snake*.svg`
- New docs page `docs/serdes/use_cases.md`: real-world uses with code and measured numbers, ring files of a load
  history bounded by `TailSize` slots (SW-18), request/reply frames over streams (`UVlq32` length, size cap), gossip
  payloads under a 4 KiB cap (schema choice vs compression), and where SW-28 compression wrappers would pay (time
  series). `tests/use_cases.rs` asserts every size and byte sequence quoted.

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.

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
