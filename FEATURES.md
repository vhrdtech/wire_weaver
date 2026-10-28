# WireWeaver features and roadmap

This file is the single source of truth for what WireWeaver supports, what is being worked on and what is
planned, for humans and AI agents alike. It replaces the external task tracker.

## How to use this file

- **Status** of each item:
  - ✅ done
  - 🚧 in progress or partially done (the note says what is missing)
  - 📋 planned
  - 💡 idea, not committed to
  - ⛔ blocked (the note says on what)
  - 🔍 probably done or obsolete, needs a check before closing
  - 🧪 done and tested on the host, the check on hardware is still open (the note says what to check)
- **Target** is the release an item is planned for: `v0.5` is the next release (the workspace is at 0.5.0,
  crates.io has 0.4.0). `v0.5.x` is the next compatible release after it: `v0.5.1` if nothing turns out to be
  breaking, otherwise the next minor. No tag means not scheduled. `grep '`v0.5`' FEATURES.md` gives the release
  scope.
- **IDs** (`SW-3`, `CLT-12`) are stable: never renumber or reuse one. New items take the next free number of their
  area, even when they are inserted in the middle of it. Use the ID in commit messages, CHANGELOG entries and code
  `TODO`s (`// TODO(SRV-4): ...`). When talking to people, name an ID with a short slug from its title
  (`SW-3 string-and-vec`), never bare.
- Items are grouped by area. Each area lists what works first (✅), then open items ordered by target.
- When finishing work, update the item in the same change: mark it ✅, move it up to the done items of its
  area and add a pointer (docs page, crate, test or example). Don't delete done items. Move items that turn
  out to be obsolete to [Dropped and superseded](#dropped-and-superseded) with a one-line reason.
- This file is also rendered as the docs site's "Feature tracker" page (`docs/features.md` includes it), so indent
  nested lists and paragraphs inside an item by 4 spaces, as Python-Markdown requires. The "Features" page
  (`docs/features_today.md`) is generated from it and from `tests/features_page/entries.md`, which has a short line,
  a reading level and a docs link per item: an item that becomes done or in progress gets a line there, then run
  `just features-page`; `just test` fails until both are done.
- New ideas go into the matching area as 💡. Small code-level gaps stay as `TODO` comments in the source;
  only those that limit users or block a feature get an item here.

## Wire format (`shrink_wrap`, `SW`)

- ✅ **SW-1 Structs, plain enums and data enums**, discriminant type selection with `ww_repr`.
- ✅ **SW-2 `Option` and `Result`**, also inside `Vec`, as one-bit flags.
- ✅ **SW-3 `String` and `Vec<T>`**, with special handling of `Vec<u8>`.
- ✅ **SW-4 Fixed size arrays, tuples and the unit type `()`**.
- ✅ **SW-5 Bit-aligned integers**: nibbles and `u1`..`u32` (`un`); `un8`/`un16` are forwarded to `un32` for smaller
  flash.
- ✅ **SW-6 Variable length numbers**: `UNib32`, `UVlq32`, and `UVlq32Backfill` for values known only after the rest
  is serialized.
- ✅ **SW-7 Borrowed and owned types**: `Sized` and `Unsized` traits, `RefStr`/`RefVec`, borrowed types by default and
  `Owned` variants generated automatically.
- ✅ **SW-8 Evolvable by default**: every struct and enum is `Unsized`; `final_structure`, `self_describing` and
  `sized` are opt-ins.
- ✅ **SW-9 `TailBytes` / `TailBytesOwned`**: bytes that take up the rest of the buffer, written without a size; only
  valid as the last item. Generated API code puts them in place of byte slices, e.g. for return values and
  property values.
- ✅ **SW-10 `EitherAnyVec`**: zero-copy, no-alloc array of `Either` elements of any types, with the L/R flags packed
  in groups of 8 instead of padding each element. Packs e.g. a list of `Result`s tightly; used for multi-read
  and multi-call results (only multi-read exists so far).
- ✅ **SW-55 `OptionAnyVec`**: zero-copy, no-alloc array of `Option` slots of any types (numbers, structs, lists,
  strings, a nested `OptionAnyVec` for a child level), only the present values written, in slot order without
  padding between them. A presence header up front in the smallest of four modes (all, dense bits, two-level bits,
  index list), self-delimiting so that readers with fewer or more slots than the writer read it (slots are only
  appended); a malformed header is an error. Writer takes the present set up front (`SetBits` over a bitset, any
  increasing iterator), `OptionAnyVecWriter` / in-place `OptionAnyVecBuilder`. For a trait level's properties in one
  update (API-38). See `docs/serdes/special_types.md#optionanyvec-sparse-slots-of-any-types`, `docs/serdes/showcase.md`.
- ✅ **SW-68 Word-wide bit writer and reader**, the same bytes: `BitWriter` with a 32-bit accumulator flushed a word
  at a time, `BitSink::put_block` for packed blocks, `write_un*` / `write_bool` / `UNib32` and the reader's
  `read_un*` / `UNib32` through one 4-byte window and one bounds check per call. The FOC observe poll on an STM32H725
  75.5 k cycles instead of 99.9 k at `opt-level = 3` (121.5 k against 172.2 k at `"z"`), a FOC level batch from the
  store 36.8 k instead of 58.9 k; reads of UN numbers and `UNib32` take 0.4-0.6x the instructions on a Cortex-M4F;
  about 1.5 KB more flash at `"z"`. See `docs/articles/series.md#the-bit-writer-and-reader-a-word-at-a-time`,
  `examples_mcu/mcu_qemu/src/bit_io.rs` (`qemu-bit-io`, `qemu-m4f-bit-io`, `stm32h725ig-bit-io-cycles`).
- ✅ **SW-64 `Series` written straight from a `SeriesStore`**, column by column, the same bytes as the item walk:
  `SeriesStore::columnar` / `write_store` (leaves, `Option`, arrays, tuples, derived structs and enums, by hand for
  `NumericValue`) and `series::write_store_series`, used by `ww_stream`'s batchers on their own. Needs one variant
  per enum and `Option` over the rows (a prefix change falls back to the walk). One FOC level batch (21 x
  `Series<T>` + clock, n = 32, `DeltaPacked`): 74.9 k instructions on a Cortex-M3 instead of 143.5 k, 100 k cycles on
  an STM32H725 instead of 312 k (60 k against 125 k at `opt-level = 3`). See
  `docs/serdes/series.md#from-the-store-column-by-column`, `shrink_wrap/shrink_wrap/src/series/store_write_tests.rs`,
  `examples_mcu/mcu_qemu` (`qemu-store-columns`).
- ✅ **SW-63 Fast and small `Series` encoding on MCUs**, the same bytes: about 6x fewer instructions per item on
  a Cortex-M3/M4F (`Series<Timed<Ampere>>`, n = 32: 341 with a fixed codec, 969 with `SMALLEST`; 25 200 cycles per
  batch of 32 on an STM32H725), and codec policies fixed at compile time (`SeriesPolicy`, `FixedCodecs`,
  `DeltaPacked`, the `no_std` default of `ww_stream`'s batcher) that link only their own codecs: 15 KB of encoder
  flash on a Cortex-M3 instead of 91 KB (25 KB with a run-time `CodecPolicy`). See
  `docs/serdes/series.md#cpu-and-flash-on-an-mcu`, `examples_mcu/mcu_qemu`, `examples_mcu/qemu_icount`.
- ✅ **SW-62 `SeriesStore` and series size bounds**: room for `N` items of a `series` type as a struct of arrays
  (`empty_store`, `set`, `get`), derived for structs (field stores) and enums (discriminants and every data
  variant's fields), by hand for `ww_numeric::NumericValue` (a variant byte and one payload as wide as the widest
  variant enabled): `Timed<Ampere>` takes 12 bytes per item with number types up to 32 bits, against 24 as items.
  `SeriesColumns::MAX_ITEM_BITS` / `MAX_FIXED_BITS` and `Series::max_bytes(n)`: the worst case of a batch under any
  codec policy, `None` for strings and opaque fields. `series(no_store)` leaves the derived store out. Piece 1 of
  API-41. See `docs/serdes/series.md#collecting-items-on-a-device-seriesstore`,
  `shrink_wrap/shrink_wrap/src/series/store_tests.rs`, `ww_stdlib/ww_si/tests/store.rs`.
- ✅ **SW-61 `RefVecBuilder<T>`: a list written in place**, element by element (`push`, `extend`, `push_with` for a
  nested builder or an element written by hand), `finish` writes the count; same bytes as `RefVec::Slice`, as the
  whole buffer, a struct field or an element of another list. A failed push undoes its element, `with_max_len` for
  `MaxLen<RefVec<..>, N>`, `RefVecBuilderOwned` for `BufWriterOwned`. A byte list also has `extend_from_slice` and
  `write_bytes_with` (a byte blob written by other means), which replace `RefVecU8Builder`.
  See `docs/serdes/special_types.md#refvecbuilder-lists-written-in-place`,
  `shrink_wrap/shrink_wrap/tests/ref_vec_builder.rs`.
- ✅ **SW-59 Tuple and unit structs in the derive**: `struct Celsius(f32);`, `struct Pair<'i>(u8, &'i str);`,
  `struct Marker;` in every layout and size assumption, same bytes as the struct with the fields named; positional
  names `_0`, `_1`, ... in consts, C names and errors. `#[flag]` needs named fields. The API model has them as
  unnamed fields, dynamic clients and Python (a tuple, a single field its value, a unit `None`) read and write them.
  A unit struct is evolvable (it records its length inside a parent); with `sized`, `final_structure`,
  `self_describing` or `fixed_layout` it is a compile error, as it could never change.
  See `docs/types.md`, `shrink_wrap/shrink_wrap/tests/tuple_structs.rs`, `tests/dynamic`.
- ✅ **SW-58 `validate = path` derive directive**: a function checking every struct value just read, so a derived
  type (and the API model it gives to dynamic clients) can still reject out-of-range data; used by `ww_date_time::Year`.
- ✅ **SW-57 `ArrayString<N>` / `ArrayVec<T, N>`**: owned, no-alloc string/list types backed by a fixed-size
  array, same bytes on the wire as `&str`/`String` and `Vec<T>`, for an owned no-alloc field (a `value_on_changed`
  field of a string/list property in a `no_alloc` server, a settings value); `Error::MaxLenExceeded` rather than a
  cut value, for serializing, deserializing, `push_str`/`push` and `TryFrom<&str>` alike.
  `docs/serdes/special_types.md#arraystring-arrayvec-owned-values-without-an-allocator`.
- ✅ **SW-11 Alloc writer** `BufWriterOwned`, `SerializeShrinkWrap` for `&[u8]`, `String` as UFS.
- ✅ **SW-12 In-place builders** used by the server codegen (event and error builders), and the manual
  patch-the-discriminant-later pattern (`docs/serdes/showcase.md`).
- ✅ **SW-13 Binary format documented** in `docs/serdes/wire_format.md`, worked tricks in `docs/serdes/showcase.md`.
- ✅ **SW-14 Usable standalone** as a dense data storage format (`examples/minimal_shrink_wrap`,
  `examples/compare_wire_formats`).
- ✅ **SW-34 Smaller code on small CPUs**: `BufReader` / `BufWriter` reads and writes that keep an error instead of
  returning it (`read_u8_latch`, `write_u32_latch`, .., `latched()`), used by `#[derive_shrink_wrap]` for plain
  fields with one check per type; `Error` without payloads; one shared integer read and write; no division in
  `UNib32`. 19 to 20 % less code for a derived request/reply codec on RV32IC and Cortex-M0, same wire format.
  Numbers, where the rest goes and what is left to do: `docs/serdes/code_size.md` (measured by TEST-17).
- ✅ **SW-36 No length FIFO code for types without lengths**: `SerializeShrinkWrap::PUSHES_LEN`, computed by
  `#[derive_shrink_wrap]` from the fields (default `true` unless `Sized`), lets `to_ww_bytes` and
  `BufWriter::finish_for::<T>()` end with `finish_no_lens()` at compile time, so the FIFO encoder is not linked:
  about 510 bytes less per such encoder on RV32IC (a struct of plain fields 1476 → 964). For types with lengths
  `encode_len_fifo` / `write_nib` are smaller (`vec_ser` 1378 → 1142). Same wire format
  (`shrink_wrap/tests/pushes_len.rs`, `docs/serdes/code_size.md` steps 4 and 5).
- ✅ **SW-37 `RefVec` of plain numbers without the decode pass**: a list of `u8`..`u128`, `i8`..`i128`, `f32` or
  `f64` finds its end with one bounds check instead of reading every element (4096 bytes: 10 µs → 6 ns on a host),
  same result, errors and reader state as the pass, same wire format (`shrink_wrap/tests/ref_vec_skip.rs`,
  `just fuzz-shrink-wrap-ref-vec`). Costs 50 to 140 bytes of code in a decoder with such a list. Other `Sized` types
  keep the pass on purpose (stride is not `size_bits`, elements are validated there):
  `docs/serdes/code_size.md`.
- ✅ **SW-38 Latched reads of `U1`..`U64` / `I2`..`I64` fields**: read like the integers of SW-34, with one error
  check per type (through a hidden `DeserializeShrinkWrap::des_shrink_wrap_latch`); a bit-packed struct decodes in
  40 bytes less on RV32IC (886 → 846), 88 on RV32I. Same wire format, results and errors as reading field by field
  (`shrink_wrap/tests/latched_reads.rs`, `just fuzz-shrink-wrap-latched`). `Nibble`, `u64` and varints are not
  latched: one such field costs more than it saves (`docs/serdes/code_size.md`).
- ✅ **SW-41 First error in types that mix latched and other fields**: an error kept by a `_latch` read stops the
  reader (reads with a `Result` return it, no read moves), so a derived decoder with lists or nested types next to
  plain fields returns the first error again, as before SW-34. 20 bytes on a request/reply decoder (RV32IC), four
  forms measured (`docs/serdes/code_size.md`, `shrink_wrap/tests/latched_errors.rs`).
- ✅ **SW-43 C99 runtime** (first step of API-26): `shrink_wrap/c/shrink_wrap.{h,c}`, the `BufReader` / `BufWriter`
  of the Rust crate for C: bits, nibbles, `un8`..`un64`, LE integers, floats, `UNib32`, `UVlq32` / `UVlq64`, bytes
  and strings as borrowed views, `split`, lengths at the back and `finish`, `Unsized` values. No `malloc`, the first
  error kept as in SW-41 (a failing call moves nothing), octets through accessors so 16-bit `char` (TI C2000) works.
  Checked against the Rust crate with golden vectors and a proptest cross-check in both directions, also on
  truncated and corrupted input (`shrink_wrap/c/tests`, part of `just test`; `just lint` builds the C with
  `-std=c99 -pedantic -Werror`). How to use it: `docs/c.md`. `TailSize`, the series types and the `_latch` /
  `finish_no_lens` code size tricks are not ported.
- ✅ **SW-44 Fixed-layout fast path**: `#[derive_shrink_wrap]` sees when a type's encoding is a fixed little-endian
  byte layout (a `sized` or `final_structure` struct, a `sized` or `self_describing` enum with a `u8` / `u16` / `u32`
  `ww_repr`; fields only `u8`..`u64`, `i8`..`i64`, `f32`, `f64` and arrays of them) and generates an offset const
  per field, `ENCODED_LEN`, and `from_fixed` / `to_fixed` that load and store at those offsets on the slice, no
  `BufReader` / `BufWriter`, nothing generic. The `fixed_layout` directive makes it a requirement (and allows
  nested fixed-layout types): a `bool`, `Option`, list, string, varint or bit-packed field is a compile error naming
  the field. Same bytes as `ser_shrink_wrap` (`shrink_wrap/tests/fixed_layout.rs`, 2000 random values per type).
  `FixedLayout::write_c` prints the matching C reader (offset defines and `static inline` loads) from the same
  derive, `fixed_layout::write_c_header` a whole header: the first generated C of API-26. Code size on Cortex-M0
  for a header of eight integers: decode 484 → 276 bytes, encode 888 → 462 (`docs/serdes/code_size.md`). First user:
  chunky_mcu_updater's plan format (P2625 PROTO-3); it serves the busgen idea of P2401 #9oct-1433 for types
  without dynamic fields. Open: arrays of nested types, a C writer, 16-bit `char` targets in the C reader.
- ✅ **SW-45 Bit fields and a variable last field in the fixed-layout fast path**: `bool`, `U1`..`U32`,
  `I2`..`I32` and `Nibble` get compile-time bit offsets (most significant bit first, a `Nibble` on a 4-bit boundary,
  a byte field after them starting a fresh byte, as `BufWriter` packs them) and are read and written with shifts and
  masks, in Rust and in the C reader (`sw_fixed_bits`, `sw_fixed_sbits`). `Option<T>`, `Result<T, E>` and enums of
  unequal variants are allowed as the last field (one rule for all variable-length fields); their flags are bit
  fields, also where `#[flag]` moves them. `MIN_LEN` and `fixed_len()` next to `ENCODED_LEN` (the longest, the
  buffer to have ready). A nested type cannot share a byte with bits of its parent: a compile error says so. Every
  refusal says why and what to use instead. Byte-equal to `ser_shrink_wrap` on mixed bit runs, a run at the end,
  options, results, relocated flags and enum variants with both (`shrink_wrap/tests/fixed_layout.rs`); the
  generated C is built and run with every warning an error (`just lint-c`). A bit-packed sample on Cortex-M0:
  decode 652 → 276 bytes, encode 1034 → 560.
- ✅ **SW-46 Fixed head, sequential tail (`head_tail`)**: an opt-in wire format of its own for types with lists,
  strings and options anywhere. The head holds every fixed-width field at a constant offset (numbers, arrays, bit
  fields as in SW-45) plus a `u16` count per list or string, the flag bit of each `Option` / `Result` and the head
  of each nested type; the tail holds the variable parts in field order. An enum is its tag, then its variant's
  head and tail. Generated by `#[derive_shrink_wrap(head_tail)]`: offset consts, `HEAD_LEN`, `from_head_tail` /
  `to_head_tail` / `head_tail_len`, the `HeadTail` trait (also implemented for every fixed-layout type) and a C reader
  (head readers plus a `<name>_tail()` walker, `shrink_wrap::head_tail::write_c_header`), built and run by
  `just lint-c`. Non-generic, size first: no `BufReader` / `BufWriter`, no length FIFO, no reversed lengths; the
  tail is a cursor (`head_tail::Tail`) that takes lengths it already knows. Lists are `head_tail::List<'i, T>` (items
  numbers or fixed-layout types of one length, read when iterated). No sw serdes and no owned version: one type,
  one format. Tests: exact bytes, round trips, every truncation, 200 000 random buffers, UTF-8, counts over u16
  (`shrink_wrap/tests/head_tail.rs`). busgen's request and reply enums (size bench `busgen_ht`): 4626 bytes on
  Cortex-M0 and 6004 on RV32IC, against 4408 / 5762 hand-written and 6652 / 8210 derived (P2401 #9oct-1433).
  Open: `head_tail` types as ww API arguments (they have no sw serdes), lists of variable-length items, a C writer,
  `u8` counts for short lists (`MaxLen`).
- ✅ **SW-52 `head_tail` views and variant writers**: `Type::view(buf)` gives a `TypeView<'_>` without making the
  value: the lengths checked and the tail walked once, the head's fields read by accessors at their offsets when
  called (the same code as `from_head_tail`, one field at a time), lists, strings, options and nested views kept
  from the walk; an enum's view is an enum of its variants' views (a tuple variant of one nested type holds its
  view). Every enum variant gets `Enum::write_<variant>(buf, fields..)`: the bytes of `to_head_tail` for that
  variant without making the value (the same `write_ht` code). `HeadTail::View` / `view_ht`, `head_tail::view`.
  Tests: values back from every accessor, writers byte-equal to `to_head_tail`, every truncation, 200 000 random
  buffers agreeing with `from_head_tail`. Size bench `busgen_view`: 4988 bytes on RV32IC and 3894 on Cortex-M0, 13 %
  and 12 % below the hand-written codec; busgen's firmware 5608 / 5090 bytes (UART / I2C image, fpga_tools BUS-12).
  View types are always `pub` (the `View` of a public trait). Open: lazy views of nested fixed-layout types (read
  when the view is made), a writer for structs.
- ✅ **SW-53 Numbers through `fixed_layout::Le`, out of line on RISC-V**: generated fixed-layout and `head_tail` code
  reads and writes multi-byte numbers with `Le::ld` / `Le::st` on the bytes at the field (`fixed_layout::at`), one
  function per type on RISC-V (no unaligned loads), inline elsewhere (on Cortex-M0 the byte loads are smaller than
  the calls, measured). Under LTO `ld` passes its pointer through `black_box`: argument promotion would otherwise
  move the load into every caller. Bit-field bytes cleared once per head. `busgen_ht` on RV32IC 6004 → 5658; a type
  read or written in one place grows (`fixed_ser` 498 → 612, docs/serdes/code_size.md).
- ✅ **SW-54 `#[cfg]` on `head_tail` enum variants**: passed to everything generated for the variant (consts, arms,
  view, writer, C text); a variant compiled out keeps its tag and reads as an unknown one. busgen's firmware builds
  only its bitstream family's requests with it.
- ✅ **SW-39 Byte-wise `memcpy`, measured, advice for very small targets**: a firmware's own `memcpy` (and
  `__aeabi_memcpy` on Arm) of 22 bytes instead of the word-wise one of `compiler_builtins`: 324 bytes less on
  RV32IC, 304 on Cortex-M0, for every image that moves a value, at the price of speed and of replacing `memcpy` for
  the whole firmware. Code and downsides in `docs/serdes/code_size.md` ("Build settings"), measured with
  `just size-bench --fw-features byte_memcpy`. Nothing in `shrink_wrap` depends on it.
- ✅ **SW-40 Bit reads stay out of the lengths in the last byte**: `read_un8`..`read_un64` (and `U1`..`U64`,
  `I2`..`I64`) no longer read into the length nibble the last byte shares with the forward data, and a length is
  no longer read from bits the forward side has read; both are `OutOfBounds..` errors. Malformed input only, every
  message the writers produce decodes as before. Checked against a bit-level model in every reader state of 1 to
  3 byte buffers (`buf_reader.rs` `bounds_every_state_every_read`) and by round trips of random messages
  (`shrink_wrap/tests/rev_nibble_bounds.rs`).
- ✅ **SW-35 Exact `ElementSize` of enums**: a `sized` / `self_describing` enum reports the discriminant plus one
  variant (`ElementSize::one_of`) instead of the sum of all variants without the discriminant; `sized` with variants
  of different sizes is a compile error; user types inside arrays, tuples, ranges and options are counted. Same wire
  format (`shrink_wrap/tests/enum_size.rs`, `docs/serdes/size_assumptions.md`). Unblocks reading `RefVec<Sized T>` in place.
- ✅ **SW-33 `UVlq64`**: byte-based variable length `u64`, same big endian VLQ encoding as `UVlq32`, 1 to 10 bytes,
  `uvlq64` field type in `#[derive_shrink_wrap(..)]`, `BufReader::read_uvlq64`/`BufWriter`/`BufWriterOwned::write_uvlq64`
  (`shrink_wrap/src/vlq64.rs`, `docs/types.md`, `docs/serdes/showcase.md`). No `UVlq64Backfill` (not asked for).
- ✅ **SW-19 Bounded lists and strings `MaxLen<T, N>`**: a `RefVec`, `Vec`, `&str` or `String` with at most `N`
  elements, same bytes as `T` (a bound can be added, raised or removed without a new wire form), checked on
  serialize and deserialize (`Error::MaxLenExceeded`, never truncated). Works in derived types and in every API
  position (arguments, returns, properties, stream and sink items), visible in introspection
  (`ww_self::Type::MaxLen`), honoured by the dynamic client and Python; raising a bound is compatible, lowering or
  adding one breaking (`ww api check`). `SerializeShrinkWrap::MAX_SIZE: Option<MaxSize>`, the worst-case size of a
  type whose every part is bounded, feeds API-18. `docs/types.md`, `docs/evolution/rules.md`,
  `shrink_wrap/tests/max_len.rs`, `tests/bounded`, `examples_mcu/mcu_qemu` (`qemu-bounded`).
- ✅ **SW-15 Handle end of buffer when reading evolved fields**: a field with `#[default = ..]` reads as its default
  only when it starts at the end of the data and runs out of it (`Error::is_read_eob`); a truncated or malformed
  value is an error. The flag of an evolved `Option` / `Result` falls back with it, `#[default]` works on any type
  and its value is used (an `Option` was always `None`). Same in the `ww_self` dynamic decoder. Padding bits of the
  old data's last byte read as 0, so bit-sized fields default to their zero value. Decided: no implicit defaults
  for `Vec`/`String`/`Option` fields without `#[default]`, the attribute is what marks a field as added later
  (and what `ww api check` looks for). `docs/serdes/derive.md#evolving-types`, `shrink_wrap/tests/evolution.rs`.
- ✅ **SW-16 `FutureVersion` enum variant**: an enum ending with `#[future_version] Unknown(u32)` reads an unknown
  discriminant into it instead of failing with `EnumFutureVersionOrMalformedData`, and writes it back; an `Unsized`
  enum skips the newer variant's fields with its size slot, `sized` / `self_describing` ones only survive new unit
  variants. No wire number of its own, no change to `ELEMENT_SIZE`, left out of the API model. The `ww_self`
  dynamic decoder reads an unknown variant of an `Unsized` enum as `<unknown N>`. Added by default since SW-47.
  `docs/serdes/derive.md#enum-variants-from-a-newer-version`, `shrink_wrap/tests/evolution.rs`.
- ✅ **SW-48 Bit-sized fields and variant payloads**: `U1`..`U64` / `I2`..`I64` (`u7`, `ub10`, `i2`.. in the API
  model) as struct fields, enum variant payloads and `#[ww_trait]` method arguments, returns and properties, packed
  bit after bit with a sub-byte discriminant: `enum I2cAddress { Seven(U7), Ten(U10) }` with `ww_repr = u1,
  self_describing` takes 8 or 11 bits instead of 16 or 24 with `u8` / `u16`. Server and client codegen (it panicked
  on such a method argument), `I16` (was missing), `From` / `TryFrom` to and from the backing integer. The dynamic
  client and Python already handled them, now tested byte for byte against derived types; the C runtime has the
  bit reads. Same wire format (`shrink_wrap/tests/sub_byte.rs`, `tests/dynamic`, `tests/methods`,
  `docs/serdes/showcase.md#bit-sized-fields-and-variant-payloads-a-7-or-10-bit-i2c-address`).
- ✅ **SW-47 `#[future_version]` variant by default**: the derive adds `Unknown(u32)` to every `Unsized` enum with a
  free discriminant; `no_future_version` opts out (framework enums whose readers must fail: `ww_self` model,
  `MultiIndex`), `future_version = Name` renames it, `final_structure` / `self_describing` / `sized` enums only get it
  when written. A Rust API change of every such enum (exhaustive matches need the arm), not of the wire or the API
  model. `docs/serdes/derive.md#enum-variants-from-a-newer-version`, `shrink_wrap/tests/evolution.rs`.
- 📋 **SW-66 128-bit numbers in the C runtime** — `shrink_wrap/c` reads and writes up to 64-bit numbers;
  `u128` / `i128` (two `u64` halves in C) are missing. Conformance cases `int.u128.max`, `int.i128.min`.
- 📋 **SW-67 `no_future_version` and `#[series(opaque)]` have no conformance case** — found by the coverage guard
  (TEST-25): no case turns off the `Unknown(u32)` catch-all (SW-47) and then feeds an out-of-range discriminant
  (needs hand-written reject bytes, no valid Rust value produces them), and no `Series<T>` (SW-60) case has a
  field marked `#[series(opaque)]`, so its own column form is untested.
- 📋 **SW-17 Const / magic type** · `v0.5` — a simple type with a generic parameter that serializes a constant value or
  byte string and checks it on deserialization, failing on a mismatch. For example the `magic` field in
  `ww_self` (a plain `u32` today).
- ✅ **SW-18 Size-of-enclosing-type field** `TailSize<N>`: a field the derive macro fills in with the size of the rest
  of the value once it is serialized (a `UVlq32` right-justified in `N` bytes, 1 to 5), in any position, nested types
  each with their own. Readers bound the value by it (a longer buffer stops at the right byte, a truncated one
  errors, newer trailing fields are skipped) or skip it without parsing. Fields before it must be `Sized` or
  `SelfDescribing`, checked at compile time. `docs/serdes/special_types.md`, showcase.
- ✅ **SW-60 `Series<T>`: columnar compressed sequences of any derive type** — a list of `T` (structs, enums with
  data, `Option`, nested) written column by column, one column per leaf field: numbers as a numeric column whose
  predictor (raw, delta, delta of delta) and coder (SW-31 prefix code, bit-packed blocks of 8, constant, plain) the
  writer picks per column, floats plain or SW-32 XOR, enum tags, bools, `Option` presence and strings as runs (a value
  and one entry per change); a column restarts where an earlier enum field (a ww_si prefix) changes. 3 to 8 times
  smaller than a plain `Vec<T>` of ADC samples, self-contained per batch: the column codecs (numeric, 128-bit,
  float, runs, codec choice by counting bits or a fixed `CodecPolicy`, no alloc, malformed input an error), the
  `Series<'i, T>` / `SeriesOwned<T>` types and the `SeriesColumns` trait with impls for the primitives, `Option`,
  tuples, arrays, strings and bytes (one-pass validating skim, parallel item iteration, evolvable structs both ways,
  the design's examples and size tables as golden bytes), fuzz targets `shrink-wrap-series-columns` and
  `shrink-wrap-series-type`; the opt-in `series` directive of `#[derive_shrink_wrap]` for structs (also generic,
  DER-14) and enums, `#[series(opaque)]` fields, compile errors naming the field for what v1 doesn't take, on
  `ww_si` quantities and `ww_numeric::NumericValue` (`shrink_wrap/tests/series_derive.rs`,
  `ww_stdlib/ww_si/tests/series.rs`); docs (`docs/serdes/series.md`, the layout in
  `docs/serdes/wire_format.md#seriest`, worked bytes in `docs/serdes/showcase.md#compressed-columns-seriest`).
  `Series<T>` is usable today in any property, method or stream item; a dynamic (non-generated) decoder and
  batching a stream or property into `Series<Timed<T>>` automatically build on it as their own items (CLT-29,
  API-41), not blocking this one.
- 📋 **SW-42 `MAX_SIZE` on owned types** · `v0.5.x` — `SerializeShrinkWrapOwned` has no worst-case size yet, only
  the borrowed trait has (SW-19); host code sizing a buffer for an owned value uses the borrowed type's.
- 📋 **SW-20 Sub-type numbers** · `v0.5.x` — numbers restricted to a range, like `10..=25`, or a set with gaps, like
  `0..=8, 12, 16`; checked on serialization and deserialization, possibly using fewer bits on the wire
  (`docs/types.md`, "Subtypes").
- 📋 **SW-21 Multi-dimensional arrays** (2D, 3D).
- 🚧 **SW-22 Fuzz `shrink_wrap`** — `fuzz/` has one `shrink_wrap` target, `shrink-wrap-tail-size` (`TailSize` types
  round-tripped, random bytes into their readers); `shrink-wrap-ref-vec`
  (SW-37) came with its feature, `shrink-wrap-option-any-vec` with SW-55 and DER-13 (sparse structs), `shrink-wrap-series-columns` and `shrink-wrap-series-type` with SW-60, `shrink-wrap-nesting` with the nesting depth limit (recursive types at random limits). Round-trip every built-in type and the derive's features.
- 🔍 **SW-23 Tuple enum variants evolution** — check that tuple variants evolve as `docs/evolution/rules.md` says
  (fields can be appended) and add a test.
- 💡 **SW-65 Aligned plain `Series` columns** — numbers byte-aligned and little-endian at their width (numeric
  escape `11 11`, float codec `0010`, an `AlignedPlain` policy), copied from a `SeriesStore`: 34 k cycles per FOC
  level batch on an STM32H725 at `opt-level = 3` against 60 k for column-wise `DeltaPacked` (SW-64), for 2.3x the
  bytes. Built and measured on branch `s/18oct2026-memcpy-fast-sw65`, not landed; may come back after observe cells
  and a `u32` payload in `NumericValueStore`.
- 💡 **SW-24 ZigZag encoding for signed variable length numbers** — consider ZigZag (as in protobuf) so that small
  negative values stay short, e.g. signed counterparts of `UNib32`/`UVlq32`.
- 💡 **SW-25 In-place builder for server handlers** · `v0.5.x` — a new step-by-step / partial builder (e.g. for a
  `RefVec`) might be useful with the new `Context` in server handlers, to write replies and events straight
  into the buffer. It might not work out.
- 💡 **SW-26 Map support** — `HashMap`/`BTreeMap`, probably as a `Vec` of `(K, V)`.
- 💡 **SW-27 In-place editing** of serialized data.
- 💡 **SW-28 Compression wrappers** — the time-series part is done as `Series<T>` (SW-60, with the SW-31 delta and
  SW-32 XOR codes); in-place dictionary compression as `Compression<T>` (strings?) stays an idea.
- 💡 **SW-29 Flatten support** — apply a type's layout across struct/enum boundaries.
- 💡 **SW-30 `derive_shrink_wrap` on a whole module at once**. For definitions in separate files with own syntax, see
  [standalone definition files](#api-model-and-codegen-api).
- 💡 **SW-56 Tri-state slots in `OptionAnyVec`** — absent / value / error per slot, e.g. a property update that says
  why one value is missing. The header has room for it: mode nibbles `4..=15` are reserved (a reader fails on them
  today), so 2-valued vecs don't pay for it.
- ✅ **DER-1 `#[derive_shrink_wrap(..)]` attribute** with clear syntax, no hidden magic and good error messages
  (`docs/serdes/derive.md`); replaced `#[derive(ShrinkWrap)]`.
- ✅ **DER-2 Discriminant checks**: discriminants must fit `ww_repr`, implicit ones are numbered like in Rust.
- ✅ **DER-3 `#[default = ..]` for evolved fields** and `#[flag]` relocation.
- ✅ **DER-4 `TailBytes` must be last**: rejected anywhere but as the last field of a struct or enum variant, and nested
  in `Option`, `Vec`, tuples or arrays (`check_tail_bytes_position`), like `UVlq32Backfill` must be first.
- ✅ **DER-10 Generic arguments on user field types**: `Foo<u8>`, `Wrapper<'i, T>` are emitted as written,
  `Wrapper<'i, T>` maps to `WrapperOwned<T>` in the owned variant (`docs/serdes/derive.md`, type mapping; test
  `shrink_wrap/tests/generic_external.rs`).
- ✅ **DER-14 Generics on derived types**: structs and enums with type parameters (`struct Timed<T> { t: Second,
  value: T }`, `enum Reading<'i, T>`), const parameters of any type (`struct Buf<const N: usize> { data: [u8; N] }`,
  `MaxLen<&'i str, N>`, `const STRICT: bool`), defaults and where clauses, also with a lifetime (`Tagged<'i, T>` /
  `TaggedOwned<T>`); each impl bounds the type parameters by its trait (serdes, owned serdes, `SeriesColumns`) on top
  of the bounds and where clause as written, the size of `T` and `N` is part of the type's. `series`, `display`,
  `validate` and `discriminants` take generics, `fixed_layout`, `head_tail` and `sparse` reject them
  (`docs/serdes/derive_reference.md#generics`; tests `shrink_wrap/tests/generic_items.rs`, `series_derive.rs`;
  `ww_si::Timed<T>`). In APIs and the API model: API-43.
- ✅ **DER-15 `PhantomData` fields**: `PhantomData<T>` is zero bits (`Sized { size_bits: 0 }`, like `()`) borrowed
  and owned, and a `SeriesColumns` with no columns, for any `T`; the derive leaves a type parameter used only inside
  `PhantomData<..>` without the serdes / `SeriesColumns` bounds of DER-14, so `struct Quantity<U> { value: i32,
  _unit: PhantomData<U> }` works with a bare unit marker and writes the same bytes as without the field
  (`docs/serdes/derive_reference.md#generics`; test `shrink_wrap/tests/phantom_data.rs`). For `ww_si` units.
- ✅ **DER-17 `#[default = ..]` fields default in serde too**: a type deriving `serde::Deserialize` (through
  `derive(..)` or a `cfg_attr`) gets `#[serde(default = ..)]` on each `#[default = ..]` field of a struct or struct
  variant, so RON or JSON written before the field was added still reads; for saved `ww_self` API snapshots when a
  later model field isn't an `Option` (EVO-13) (`docs/serdes/derive.md#evolving-types`; test
  `shrink_wrap/tests/serde_default.rs`).
- ✅ **DER-19 `#[wire(W)]` field attribute**: a field keeps a plain Rust integer (`i8`, `u16`, `u64`, ..) and is
  written as a narrower or variable-length wire type (`#[wire(I4)] i8`, `#[wire(UNib32)] u64`, `#[wire(U12)] u16`):
  checked on write (`Error::SubtypeOutOfRange`, never a cut value), widened on read, same bytes as a field of type W
  (runtime trait `shrink_wrap::WireAs`). Struct and enum fields, `#[default = ..]`, `sized`/`final_structure`,
  `#[cfg_attr(pred, wire(W))]`, `series` columns (`series::Wired<W, T>`, a W column). The API model records W (the
  walker reads it, also inside `cfg_attr`), so the dynamic client, Python and C need nothing new
  (`docs/serdes/derive_reference.md#wirew-field-attribute`; tests `shrink_wrap/tests/wire_as.rs`, conformance
  `wire.*`). For `ww_si` exact units (`prj/P2401-wireweaver/design/ww_si_in.md`).
- 💡 **DER-21 `#[wire(W)]` beyond plain fields** — inside `Option`, arrays and lists (`Option<#[wire] u8>` has no
  syntax yet: a wrapper type or `#[wire(Option<U4>)]`), and in `fixed_layout` / `head_tail` types, which reject
  the attribute with a compile error naming DER-21 today.
- ✅ **DER-18 `transparent` derive directive**: a struct with one field that isn't zero bits (`PhantomData`, `()`)
  takes that field's size class (`ELEMENT_SIZE`, `PUSHES_LEN`, `MAX_SIZE`), series columns, store and cells, and
  writes exactly its bytes in every position, borrowed and owned, generic, with `validate` and `display`; compile
  errors name the type for zero or two real fields and for the directives it excludes. Recorded in the API model
  (`ItemStructExtra::transparent`, read by the walker, `ValueOwned`'s decoder and `Series` columns, so the `ww` CLI and
  `wire_weaver_py` read it too). For `ww_si::Fixed` (STD-55), where a `final_structure` wrapper was
  `UnsizedFinalStructure` even around a `u64` and couldn't sit in a `sized` parent. Also fixed: a generic parameter
  named `P`, `I`, `S` or `C` clashing with the derive's own in series code (`PhantomData<(U, P)>` included), and
  `#[repr(..)]` of a derived struct being dropped (`docs/serdes/derive_reference.md#transparent`; tests
  `shrink_wrap/tests/transparent.rs`, `phantom_data.rs`, conformance cases `transparent.*`). The evolution
  checker compares wrappers by meaning: EVO-15.
- ✅ **DER-20 Unit-typed fields cost what plain fields cost**: a `transparent` wrapper (DER-18, e.g.
  `exact_units::Fixed`) inside a derived struct is written and read on exactly the path of its inner plain field:
  the latched `write_*_latch` / `des_shrink_wrap_latch` calls with one error check per type (SW-34, SW-38), not the
  wrapper's own `ser_shrink_wrap` with a checked write per field. Hidden `ser_shrink_wrap_latch` (borrowed and owned)
  with `BufWriter::write_field`, used by the derive for every user-type field; a wrapper with `validate` (`Fixed`'s
  bounds) keeps a value out of range as the latched error (`BufReader::keep_error`). TEST-27 had measured +2 to 15 instructions and +4 to 42 bytes per struct encode/decode on M0, M4F and
  M33 before it; same wire format (`docs/serdes/derive_reference.md#transparent`,
  `docs/serdes/code_size.md#transparent-fields`; test `shrink_wrap/tests/transparent.rs`
  `fields_are_latched_like_the_plain_field`, fuzz `shrink_wrap_latched`).
- ✅ **DER-11 Field type from a macro_rules `$T:ty` fragment**: accepted like the type written directly, instead of
  rejected as an unsupported `syn::Type::Group` (test `shrink_wrap/tests/macro_ty_fragment.rs`).
- ✅ **DER-12 `Display` from doc comments**: a `display` directive writes `core::fmt::Display` and
  `core::error::Error` (borrowed and owned) from the first doc line of each variant/struct, `{field}` filled in,
  `#[display("..")]` to override; a variant with neither is a compile error. Error enums need no hand-written
  `Display` on hosts (`docs/serdes/derive_reference.md`; test `shrink_wrap/tests/display.rs`).
- ✅ **DER-13 Sparse structs**: `#[derive_shrink_wrap(sparse, ..)]` on a struct of `Option<T>` fields (borrowed and
  owned) encodes it as an `OptionAnyVec` (SW-55), one slot per field in declaration order or at `#[slot = N]`
  (increasing, gaps allowed, a present value in a gap is `Error::OptionAnyVecUnknownSlot`); a nested sparse struct
  is one slot. Fields are only appended. A non-`Option` field or a combination with `final_structure` / `sized` /
  `self_describing` / `fixed_layout` / `head_tail` is a compile error. For the per trait level `<Level>Update` of
  SRV-18 (`docs/serdes/layouts.md`, `docs/evolution/rules.md`; test `shrink_wrap/tests/sparse.rs`, fuzz
  `shrink-wrap-option-any-vec`). `ww_self` carries the flag (`ItemStruct::sparse`, `Field::slot`, TOOL-11), so a
  dynamic decoder (`ValueOwned`, the `ww` CLI, `wire_weaver_py`) reads a sparse struct correctly.
- 📋 **DER-5 `#[since = "x.y.z"]` on fields** — generate correct evolution code from it (`TODO` in
  `shrink_wrap_derive/src/lib.rs`).
- 📋 **DER-6 Check that `TailBytes` ends the outer type too** — a struct ending in `TailBytes` used as a non-last field
  of an outer `sized` / `final_structure` type ("last in the first `Unsized` type on the way up") is not visible to
  the derive; needs a marker on the type, e.g. an associated const, and a compile-time assertion in the outer type.
- 📋 **DER-7 Prefix enum discriminants** · `v0.5.x`.
- 💡 **DER-16 Where clauses naming the borrowed lifetime, for owned types** — a generic type bounded on its
  lifetime (`enum Batched<'i, T> where Timed<T>: SeriesColumns<'i>`) can't be derived with `owned`: the owned side
  has no `'i` and would need the bound as `for<'i> SeriesColumns<'i>`. `ww_client_server::observe::Batched` /
  `BatchedOwned` (API-39) are written by hand for that.
- ✅ **DER-8 `#[cfg(..)]` on enum variants** — a variant keeps its discriminant in every configuration, a build without
  it reads `Error::EnumVariantDisabled` (`docs/serdes/derive_reference.md`). First user: `ww_numeric::NumericValue`, one cargo
  feature per number type (32 to 8 bytes in RAM with 32-bit types only). Not yet: `#[cfg]` on fields (it would change
  the wire format, should be a compile error), `ww_si` enums.
- ✅ **DER-23 A field written as a user type**: `#[wire(W)]` (DER-19) with any `W: WireAs<T>`, not only integers: a
  field keeps a handle in Rust and writes the value it stands for (`exact_units::Prefix::Exact(#[wire(Magnitude)]
  ExactScale)`: one byte in Rust, the full magnitude on the wire, STD-59). In a `Series` the field is `W`'s column
  (`series::WiredRef`, `WireAsRef` lends the value) while store and cells keep the handle. Tests
  `shrink_wrap/tests/wire_as_user.rs`.
- ✅ **API-1 Methods, properties, streams and sinks**, nested traits (`docs/api/overview.md`).
- ✅ **API-2 Arrays of resources** with `ValidIndices` and a built-in array size getter returning a range or list.
- ✅ **API-3 Global traits** from `ww_stdlib` (`impl Trait`) and trait attachments (`examples/all_gpio`).
- ✅ **API-4 Results**: methods return `RpcResult` (`Ready`, `Defer`, `Unimplemented`), properties return
  `GetResult`/`SetResult`, with user errors and property errors.
- ✅ **API-5 Request `Context` in every handler**: seq, medium, event out, deferred replies.
- ✅ **API-6 Event, `EventKind` and error builders**.
- ✅ **API-7 `match` on index slices**, the base of multi-read support.
- ✅ **API-8 `build.rs` codegen** via `gen_server`/`gen_client`, as an alternative to the proc macro.
- ✅ **API-9 Full name chains** in generated names (fixed the `event_sideband` collision).
- ✅ **API-10 Type paths from Cargo**: trait source path, external types in API crates as full paths.
- ✅ **API-11 Sync and async servers, async / blocking / promise clients** from the same trait.
- ✅ **API-28 Credit-based flow control for sinks** — `#[credit] sink!(..)`: the device grants credit in bytes of
  items (the serialized value of each write, never split; the grant of an `Ack` since API-34), so a receive
  buffer holds as many items as fit whatever their sizes; the host waits when out of credit (async, blocking,
  `try_send`), writes carry the byte position of their first byte so a lossy medium resumes with go-back-N from `have`
  (`Resend`, `Query`), `flush()` waits for the final ack. No deadlock: generated `<SINK>_MAX_ITEM_BYTES` (from
  `MAX_SIZE`, `MaxLen` bounds included) and `SinkCredit::sized_for()` check at compile time that the buffer holds the
  largest item and cap the re-grant threshold, the host fails an item bigger than the buffer with
  `StreamError::ItemTooBig`. Device state in `ww_stream::sink::SinkCredit` (API-34), dynamic and Python clients too.
  Used by `ww_dfu` `StageBulk::plan`. For bulk transfers (chunky OTA plans over LTE-M/LoRa/UDP), UART bridges, CAN
  tx. See also CLT-18 backpressure (`docs/api/credit_sinks.md`, `tests/credit_sink`).
- ✅ **API-42 Credit-sink positions without a session limit** · `v0.5` — host (`SinkTx`) and device (`SinkCredit`)
  count positions in `u64`, the wire carries them modulo 2^28 (`ww_stream::sink::POS_MODULUS`, at most 4 bytes as a
  `UVlq32`), the receiver extends them from the position it expects (serial number arithmetic). The
  buffer is at most `MAX_WINDOW` (64 MiB, a quarter of the modulus), checked at compile time by `sized_for`, and the
  host never has more in flight; one fixed modulus on every link, nothing negotiated at the open. Tested over a seeded
  lossy link (loss, duplicates, reordering, reconnects) with sessions starting near the wrap and far past 4 GiB
  (`docs/api/stream_protocol.md#positions-wrap-at-256-mib`).
- ✅ **API-43 Generic types in APIs** · `v0.5` — generic structs and enums (DER-14, `Timed<Ampere>`, `Window<4>`,
  `Reading<i16>`, `Tagged<'i, T>`) as method arguments and return values, properties, stream items and fields. The API
  model (`ww_self` 0.3) has generics: each definition once with its type and const parameters (`Type::Param`,
  `Length::Param`), each use a `Type::Instance` with its arguments (types, integer/bool values, a const parameter
  passed on, defaults filled in by codegen); generic definitions are in crate snapshots, so `ww_si::Timed` is left
  out of introspection data like any known type. Generated code uses the types as written. Hosts check untrusted
  introspection data and instantiate it (`ww_self::generics::prepare`: indices, argument count and kinds, no
  cycles through generic definitions, nesting depth, number and size of instances bounded; dynamic decoding bounded
  by a value budget), so the
  dynamic client, `ww` and Python see concrete types; the evolution check compares parameters by position and kind
  (`docs/types.md#generic-types-in-an-api`, `docs/evolution/rules.md#generic-types`; tests `tests/generics`,
  `ww_self` unit tests, `fuzz` target `introspection`). Breaks the introspection format: hosts and devices must be
  built with the same `ww_self`. Generic traits (`#[ww_trait] trait T<X>`) are not supported.
- ✅ **API-44 Self-referential types in APIs** · `v0.5` — types that contain themselves through `RefBox` or a list
  (`enum Expr<'i> { Neg(RefBox<'i, Expr<'i>>), Lit(i32) }`, `struct Tree<'i> { children: RefVec<'i, Tree<'i>> }`),
  directly or through other types, as method arguments and return values, properties, stream items and fields. Codegen
  puts each definition into the API model once, referring to its own index; a cycle through a generic definition is
  an error. Hosts check untrusted introspection data (`ww_self::generics::validate`: cycles found with Tarjan's
  algorithm, only through `Box`/`Vec`, never through a generic definition, depth bounded by the sum of a cycle's
  definitions), and every walk over types stops at a definition it is already in (layout, compatibility check, names,
  definitions, default values: the first enum variant that doesn't contain the enum); values decode as deep as the
  reader allows (`MAX_DECODE_DEPTH`, ~90 levels on a 2 MiB stack in a debug build), Python objects are depth bounded
  (`docs/types.md#self-referential-types-refbox`; tests `tests/recursive`, `ww_self`/`wire_weaver_core`/
  `wire_weaver_client` unit tests, `fuzz` target `introspection`). `ww_si::SIExpr` is one (STD-37).
- ✅ **API-45 Type aliases in APIs** · `v0.5` — `pub type Volt = Quantity<VoltUnit>;` and generic aliases
  (`type Reading<T> = Timed<Quantity<T>>`, const parameters too) in every API position, found like structs and enums
  (lib.rs, modules, `use` imports, globs, other crates); the right-hand side is converted in the alias' crate with each
  parameter replaced by its argument. The API model has no alias definitions (what an alias names is used directly,
  ww_si generic D2 a); alias definitions in `ww_self` are an idea for later. `PhantomData` fields (DER-15) are left
  out of a struct's definition, the parameter stays in its generics (`ww_self::generics::prepare` accepts that). Tests
  `tests/generics` (property, method argument, inside `Timed`, a `PhantomData` marker, dynamic client),
  `wire_weaver_core` unit tests, `ww api tree` (`docs/types.md#type-aliases`). `use .. as ..` renames are followed
  (API-46).
- ✅ **API-47 Resend and resume from inside a multi-unit event** · `v0.5` — an event of several units (a block, a
  batch, API-41) may be asked for again from a position inside it: a `Resend` there, or a resume from a cursor
  user code confirmed in the middle of it (`AckMode::Explicit`). The device sends the event that holds the position
  whole (`StreamStore::read` returns it for every store), the host's `Receiver` takes it with
  `Verdict::Accept { skip }` (also for ranges it had before a resume), `PositionedStream` gives the new units. Fixed
  with it: `Resend` no longer counts as an acknowledgement on the device (the store kept less than an explicit
  host had confirmed), acknowledgements and resends are valid up to the highest position sent (a late `Resend`
  on a reordering link stalled a backfill), the store is released when the backfill run ends in `next_event`. Tests
  `ww_stream` `tests/sim.rs` (batches of 1..64 units, a resend from every position inside, a resume cursor inside a
  batch, chaos with acks inside batches), `wire_weaver_client` `stream_e2e_tests`
  (`docs/api/stream_protocol.md#positions-inside-an-event`).
- ✅ **API-46 Codegen names types by their module path** · `v0.5` — each struct and enum records the path other crates
  name it by (`ItemStruct::path` / `ItemEnum::path` in `ww_self`, `None` at the crate root): the shortest one through
  `pub mod`s and `pub use` re-exports (by name, renamed, glob); a type re-exported under another name is named that
  way, a type no other crate can reach is an error naming it. Its identity in its crate is path and name
  (`units::Volt` and a root `Volt` are two definitions): generated code names types by it (a type in a `pub mod`
  needs no root re-export, the ww_si 0.2 layout of a marker `units::Volt` next to the alias `Volt` works), snapshots
  take `#[derive_shrink_wrap]` types and unit marker structs from `pub mod`s too, and `SkippedFullVersion::type_name`,
  signature resolution, `inline_skipped` and the definition cache look definitions up by it. Part of signatures, so
  introspection data and API hashes changed (snapshots saved again). The walker follows `use .. as ..` imports, and
  converts the arguments of `a::b::Ty<Arg>` where they are written. Tests `tests/module_paths` (pub module, lifetime
  two modules deep, rename, root re-export, re-export under another name, two equal `Point`s,
  `Quantity<units::Volt>` next to a root `Volt`), `tests/generics` (ww_si layout as property, method argument,
  inside `Timed`), `wire_weaver_core` unit tests (a snapshot with `units::Volt` and a root `Volt` saved through RON
  and put back in an API) (`docs/types.md#types-in-modules`).
- ✅ **API-31 Stream functions** — `#[stream(T, counted)] fn name(args) -> Result<A, E>` and `#[counted]` /
  `#[reliable] stream!(name: T)`: every stream (plain ones too) is opened by calling it (`RequestKind::StreamOpen`),
  with typed arguments and result; the device's handler named like the stream decides, `Ok` starts a session,
  `close_<name>` closes it. Counted and reliable streams keep their state in what `stream_<name>()` returns
  (`ww_stream::tx::Counted` / `Reliable`), resume by cursor (`name_resume(cursor)`), answer a read with their
  `StreamStatus` (`name_status()`), send with `stream_data_ser().<name>_send` (counted) or push into the store with
  `<name>_push` (reliable), which the generated `poll_streams` sends by priority between requests and resends on the
  host's `Resend`; `ww api scaffold`, introspection (`ApiItemKind::StreamFn`), compatibility check and API diff
  know them. Includes API-15. Not yet: reliable streams in resource arrays (`poll_streams` refuses them), the open
  arguments in `StreamStatus`, stream functions in the dynamic client and Python (CLT-23), streams that detach by
  themselves when the link goes down (a resume re-attaches) (`docs/api/stream_reliability.md`,
  `tests/streams_reliable`).
- ✅ **API-16 Optional items** — `#[optional]` / `#[optional(group)]` on a trait's methods, properties and streams;
  a server leaves them out with `ww_codegen!(.., omit = ["i2c.pullups"])` or `omit_if(<cfg>) = [..]` (the user
  crate's features), pays no dispatch arm and writes no handler (about 100 bytes of flash per item on Cortex-M). Only
  optional items can be left out (compile error otherwise). A call answers `Unimplemented` from the level's fallback
  arm (`id < ITEM_COUNT`), the device's model lists the omitted items (`ww_self` `omitted`, `root_omitted`), so the
  generated and dynamic clients and Python fail locally with `NotImplementedByDevice`; `ww api scaffold --omit`, CLI
  tree and Python show them (`docs/api/optional_items.md`, `tests/traits`). Whole traits: API-54. Not yet: the GUI
  (`wire_weaver_tool`).
- ✅ **API-54 Optional traits** — `#[optional]` / `#[optional(group)]` on a `ww_impl!` (a single trait or an array),
  left out with `omit` / `omit_if` like API-16 items: no dispatch below it and none of its handlers (cfg-gated with
  `omit_if`), its consts out of (or cfg-gated in) the consts trait, its streams not sent by `poll_streams` (one per
  `omit_if` combination, same signature), the device's model marks it left out so clients fail every call below it
  locally; `ww api scaffold` and `ww api tree` show it, `ww api check` classifies `#[optional]` changes
  (`docs/api/optional_items.md#optional-traits`, `docs/evolution/rules.md#optional-items`, `tests/traits`,
  `tests/streams_batched`). Not yet: `+observe` properties of a trait left out with `omit_if` aren't observed in
  either build.
- ✅ **API-32 Per-method error lists** — `#[errors(Common(InUse), Nack, ..)]` on a trait method, a stream function
  (the open's `Result` error) or above a `property!` with an error type (its reads and writes): checked against the
  error enum when the trait is parsed (a compile error on a wrong name), in the API model (`ww_self` `Method`,
  `StreamFn` and `Property` `errors`), generated client and rustdoc `# Errors` with each variant's doc line, `ww api
  tree`, Python `help()`, `ww api check` (compatible); the generated and dynamic clients warn when a device returns a
  variant outside the list, tests fail (`tests_common`). Nothing on the device (`docs/api/methods.md#error-lists`,
  `tests/methods`, `tests/properties`, `tests/streams_reliable`). Not yet: the generated client's property reads
  (their errors are raw bytes, so only the dynamic client checks them); the GUI (`wire_weaver_tool`).
- 📋 **API-12 Calculate max path depth in server codegen** · `v0.5` — `MAX_DEPTH` is hardcoded to 16 in
  `wire_weaver_core/src/codegen/api_server.rs`.
- 📋 **API-13 Error system, remove `unwrap`s from codegen** · `v0.5` — report errors with spans instead of panicking.
- 📋 **API-56 care-marker Care marker on switching writes** — a typed attribute (the `ww_self::Meta` of SRV-34) on
    a method or property whose call or write switches power or outputs (a rail, a relay, a motor enable), carried in
    introspection: GUIs ask to confirm, `ww call` / `ww set` want `--yes`, the hub can log who did it. Wire
    compatible, nothing on the device.
- 📋 **API-57 device-metadata Device-level metadata** — typed attributes on the API root or in a stdlib trait for what
    a device is: product, board revision, connectors (with the resources behind each, so a label like "J4" points at
    pins and buses), supported clock sync (`ww_timebase` host sync and its sources); in introspection next to
    SRV-34's per-item `Meta`, shown by `ww info`, the hub's registry and GUIs. Design first: which part is fixed in
    the API (connectors, sync support) and which is per unit at run time (`ww_board_info`, serial, revision).
- 💡 **API-33 Narrow error types per method** — generated per-method error enums with the crate enum's
  discriminants, as server handler return types only (`narrow_errors = true`), for a compile-time contract on the
  device. Explored and not recommended for clients: friction in every driver crate.
- ✅ **API-35 Stream functions with a borrowed error type** · `v0.5` — a stream function whose `Result` error has a
  lifetime (`#[stream(I2cRecord<'i>, counted)] fn records(..) -> Result<(), I2cError<'i>>`, every IO family error
  since it holds `CommonError<'i>`) compiles on the server with the handler signature `ww api scaffold` prints
  (`RpcResult<Result<(), I2cError<'_>>>`, an error borrowed from the server), for plain, counted and reliable
  streams, sync and async servers: the generated dispatch takes the `Ok` value out of the result before it opens
  the stream's state (`tests/streams_borrowed_error`). An `Ok` value that borrows from the server still can't.
- ✅ **API-36 Const items served from a value** · `v0.5` — a `const NAME: T;` of a `#[ww_trait]` is served without a
  `get_<name>` handler: the server struct implements the generated `<RootTrait>Consts` with `const NAME: T = ..;`
  (a Rust const) or, picked with `const_model = "NAME=fn"`, `fn name(&self) -> T` for a value fixed at boot or link
  time (OTP, chip id, build id); names carry the trait item chain prefix (`BOARD_BOARD` / `board_board`).
  `ww api scaffold --const-model` prints the impl. Clients read a const once per connection, then answer from a
  cache (generated, dynamic and Python clients, promises too); `ww introspect` shows const values
  (`tests/consts`, `docs/api/properties.md`). `ww_firmware_info`, `ww_board_info`, `ww_uid`
  and `ww_user_info` servers use it.
- ✅ **API-38 Observe requests** · `v0.5.x` — `RequestKind::Observe` on a trait level: per property `Off`, `OnChange`,
  `Periodic` or `OnChangeOrBeat` and a byte budget, the device answers with what it granted; updates arrive as one
  `EventKind::PropertyUpdate` per level carrying any subset of its properties (SW-55). Only `+observe` properties
  can be observed. Protocol `ww_client_server::observe` (`docs/std_library/ww_client_server.md`), device side SRV-18,
  host side CLT-28 (`docs/api/observing.md`, `tests/observe`).
- 💡 **API-37 Const values in the introspection blob** — a codegen option putting the values of `const` items into
  the device's introspection data, so `ww info` gets them with the link setup and no extra reads. Costs flash per
  device, so not a default.
- ✅ **API-41 Batched streams and properties on `Series`** — a stream or property opts into batching
  (`+batched`/`batched`, no size in the API: the device owns the batch size through its own config, default 32,
  the host picks latency) and sends `Series<Timed<T>>` (SW-60) batches instead of one item per event (device-side
  `Batcher`, reliable streams storing encoded batches so a resend costs no re-encoding); the same mechanism under
  API-39's batched property samples and a batched observe slot. Designed, with motor-demo numbers backing it
  (integer storage over floats, default batch 32, autoranging cost cheap); the batch store and size bounds are in
  (SW-62). In: the device side, `ww_stream::batch` (`Batcher` with two stores handed between a producer, also an
  interrupt, and the event loop through one atomic word, sealing on size, age and budget, encoding in the event
  loop; `Outbox` for observe; `Batched` reliable and counted streams), and batched streams end to end: `batched` /
  `batched(untimed)` in `#[stream(..)]` and `#[batched]` on `stream!` for plain, counted and reliable streams,
  `StreamOpen { max_age_us }`, generated `<name>_push` (O(1), interrupt-safe) and `poll_streams` encoding sealed
  batches, typed clients giving items one by one with their own time or whole batches (`recv_batch`),
  `tests/streams_batched`; the dynamic client giving the same (`DynPositionedStream`/`DynStream`, no generated
  code, `ApiItemKind::StreamFn::batch_item_ty`), `ww stream <path> [--batches]` and Python's
  `recv_batch()` (numpy columns, `ww_self::SeriesColumn::from_items`) (piece 6, no-arg streams only); batched observe is
  API-39. Docs: `docs/api/batching.md` (how-to, sealing, budgets, the motor demo's numbers). Motor demo
  (`motor_sim` repo): every `Foc` property and `Control`'s 13 measured values are `+observe+batched`.
- 🧪 **API-55 Compressed counted streams** · `v0.5` — a counted stream's events carry their serialized item as
  one LZ4 block that copies from the items before it (history kept across events, SRV-27 `ww_lz4`), so a byte
  stream of text (a UART at 6 Mbaud of shell or TUI output, 3-5x) fits a slower link. Negotiated at open:
  `StreamOpen { codecs }` is the host's offer, `StreamOpened { codec }` the device's answer (`StreamCodec::Lz4 {
  history }`), so an older host gets the events as they are; an item that doesn't shrink goes as a block of literals.
  `HeadExt::codec_restart` marks where the history starts over: at open and resume, after an event the device
  couldn't send, and every 32 KiB, so a link loss heals; the host drops what it can't decode until then and reports
  it as a link gap (`StreamStats::lost_codec`, `codec_bytes_in` / `_out`). The API doesn't change: the device opts
  in per stream with its state, `ww_stream::codec::Compressed` (feature `lz4`) instead of `Counted`, through
  `ww_stream::codec::CountedState`; `ClientConfig::stream_compression(false)` turns the host's offer off. Typed and
  dynamic clients and Python decode alike (`docs/api/stream_compression.md`; tests `ww_stream` `tests::codec`,
  `wire_weaver_client` `compressed_stream_decodes_and_heals`, `tests/io_uart` `rx_compressed`). To check on
  hardware: Donguru's UART rx at 6 Mbaud (typec_dongle FW-42). Not yet: a restart the host asks for, batched and
  reliable streams, sinks (host -> device).

## Server / device side (`SRV`)

- ✅ **API-39 Batched property samples (observe every sample)** — a `ro+observe+batched` property observed in
  `ObserveMode::Samples { every_us, max_age_us }` sends every sample the device takes as a `Series<T>` in its
  `Batched<T>` slot of the level's update, their times shared by the level (API-49)
  (`docs/api/observing.md#every-sample-batched`, `tests/observe_batched`). In: parsing and the model
  (`ApiItemKind::Property::batched`), the mode
  validated and clamped (`ObserveLimits::samples_max_age_us`), generated `<Level>Sampler::sample(t, ..)` (O(1),
  interrupt-safe after `ObserveBatches::split()`), double-buffered stores and a level outbox encoded in
  `poll_properties` with several updates per call, `missed` for lost samples and dropped batches, typed observers
  per sample, the dynamic client, `ww observe --samples` and Python (`recv()` samples, `recv_batch()` columns).
  Motor demo (`motor_sim` repo) on it, measured through the real generated code: `docs/api/batching.md`, and
  `sample()` itself in an interrupt on an STM32H725 next to observe cells (API-50,
  `docs/api/batching.md#observe-cells-measured`).
- ✅ **API-49 One time column per observe level (shared clock)** — the samples a `sample(t, ..)` call takes for a
  level's `+batched` properties share one time column per update: `EventKind::PropertyUpdate::sample_times`
  (`Series<Second>`) next to `t`, the `Batched::Samples` slots untimed, `Batching::SharedClock` in introspection. A
  level keeps one batcher of rows (time and every value), its `Samples` entries share the finest `every_us` and the
  shortest `max_age_us`. Against a time column per property (21 FOC properties, n = 32, `DeltaPacked`): -36 % bytes,
  -52 % encode cycles on an STM32H725, -46 % instructions on Cortex-M3/M4F
  (`docs/api/observing.md#which-rates-fit-an-mcu`, `tests/observe_batched`).
- ✅ **API-60 Types from crates that aren't API crates** · `v0.5` — a crate without `wire_weaver` or `#[ww_trait]`
  (`exact_units`, STD-61) is walked and snapshotted like an API crate; generated code names its types through a
  crate that re-exports it (`ww_si::exact_units::..`, `CratePaths` from `load_dep_with_paths`) when the generating
  crate doesn't depend on it; the walker follows `use dep as name;` and `use dep::module;` for leading path
  segments. With `shrink_wrap` optional (`#[cfg_attr(feature = "shrink_wrap", derive_shrink_wrap(..))]`) the walker
  reads `cfg_attr`s with the features Cargo builds the crate with (dependency entries, `[workspace.dependencies]`,
  `dep/feature`, defaults, unified across dependents), snapshots with all features; a field's or variant's
  `cfg_attr` of the derive's own attributes (`default`, `wire`, ..) is read as holding by the derive and the walker,
  a compile error where it doesn't; other field and variant attributes pass through to the generated type. Tests
  `tests/standalone` (server and client without a dependency on `standalone_units`), `wire_weaver_core` unit tests,
  `shrink_wrap/tests/cfg_attr.rs` and `field_attributes.rs`, conformance `cfg_attr.*`
  (`docs/types.md#types-from-crates-that-arent-api-crates`).
- ✅ **API-14 API crates with git and crates.io dependencies** · `v0.5` — the crate walker (`#[ww_api_root]`,
  `#[ww_trait]`, `ww_codegen!`, `ww api`) follows `{ git = .. }` and `"version"` / `{ version = .. }` dependencies
  (also through `[workspace.dependencies]` and `package = ".."` renames), not only path ones: the exact package from
  the nearest `Cargo.lock` (the entry in the dependent's own list, so two versions of a crate are told apart), its
  source where Cargo fetched it (`CARGO_HOME`'s `git/checkouts/<repo>-*/<rev>/`, the member found by package name
  anywhere in the repository; `registry/src/*/<name>-<version>/`), clear errors naming `cargo fetch` when the lock
  or the source is missing. In a crate Cargo fetched itself (a published API crate's own `#[ww_trait]`, docs.rs),
  crates.io dependencies are the newest matching version on disk, as the lock in use is out of reach. Tests
  `wire_weaver_core` `crate_walker::tests` (a real `cargo fetch` of a local git repository), `docs/api/traits.md`.
- ✅ **API-50 Observe cells: sampling a level from an interrupt in place** — `ww_codegen!(.., observe_cells =
  "root_foc")` puts a level with `+batched` properties on a ring of atomics instead of `<Level>Sampler::sample`: a
  `const fn` `<Level>Cells<R>` (a `static`, or an `Arc`), one `<Level>Writer` with a setter per property (relaxed
  stores where the value is computed) and `commit(t)` (the level's gate and grid, release fences, no lock), its reader
  in `ObserveBatches::new` / `init`; `poll_properties` copies the rows into one staging batch and sends them as the sampler's
  (same wire; a row the writer laps is `missed`, never sent mixed). Opt-in per level, the default stays. Built on
  `shrink_wrap::series::SeriesCells` (`series(cells)` derive, `ww_si` and numbers have it). On an STM32H725 a row of
  the motor demo's 21 FOC values takes 354 cycles at `opt-level = "z"` against 412 through `sample()` (328 against
  419 at 3), 44-52 B of stack against 108-116 B, the same RAM. `ww api scaffold --observe-cells`. Writer and reader
  on two threads tested under miri (`just miri`). (`docs/api/observing.md#cells-sampling-from-an-interrupt`,
  `tests/observe_batched`, `examples_mcu` `foc_observe`.)
- ✅ **API-52 A cheaper observe poll** — `poll_properties` of a level with `+batched` properties spends its cycles
  on the codec, not on bookkeeping, with the same bytes: a batch whose worst case doesn't fit the outbox's free room
  is written into it straight away and measured only when it didn't fit, a measure counts each column from the
  store (`shrink_wrap::series::count_store_series`) instead of rebuilding it into the scratch, the outbox keeps its
  batch count and offsets (`LevelBatch::column_after`), and a cells level copies only the time and the observed
  columns out of its ring (a column observed mid-batch is copied for the staged rows afterwards, seqlock-checked).
  On an STM32H725 the poll that encodes a 32-row batch of the 21 FOC properties takes 100 k cycles instead of
  191 k at `opt-level = 3` and 169 k instead of 513 k at `"z"` (cells 107 k / 180 k), so all 21 at 25 kHz are 17 %
  of the core with the rows instead of 30 % (27 % instead of 77 % at `"z"`); a cells drain of 3 of 21 observed
  columns 4.4 k instead of 9.2 k. The high rates moved little (3 values at 400 kHz 74 % of the H725 instead of
  78 %, 1 MHz 181 %, 2 values at 2 MHz 313 %): their polls are the codec and each column's framing now
  (`docs/articles/series.md#high-rates-a-few-values-at-400-khz-to-2-mhz`, `examples_mcu` `foc_observe`,
  `high_rate`, `ww_stream` `cells_tests`).
- ✅ **API-21 `no_alloc = false` servers** — servers generated for `std` whose handlers take and give owned types
  (`String`, `Vec<T>`, `FooOwned`): method arguments and returns, property values and write errors, sink items,
  stream function results, stream items, deferred returns; written into the reply buffer with the bytes a `no_alloc`
  server sends (shrink_wrap `BufWriter::ser_owned` / `write_owned`, `OwnedSer`). The `methods`, `properties` and
  `streams` suites run against such servers with a `no_alloc` one as the byte reference (`tests_common::Twin`);
  `ww api scaffold --alloc` prints the owned signatures. Prerequisite of the hub API (SRV-28)
  (`docs/transport/hub.md#servers-on-std-no_alloc-false`).
- 📋 **API-17 Methods arguments to a user struct** · `v0.5.x`.
- 🚧 **API-18 Size analysis** · `v0.5.x` — maximum buffer usage in the worst case, auto-calculated buffer sizes for events,
  arguments and outputs. Done: `MAX_SIZE` of types (SW-19) and per-method `<METHOD>_REQUEST_MAX_LEN` /
  `<METHOD>_RESPONSE_MAX_LEN` consts in `no_alloc` servers, checked at compile time against a buffer in
  `tests/bounded` and `examples_mcu/mcu_qemu`. Missing: consts for properties, streams and sinks, the largest of
  all of them for a whole API (to size `ww_device` buffers such as `UdpBuffers<MAX_MESSAGE_LEN>` automatically),
  paths of nested traits counted exactly (now the worst case of the full path), the same consts on the client.
- 📋 **API-19 Change dispatcher match order** to process common requests faster · `v0.5.x`.
- 🚧 **API-20 Trait paths on the server side** · `v0.5` — servers serve `PathKind::GlobalFull` requests (crate,
  version and trait name) and `PathKind::GlobalCompact` ones (the trait's `ww_global` id) wherever the trait sits in
  the tree, `ErrorKind::TraitNotFound` otherwise (`tests/cross_version`, `docs/api/addressing.md`). Every public
  `ww_stdlib` trait has an id (`ww_global/wire_weaver_gid.json`, looked up by crate and trait name, guarded by
  `tests/global_ids` and `assigned_ids.lock`), the API model carries it (`ApiLevel::compact_id`), the client sends
  compact paths and falls back to `GlobalFull` for devices built before an id existed, the tap decoder and
  `ww explain` name compact calls; 64 B of flash on a Cortex-M3 (`examples_mcu/mcu_qemu` `qemu-global-paths`).
  Missing: traits under a resource array.
- ⛔ **API-22 Implement the same trait several times → enum selector** · `v0.5.x` — stalled. At least print a proper error
  when a trait is implemented twice (`tests/traits_api`).
- ⛔ **API-23 Index chain array → named arguments** · `v0.5.x` — stalled. Generating ad-hoc structs with named index
  fields might be a good ergonomic: users see the names through IDE lookup instead of guessing array indices.
- 📋 **API-29 Bounds of resource array indices** · `v0.5.x` — `stream!(adc[8]: u16)` or `motor[1..=4]`, checked by the
  server before a handler is called and visible in introspection (`docs/api/arrays.md`). Lengths of lists and
  strings inside a resource's type are bounded with `MaxLen` (SW-19). With a bound, observed properties (SRV-18) can
  get a table per array element: until then a trait level in a resource array can't be observed
  (STD-31 `ww_gpio` pin level waits for it).
- 🚧 **API-30 Counted and reliable streams** · `v0.5` — counted streams put a head in every event (flags and a
  length-coded 64-bit `pos` in units the device names when it sends, the event's `units`, `lost`, optional device time
  `t`), so drops on the device, link losses and duplicates are reported exactly and item types carry no `seq` /
  `lost` / `t` fields (IO family D4, D5). Reliable streams add `Ack` / `Resend` / `Query` (go-back-N, grants in
  bytes), keep unacknowledged events in a store, resume by cursor after a reconnect with the latest values first and
  the backlog beside them; a send scheduler with priorities between streams; typed `cnt` counters
  (`docs/api/stream_reliability.md`). Done, the protocol core: wire types in `ww_client_server::stream`
  (`StreamHead` with its hand-laid encoding, byte-verified; `StreamCursor`, `StreamOpened`, `EventKind::StreamItem`,
  `RequestKind::StreamOpen`, the sideband), the `ww_stream` crate (`Counted`, `Reliable`, `Receiver`, `RamRing`,
  `Scheduler`, `StreamCnt`; sans-IO, `no_std`, tested over a seeded lossy link with random disconnects),
  `ww_self::ApiItemKind::StreamFn`, codegen (API-31), the host runtime (CLT-23). Missing: credit sinks on it
  (API-34). The persistent store is done (SRV-15).
- 🧪 **API-34 Credit sinks on the stream protocol** · `v0.5` — `Ack` / `Query` / `Resend` shared with reliable
  streams, sessions and resume for credit sinks, so a sink (UART tx, a DFU plan) continues after a reconnect.
  Positions count units the host names per item (1 by default, the payload's bytes for a byte sink), grants count
  bytes; positions stay as API-42 has them (`u64` in state, modulo 2^28 on the wire), the host keeps at most
  `MAX_WINDOW` bytes and units in flight; streams keep their full `UVlq64` positions (their gaps are unbounded). The
  host names the session in `StreamOpen`'s cursor, so a repeated or late duplicate open (UDP) no longer resets a
  running session. Done: the protocol core, `ww_stream::sink` (`SinkCredit` on the device, `SinkTx` on the host),
  `RequestKind::SinkWrite` / `SinkWriteUnits`, and the switch-over: generated server code (open by `StreamOpen`, a
  read returns `StreamStatus`), the host `Sink` (`send_units`, `open_session`, a device that lost the session is
  reported), dynamic client and Python, `ww_dfu`, every test crate; the old `Credit` / `CreditQuery` / `Resend`,
  `CreditWrite` and `ww_client_server::credit` are gone, `ResendFrom` is `Resend`. Resume in the host API:
  `Sink::cursor()`, sinks in `disconnect().keep_streams()` / `Commander::resume_streams()` (the session goes on from
  where the device is, nothing repeated or lost), the generated `<sink>_resume(cursor)` after a host restart,
  `StreamError::SinkSessionLost` for a device that lost the session; Python `Sink.cursor` / `resume` /
  `ww.SessionLost`. Tests: `wire_weaver_client` `event_loop/sink_e2e_tests.rs` (reconnect in the middle of a
  transfer with lost writes, a rebooted device, a restarted host, a late duplicate open), `tests/credit_sink`,
  the 2C-APP-BULK DFU scenario. See `docs/api/credit_sinks.md#resume`. To check on hardware: TEST-26.
- 💡 **API-53 Sink functions** — `#[sink(T, credit)] fn uart_tx(config: TxConfig) -> Result<A, E>`, the
  counterpart of stream functions (API-31): a sink opened with a configuration and a typed result, a new session
  exactly when the configuration changes. The wire has it (`StreamOpen { args }`); codegen and the host API are
  missing.
- 💡 **API-24 Guarded calls for critical resources** · `v0.5.x` — for safety-critical actions, such as starting a motor
  or switching high power, a request that happens to match predefined bytes must not be enough. Add an extra
  check, e.g. a CRC over the resource identity and arguments (a "strict mode" per resource), or a
  request/response challenge before the call is executed.
- 🚧 **API-26 Generate C** · `v0.5.x`, and an **FFI example**. Generated C comes from the `ww_self` model, not from
  the derive's AST: a generator run after the build, from an introspection bundle (e.g. `ww gen c`), writes `.h` /
  `.c` for an API's types on top of the C runtime (SW-43 c-runtime), later a device-side dispatcher, introspection
  as a const blob and `ww_framer`. None of that is there yet. The C header text `fixed_layout` (SW-44, SW-45) and
  `head_tail` (SW-46) print from the derive today moves to that generator over time. Other languages are generated
  the same way, from the model. The conformance matrix (TEST-23) is what keeps the generated code byte-equal to the
  derive.
- 💡 **API-27 Standalone definition files with custom syntax** — APIs and types in their own files, with a friendlier
  syntax: nested levels, no macro calls, SI units. Tried twice already, approach with care:
  - at the very beginning of this repo, a YAML based language;
  - later, a big effort to support a custom syntax, which turned into a project of its own and took a lot of
    resources away from the actual work.

  Plain Rust with `#[ww_trait]` and `#[derive_shrink_wrap]` is the current answer; revisit only with a concrete
  benefit that outweighs the cost of a parser, tooling and IDE support.
- 💡 **API-40 `value_on_changed` strings without an allocator** — a `no_alloc` server's field for a
  `MaxLen<&'i str, N>` property is `&'static str` today, which a write can't fill; generate an
  `shrink_wrap::ArrayString<N>` field (SW-57) instead, and `ArrayVec<T, N>` for lists, so settings like a device label
  can be `value_on_changed` too (STD-32).
- 💡 **API-51 Stream codecs across items** — any stream or observed property could send each item as a diff against
  the one before, generic over `T`, from the derive per leaf: integers as delta or delta of delta, floats XOR coded,
  enums, units, strings and `bool` as one "same" bit or the new value, a whole item "unchanged" bit; byte items
  (UART or CAN frames) XOR'd with the previous one or through a small no_std LZ window (LZ4 across a
  counted stream's events is API-55). Declared in the API
  (`#[stream(T, codec = auto)]`), in the introspection data so dynamic clients and Python decode it; keyframes on
  open, after a gap and every N items. It is how a string or a frame, which a `Series` batch can't take (no size
  bound), would get cheaper per item. Prior art: Gorilla, Sprintz, Chimp, ALP.
- 📋 **API-58 `#[read_only]` methods** · `v0.5.x` — a typed attribute in `ww_self::Meta` (next to SRV-34's `claim`)
  for methods without effects, carried in introspection: the hub lets peers with only the `observe` right call them
  (SRV-37), GUIs may call them freely. The API author's promise, not something the hub can check. Design: hub
  federation (SRV-33).
- ✅ **SRV-1 `no_std`, no-alloc server**, IO-free generated code.
- ✅ **SRV-2 Sans-IO `DeviceLink`**, async `Server` with `wait()`/`handle()` for a user-owned event loop, and
  `blocking::Server`.
- ✅ **SRV-3 Fully split TX and RX flows** for every transport (fixed the half-duplex interlock), framing without IO.
- ✅ **SRV-4 User handlers called straight from the RX loop**, transport glue extracted into `ww_device`.
- ✅ **SRV-5 Device descriptors**: serial, WireWeaver version, API id, version, hash and user label in the USB interface
  string.
- ✅ **SRV-6 Media**: USB (embassy), RTT, WebSocket and UDP (both also on `embassy-net`).
- ✅ **SRV-7 UDP device medium** — `ww_device::udp`, one frame per datagram over any datagram socket, one host at a
  time, another host takes over by starting link setup (`docs/transport/udp.md`).
- ✅ **SRV-15 Stream stores and backfill** — `ww_stream::store`: what a reliable stream (API-30) keeps until the
  host acknowledges it: `RamRing` (API-30) and `LogStore`, a power-cut safe log on a block device (feature
  `log-store`; `SdMedium` on an `embedded-sdmmc` `BlockDevice` with `sdmmc`, `NorMedium` on an
  `embedded-storage-async` `NorFlash` with `nor-flash`) that keeps session, positions and clock epochs across reboots;
  drop-newest or drop-oldest when full, counted exactly. Tested with a power cut at every write
  (`ww_stream/src/tests/log_store.rs`); `docs/api/stream_stores.md`.
- ✅ **SRV-16 Persistent RAM store** — `ww_stream::store::PersistentRamRing` (feature `persistent-ram`): a reliable
  stream's store in `.uninit` RAM that survives a soft reset (fault → reset), for the logs before a crash (STD-26).
  Header slots and records checked by CRC at boot, so garbage after a power-on is never taken for data; continues
  session and positions like `LogStore`, a new clock epoch per boot. Tested with a fault after every byte written
  (`ww_stream/src/tests/persistent_ram.rs`); `docs/api/stream_stores.md`.
- ✅ **SRV-18 Observed properties on the device** — servers of APIs with `+observe` properties answer `Observe`
  requests (API-38) and send `PropertyUpdate`s from the generated `poll_properties`: `ObserveState` with one table
  per trait level sized at compile time, a sparse `<Trait>Update` per level (DER-13), `mark_changed_<name>()` (called
  for `value_on_changed` writes), periodic properties sampled through their getter. A token bucket per level at the
  granted budget, earliest deadline first, at most one update per level per call, the rest slips and counts as
  missed; properties have a reserved share of the link (`ObserveLimits::total_budget`) next to stored streams.
  `Observe` is untrusted input: validated in full before anything changes, clamped, bounded work, refused per
  entry. Levels under a resource array answer `OperationNotSupported` until their bound is known (API-29), `+observe`
  properties in an array or `#[optional]` aren't observable yet. `ww_stream::observe`,
  `docs/api/observing.md#the-device-side`; tests `tests/observe` (fake clock, real host event loop),
  `ww_stream/src/observe/tests.rs`.
- ✅ **SRV-20 deferred-nested-levels Deferred return helpers on nested levels** — `<prefix>_<method>_send_return`,
  `_send_return_blocking` and `_ser_return_event` are generated for deferred methods on nested trait levels too, named
  like the handler (`plug_vbus_cycle_send_return`); `method_model` patterns match the prefixed name. Test
  `tests/methods` (`deferred_on_nested_level`), `docs/api/methods.md#deferred`.
- ✅ **SRV-21 handler-model Sync handlers in an async server** — `ww_codegen!(.., handler_model =
  "device.settings.*|meter.reading=async, _=sync")`: per item (regex over its dotted path, first match, `_` the
  default, `use_async` without one) its handlers are `async fn` or plain `fn`; the dispatcher and `poll_properties`
  call sync ones without `.await`, a level whose items are all sync gets a plain `fn` dispatcher. A handler of the
  other kind is a compile error (`wire_weaver::handler` checks), so is a pattern that matches nothing.
  `ww api scaffold --handler-model`. Donguru: -20.2 KiB flash at `z`, -12.6 KiB at 3. `docs/api/handlers.md`; test
  `tests/observe` (a mixed server).
- ✅ **SRV-23 observe-table-unsized One observe table code for every level** — `ww_stream::observe::ObserveTable<S: ?Sized = [Slot]>`: a level stores `ObserveTable<[Slot; N]>`, the methods (`apply`, `plan`, `commit`, ...) are compiled once, on `ObserveTable<[Slot]>`; `LevelDesc` is slices, the plan and the selection keep their per-slot state in caller storage (`Due`, `Mark`), and levels without observable properties answer `Observe` through one shared path (`refuse_all`). Donguru: −13.3 KiB flash at `opt-level = 3`, −9.4 KiB at `z`. `ww_stream/src/observe.rs`, `docs/api/observing.md#the-device-side`.
- ✅ **SRV-25 event-driven-observe No fixed observe tick** — a device sleeps until something observed is due
  instead of polling every millisecond. `ObserveState::next_wake(now_us)` (`ObserveTable::next_wake` per level: the
  periodic grid, `last + min` of a changed property, `last + max` of a beat, a held level's bucket refill, a batch
  sealed or as old as allowed; `None` when nothing comes due without a change); a generated `ObserveSignal` any task
  or interrupt marks a property changed through (`mark_changed_<name>()`) or wakes the server task with, no lost
  wake between a change and the sleep (`ww_stream::wake`); `with_wake` on samplers and cells writers;
  `ww_device::Server::wait_until(at)`; `ww api scaffold` prints the signal. Tests: `ww_stream` (polling only at
  `next_wake` sends what polling every µs does), `tests/observe` (fake clock: polls only on the grid, a change from
  another thread sent at once), `tests/observe_batched`. `docs/api/observing.md#the-server-loop`.
- ✅ **SRV-24 shared-observe-loop One observe loop for every level** — the algorithm of `poll_properties` (plan, overhead, selection, measuring, writing, commit, missed counts, batches of samples) is `ww_stream::observe`'s, sync: `plan_level`, `select`, `write`, `ser_update`. The generated `poll_properties` is one loop over the levels with one send site; per property it gives only a `size` and a `write` arm (through `&mut dyn FnMut`) and awaits its async getters (SRV-21) itself, reading an unbounded async value once. `ObserveState::level` / `oldest_batch` / `remove_oldest_batch` reach a level by its number. Donguru, on top of SRV-21 and SRV-23: −5.8 KiB flash at `opt-level = 3`, −5.0 KiB at `z`. `ww_stream/src/observe.rs`, `docs/api/observing.md#the-device-side`; tests `tests/observe`, `tests/observe_batched`, `ww_stream/src/observe/tests.rs`. A value that can't be had at write time is dropped alone (counted missed), the rest of the update goes; tested on Donguru B153A (`plug` with `cc` unimplemented).
- ✅ **SRV-22 error-seq-per-item Error sites numbered the same in every server** — `err_seq` of an `Error` is no
  longer one counter across the generated server (705 numbers on Donguru) but fixed per site
  (`ww_client_server::err_seq`): item steps in step order and the same for every item (0 kind not taken, 1 decode,
  2 handler, 3 encode, ..), level sites (40..), request sites (48..), deferred return (32..). With the build, the
  request path and the error kind it still pinpoints one site, and identical error arms are shared code: Donguru
  -1.5 KiB flash at `z`, -1.7 KiB at 3. `Error::site()`, the host prints `Unimplemented at item site 2 (handler)`.
  `docs/api/errors.md#where-on-the-device-an-error-came-from`; tests `tests/methods`, `tests/traits`.
- ✅ **SRV-26 Device-side engine for I2C and SPI slaves** — `ww_bus_slave`, no_std, sans-IO, no alloc: `I2cSlave` and
  `SpiSlave` do what STD-35's API promises, so a firmware only wires its peripheral's interrupts to them. I2C: the
  register map (pointer bytes, auto-increment, wrap at `len`, page wrap, `read_only`), host updates staged until the
  stop, the answer queue with `Fill`, `Stretch { max }` (deadline, `resume`, `ReadWaiting`) and `Nack`
  (`acks_address`), `unsent` for a TX FIFO loaded ahead. SPI: queue answers (`idle`, `repeat_last`, `underrun`) and
  `SpiRegisters` frames (address, `rw` bit, dummy bytes before read data), the answer picked at the first `miso()`
  (FIFO preload, `rewind`); 8-bit words only. Events built as `I2cSlaveEvent` / `SpiSlaveFrame`. `const fn new` for a
  `static`; 58-100 instructions per byte on a Cortex-M3 (`examples_mcu/mcu_qemu` bin `qemu-bus-slave`).
  `docs/api/bus_slave.md`, tests `ww_bus_slave/tests`. First firmware user: usb_io_fw IF-5 (rePort, RP2350B).
- ✅ **SRV-27 ww-lz4-encoder Streaming LZ4 encoder for byte streams** — `ww_lz4`, no_std, no alloc: each chunk of a
  stream (UART RX, a log) becomes one standard LZ4 block whose matches reach back into earlier chunks, so 512 B chunks
  of shell text go 3.2-5.1x and 64 B chunks 2.4-3.3x (captured boot log, `ls`/`rg` colour output, `top`), about 2x
  without history. `Encoder<HASH, BUF>` with a `const fn new(keep)` for a `static`: a hash table of `u16` positions
  (2 B per entry), `input(want)` gives the free tail of the history buffer (slides first, `keep` bytes survive) so a
  chunk is written in place, `compress(len, out)` writes the block, `reset()` starts the history over, `HISTORY`,
  `max_block_len(n)`. A chunk that doesn't shrink goes out as a literal-only block, and after 1 KiB of those the
  encoder only retries every 4 KiB (`with_backoff`). Feature `ram` puts the hot code (1.3 KiB) in `.data.ww_lz4`.
  Host side `Decoder` (feature `std`, lz4_flex with the last 64 KiB as dictionary). Same ratios as the lz4s bench of
  P2534 (within 0.1 %); tests `ww_lz4/tests` (round trips against lz4_flex, random chunking, untrusted blocks) and
  `just test-lz4-c` (C liblz4 decodes the blocks). Device and host user: API-55.
- ✅ **SRV-34 per-resource-claims Per-resource claims through the hub** · `v0.5.x` — typed metadata attributes,
    carried in introspection as data (`ww_self::Meta` on `ApiItem` and `ApiLevel`), checked when the API is loaded:
    `#[claim(exclusive | shared | observe)]` on a `ww_impl!` or trait definition (the root is always claimable),
    `#[shares(pins = "PA9, PA10", peripherals = "USART1", index = N)]` (pin ranges `PA0-PA15`) so overlapping
    resources conflict, `#[label("CAN1, DB-9 on the front")]` (one per array element) on any item;
    `ApiItemOwned::claim_mode` / `label`, `ww api tree`, rustdoc notes, `ww api check` reports changes as compatible;
    stdlib IO traits claimable (`ww_can_bus`, `ww_uart`, `ww_i2c`, `ww_spi`, `ww_gpio::Bank`, `ww_dfu`,
    `ww_pin_diag`). The hub's `claims` group (`claim(key, path, purpose, ttl_s) -> Result<u32, ClaimError {
    UnknownDevice, ApiUnknown, BadPath, NotClaimable, InUse, Overlap }>`, `release`, `claims(key)`, `Claimed` /
    `Released` events): calls and writes of other client instances at or under a claimed path are answered by the hub
    with `Common(InUse { by })` in the item's own error type (an error event naming the holder without one), without
    reaching the device; reads, streams and observe stay open (taken too for `observe`); a claim belongs to the
    client instance (CLT-41) as its principal and drops with its last connection or at the ttl; overlap by path and by
    `#[shares]`, nesting within one instance. `docs/api/metadata.md`, `docs/transport/hub.md` (with how hub and
    device-side claims combine); tests `tests/traits` (`meta_in_introspection`), `wire_weaver_core` (`meta_*`),
    `ww_hub` (`claims`), `tests/hub` (`claim*`). Later attributes: API-56, API-57.
- 🧪 **SRV-19 ECC-safe STM32 flash for stores** — `mcu/ww_ecc_flash`: `EccSafeFlash` wraps any `NorFlash` (embassy-stm32's
  `Flash`) and reads it one ECC word at a time; a word with an uncorrectable ECC error (power cut while programming
  or erasing) reads as zeros instead of an NMI (STM32G0, `FLASH_ECCR.ECCD`, through `on_nmi()` or feature
  `nmi-handler`) or a bus error (STM32H7, `DBECCERR`, ignored under FAULTMASK + `BFHFNMIGN`), so sequential-storage
  skips it. Never `MultiwriteNorFlash`. Blocking and async. `docs/api/ecc_flash.md`. Strict mode for bootloaders
  and image checks: `EccSafeFlash::new_strict` → `EccStrictFlash`, whose read fails with `Error::Ecc { offset }`
  (`is_ecc()`) instead of zeros; chunky's feature `ww-ecc-flash` implements `chunky::ecc::EccError` for it (chunky
  ECC-2), for the B135 bootloader and app. Both modes host-tested on a simulated flash (`just test-ecc-flash`).
  On hardware: a G0B1 (B153A) store survives 3000 resets in the middle of its programs and erases (TEST-21), but a
  reset never tears a G0 flash word, so the zeros path itself still waits for a real power cut (TEST-21); H7 is
  built, and `examples_mcu/ecc_flash_h7` makes real ECC errors by programming a scratch flash word twice to check
  both modes (not run on a board yet).
- ⛔ **SRV-8 One crate per role for firmware and host** · `v0.5` — blocked on generated code using absolute paths.
  Agreed direction, instead of putting everything into `wire_weaver` behind features or splitting by
  std/no_std:
  - `wire_weaver` stays lean (`shrink_wrap`, macros, `ww_version`, result types) since every API crate and
    `ww_stdlib` crate depends on it;
  - `ww_device` re-exports `wire_weaver`, `ww_framer`, `ww_link` and gets features `usb-embassy`, `rtt`, `udp`,
    `ws`, `defmt`, `embassy-time`, `std`;
  - `wire_weaver_client` re-exports `wire_weaver` with transport features.

  Requires: codegen emitting paths through the role crate (or a `crate = ".."` argument / `proc-macro-crate`;
  `WIRE_WEAVER_REEXPORTS` in the crate walker is partly there), and `wire_weaver_usb_embassy` building in the
  root workspace or merging into `ww_device`. Separately consider making `wire_weaver`'s `std` default off.

- 📋 **SRV-9 Handle USB suspend/resume** · `v0.5` — run until suspend, wait for resume, loop
  (`mcu/wire_weaver_usb_embassy`). Later (`v0.5.x`): detect the host going to sleep and keep the connection,
  informing the device.
- 📋 **SRV-10 Collect error codes into an ELF section** for easier error decoding · `v0.5.x`.
- 📋 **SRV-11 Link statistics** in `ww_device` (`TODO` in `link.rs`).
- 💡 **SRV-12 std server on Tower** · `v0.5`.
- 📋 **SRV-13 ww hub: one device, many programs** · `v0.5.x` — a per-user hub that starts itself on first use and
  exits when idle (sccache / ssh ControlMaster style, no system service, Linux, macOS and Windows), holds the
  devices (USB, RTT, later UDP) and serves every client over a user-only local socket (TR-16); it is itself a
  wire_weaver device with its own API (`ww_hub_api`). Today a second program gets "busy". Pieces: SRV-28 process and
  API, SRV-29 device multiplexing, SRV-30 version skew, SRV-31 clock sync, SRV-32 exclusive locks, SRV-33 federation (SRV-36..40),
  SRV-34 per-resource claims, TR-16, CLT-38..41, CLT-43, CLT-44, TOOL-17, TOOL-18; prerequisite API-21 (done). Later API groups: logs to several viewers, shared
  captures (TR-14's `ClientMap`), labels and settings, fault notifications, published (virtual) devices.
- 🧪 **SRV-28 Hub process and its API** · `v0.5.x` — crates `ww_hub_api` (`Hub`: `info`, `handover`, `shutdown`,
  `keep`; `Registry`: `devices`, `clients`, `take`, `events`) and `ww_hub` (`docs/transport/hub.md`): one hub per
  user (`hub.lock`), the socket with the preamble picking the hub API or a device, the API served with generated
  `no_alloc = false` code on `ww_device::Server` per connection, devices enumerated without opening plus the
  CLT-38 holders, plug / unplug / holder / client events, idle exit, `keep`, `shutdown`; `ww hub serve`,
  `ww_hub::main_if_hub()`, the typed `ww_hub::client::HubClient`. Starting it detached is CLT-39's, `handover`
  SRV-30's; `take` came with SRV-29. Tested on Linux; check open on macOS and
  Windows (compiles for both).
- 🧪 **SRV-29 Hub device multiplexing** · `v0.5.x` — device tunnels in `ww_hub`
  (`docs/transport/hub.md#device-tunnels-srv-29`, `ClientConfig::hub_device`): one client event loop per open device
  (`Command::Forward` / `ForwardSink`), a `ww_device::DeviceLink` per client tunnel answering like the device, seq
  remap, stream opens counted per path and fanned out, observe settings merged per level, introspection from cache,
  bounded per-client queues (stream data dropped and counted, replies never), devices opened on demand and closed after
  a linger (30 s), lock file mode `hub`, `registry().take`. The hub reads only seq, path and kind, never a user API. E2E
  in `tests/hub`; measured against direct USB on three boards (`tests/hub/examples/hub_loopback.rs`: throughput as
  direct, 16 B round trips over USB FS 10-25 % fewer on an idle PC, 3-13 % under load, from the two extra process
  wakes). Tested on Linux; check on macOS and Windows open. Limits: sinks and the sideband of a shared stream are the
  first client's, streams on trait paths are not counted per client.
- 🧪 **SRV-42 Device flushes by priority, also deferred replies** · `v0.5.x` — the device answers by the request's
  priority (CLT-50): `Context::priority()` and `ReplyTo::priority` carry it to handlers, a reply to an urgent
  request is flushed at once from `Server::handle`, and a deferred one (generated `<method>_send_return(out,
  reply_to, ..)`, sent through `Server::sink()` from outside `handle`) through `MessageSink::send_reply`;
  default-priority replies, immediate or deferred, wait for the accumulation window like everything else.
  `Sink::flush` (async and blocking) lets firmware send what waits now. Before, a deferred reply always waited for
  the window and firmware had no way to flush it. Tests: `ww_device` `async_server::replies_flushed_by_priority`,
  `wire_weaver_client` `flush_e2e_tests`. On hardware: part of CLT-50's re-measure.
- 💡 **SRV-41 Fewer wakes on the hub path** — the tunnel's cost is two extra process wakes per message (client →
  hub → client): 16 B round trips over USB FS are 15 % (donguru) and 23 % (b200) fewer than direct on an idle PC
  (median of 7 rounds), 3.5 % and 12.6 % under load; the hub's own USB round trip equals direct (480 µs against
  510-600 µs), throughput and max-size transfers are as direct. Ideas if it matters: busy-poll the socket for a short
  window after a request (spends a core), or a shared-memory ring between client and hub with a futex / event for
  the wake. Kept as is for now (person, 22 Oct 2026); measure with `tests/hub/examples/hub_loopback.rs` first.
- 🧪 **SRV-30 Hub version skew and handover** · `v0.5.x` — a hub serves every client whose link and API model
  versions its build can serve; a client that can start a newer hub asks the running one to hand over (only strictly
  newer replaces older, so it converges); a refusal names both versions and the fix. Built
  (`docs/transport/hub.md#version-skew-and-handover-srv-30`, `docs/evolution/rules.md#the-ww-hub-and-version-skew`):
  preamble version 2 with a skippable trailer carrying the client's `ww_link` and `ww_client_server` versions (version
  1 still read: versions unknown; a client asks a v1-only hub again in v1), the hub's window (`ww_hub/src/window.rs`:
  SemVer-compatible with its build, plus earlier majors it keeps after a break, none yet) and status 4 with
  `Error::HubRefused` naming both sides and the fix; `Hub::handover(socket)` (strictly newer only, `Event::Handover`,
  links closed with `DisconnectReason::HubHandover`, `Exit::Handover`), `ww_hub::Bound::take_over` (own socket, then
  `hub.lock`, rename into place on Unix, the pipe anew on Windows), and `hub::ensure_running` replacing an older
  running hub with the program's own (`main_if_hub`, `WW_HUB_HANDOVER`, under `hub.start.lock`). Tested on Linux in
  `tests/hub_reconnect` (`versions.rs`, `handover.rs` with clients connected, `replace.rs` across processes); check
  on macOS and Windows open (compiles for both).
- ✅ **SRV-31 Clock sync in the hub** · `v0.5.x` — the hub steers every open device serving `ww_timebase::Clock` with
  `host_sync` (found in its introspection data at any path) onto the PC's UTC with `clock_sync::ClockSync` on its own
  connection, and keeps it open for a sync linger (30 min, `WW_HUB_SYNC_LINGER`, `forever`) so boards stay steered
  between short-lived commands; `WW_HUB_SYNC=off` / `sync().set_enabled` per device or all. `ww_hub_api::ClockSync` at
  `sync`: `devices` (`SyncEntry`: phase, state, source, the steerer the device follows and one it only notes, rate,
  shortest RTT, residual via `shared_now`, error, slew, samples), `linger`, the `stats` stream. Rate cache per device
  serial in the state dir (`clock_sync::RateCache`, atomic writes, shared with direct-mode programs): a known device
  is steered after its first reading. Steer points say who steers (`ww_timebase::Steerer`, STD-49 follow-up); the device
  follows one and notes the others (embedded_bedrock `bedrock_sync`). `ww sync` shows the hub's steering with a hub,
  steers directly without one, STEERED BY column. `docs/transport/hub.md#clock-sync-in-the-hub-srv-31`,
  `docs/std_library/ww_timebase.md#one-steerer-at-a-time`; E2E in `tests/hub` (`clock_sync`), `tests/io_timebase`.
  Paused (`SyncPhase::Paused`, readings go on, no steer points) while a program holds the whole device (SRV-32);
  programs holding a device directly steer on their own connection by default (CLT-46).
  Checked on omarchy-m1 (22 Oct 2026, Donguru and B135 through the hub, `ww sync` as its view): `Locked` 1.4-2.6 s
  after the hub opened them with no cached rate, 0.4 s with one, also after a hub restart and after 60 s closed in
  holdover; a loop of short `ww info` calls goes through the hub from its third call and the board stays steered
  without clients for the sync linger, then closes; residual against UTC B135 -5..-16 µs (±55 µs, RTT 120 µs),
  Donguru within ±1.2 ms (RTT 1.4-1.6 ms). Two steerers on one board (follow one, note the other, take over) checked
  on Donguru and B135 by `bedrock_sync`'s side.
- 🧪 **SRV-32 Device-wide exclusive and lending** · `v0.5.x` — exclusive access to a whole device (DFU, settings
  writes) is a claim on its root path (SRV-34): other programs' calls and writes anywhere on it refused naming the
  holder, and `Hub::steering_paused` for the hub's clock steering (SRV-31 skips the device while it holds);
  `wire_weaver_client::hub::Exclusive` takes it when a hub holds the device (`None` otherwise: the program holds it
  directly), and `ww dfu reboot | flash | confirm | revert` do, `flash` across the device's reset. Lending:
  `Claims::lend` (a root claim marked lent) closes the device (tunnels get `Disconnect(Other(DISCONNECT_LENT))`),
  refuses tunnels with `Status::Lent` and others' claims naming it until released, then opens it again if the hub
  held it; `Lent` / `Returned` events; a crashed borrower or a ttl returns it. `ww hub lend DEVICE -- CMD` (Ctrl+C
  reaches the command, its exit code is ww's), `hub::Lend`. `docs/transport/hub.md`; tests `ww_hub` (`claims`),
  `tests/hub` (`exclusive*`, `lend*`). Lending checked on omarchy-m1 (22 Oct 2026): `ww hub lend 204B319B5431 --
  sh -c '...; probe-rs reset --probe <J-Link> ...'` on Donguru while `ww sync` watched it through the hub: the hub
  paused its steering and closed the device 7 ms after the ask, the command ran, the device was back open in the hub
  3 ms after the return (1.4 s for the whole lend), and `ww sync`'s connection came back and the board was `Locked`
  again 1.8 s later. Open: `ww dfu flash` through the hub on hardware, as no free bench board has chunky A/B (Donguru
  and B135 run in place without it, rePort was busy; B135 could be provisioned with vb135's `just
  provision-ab-dfu`, which erases its settings: the person's call); no `ww settings` command writes to a device yet (`dump` reads a file), one that
  does takes `Exclusive` the same way. Clients reconnect after a lend with the `Lent` reason (CLT-40), and the hub's
  steering pauses while the root is held (SRV-31, `tests/hub` `a_root_claim_pauses_steering`).
- 📋 **SRV-33 Hub federation between PCs** · `v0.5.x` — remote devices through another PC's hub, as `HOST/<key>`,
  with who may connect decided by the hub that owns the device (security design reviewed before code). Steps:
  SRV-36 ssh (full access), SRV-37 principals and per-device rights, SRV-38 restricted ssh keys, SRV-39 the browser
  on localhost (TOOL-6), SRV-40 native hub-to-hub with pinned keys (Noise IK). No UDP, no transitive federation.
- 📋 **SRV-36 `ww hub attach` over ssh** · `v0.5.x` — `ww hub attach HOST` logs in once in the terminal (an ssh
  ControlMaster), the local hub then opens one `ssh HOST ww hub stdio` channel per tunnel and lists the remote
  devices as `HOST/<key>`; ssh authenticates, the remote hub serves the channel as its own user. Byte-spliced
  tunnels (the remote hub multiplexes), host names and remote keys validated, `ww hub detach`.
- 📋 **SRV-37 Hub principals and per-device rights** · `v0.5.x` — who is connected, from what the OS, ssh or a key
  says (never the client's own metadata), and what it may do per device: `observe`, `control`, `dfu`, from glob
  rules in `~/.config/ww/hub_peers.toml`; the hub answers refused requests with `Forbidden` itself, filters the
  registry, keeps hub API methods local unless classified, and owns claims per principal.
- 📋 **SRV-38 Restricted ssh keys for the hub** · `v0.5.x` — `authorized_keys` lines with
  `restrict,command="ww hub stdio --peer NAME"` give a key only the rights of `[peer.NAME]`; `ww hub allow --ssh`
  prints the line.
- 📋 **SRV-39 The hub for a browser on localhost** · `v0.5.x` — an HTTP + WebSocket listener on loopback only that
  serves the TOOL-6 web GUI: `Host` and `Origin` checked, a single-use launch token from `ww gui` in the URL
  fragment exchanged for a per-tab session token, the peer's user checked on Linux and Windows.
- 💡 **SRV-40 Native hub-to-hub with pinned keys** — for PCs without ssh: TCP with Noise IK (`snow`, pure Rust),
  a static key per PC (`ww hub key`), peers allowlisted by key and source address with SRV-37 rules. Built when a
  box without ssh needs it.
- 💡 **SRV-14 FPGA server** · `v0.5.x`.
- 📋 **SRV-44 Device serves waiting requests by priority** — requests that arrived in one frame are handled in
  order; take the highest priority (CLT-50) first, so its reply leaves first. `TODO(SRV-44)` in `ww_device`
  `server.rs` and `blocking.rs`.
- 📋 **SRV-45 Hub queues by priority across clients** — the hub (SRV-29) forwards each client's requests in the order
  they came and flushes urgent ones at once (CLT-50); with several busy clients on one device, an urgent request of
  one could still wait behind another's batch in the device link's queue. Sort what waits there by priority
  (with CLT-52).

## Client / host side (`CLT`)

- ✅ **CLT-1 `std` client** with async, blocking and promise flavors, prepared call objects, no `root()` needed.
- ✅ **CLT-2 Generic sans-IO event loop** over a `Transport`, independent TX and RX flows.
- ✅ **CLT-3 USB transport** on nusb: endpoint addresses from descriptors, bulk with lower priority, vec recycling.
- ✅ **CLT-4 RTT transport** over a debug probe, control block address from an ELF file.
- ✅ **CLT-5 WebSocket, UDP and in-process transports**.
- ✅ **CLT-6 `DynClient`** and dynamic resources, for APIs known only at runtime.
- ✅ **CLT-7 Introspection cache** keyed by API hash with and without docs, with a download timeout.
- ✅ **CLT-8 Compatibility check on connect**: each resource is checked against device introspection when the API hash
  differs.
- ✅ **CLT-9 Connection selectors**, clear errors when no device is found, `disconnect()` releases the transport.
- ✅ **CLT-24 One `?` per call** — a method returning `Result<T, E>` gives `Result<T, CallError<E>>`
  (`Device(E)`, `Unimplemented { resource }` or `Client(Error)`) from `call`, `blocking_call` and promises, a property
  with a user error type the same from `write` and `write_promise`, instead of `Result<Result<T, E>, Error>` and `??`;
  `Error::is_unimplemented()` for items without an error type (`docs/api/methods.md#client-side`,
  `tests/methods`).
- ✅ **CLT-25 `ww info` and `ww report`** · `v0.5` — `ww info` shows firmware, board, ids, label and last reset of
  any device by global trait path, whatever its API version; `ww report` adds the counters, the stored `defmt` log
  and the crash's addresses named with the ELF, found by build id, as one file a tester sends; `ww elf add
  <elf|archive>` fills the ELF directory from an ELF or a release's `<firmware>-<version>-elf.tar.zst` (shared with
  `cnt elf add`, `cnt_core::elf`). Library crate `ww_report` (`Contract`, `collect_info`, `collect_report`),
  `docs/std_library/device_contract.md`, `tests/device_report`. The stored log is a stream and streams open on the
  absolute path only (CLT-23), so `ww report` reads the device's introspection data for the log's path; `ww info`
  needs none. Not yet: fetching the release archive by itself.
- 🐛 **CLT-30 Link setup gives up too early on slow RTT probes** — the client sends `GetDeviceInfo` 5 times 50 ms
  apart (`LINK_SETUP_RETRIES` / `LINK_SETUP_RETRY_INTERVAL` in `event_loop/core/tx.rs`), 250 ms for every transport.
  Through a J-Link on omarchy-m1 (aarch64, probe-rs, each RTT write ~17 ms) most connects fail with "LinkSetup was
  not received from device after several retries" while the device is fine; with 40 retries they mostly work. Needs
  a per-transport budget (RTT: seconds) or one taken from the connect timeout. Found 16 Oct 2026.
- ✅ **CLT-28 Observing properties on the host** · `v0.5.x` — generated std clients have `observe(cfg)` per trait
  level with `+observe` properties (an `Observer` of the level's `<Trait>Update`) and `observe_<name>(mode)` per
  property, fanned out from the level: one device subscription per level covering all its observers (merged modes,
  biggest budget), sent again when an observer comes, changes (`configure`) or goes, `Off` after the last one;
  `<Trait>Observe` typed settings and what was granted, `<Trait>Latest` latest-value cache with receive times;
  subscriptions sent again by `resume_streams()` after a reconnect; `Error::Unsupported` for a device whose API model
  predates API-38 (`wire_weaver_client::observe`, `docs/api/observing.md#the-host-side`, `tests/observe`,
  `observe_e2e_tests`). Not yet: levels under a resource array.
- ✅ **CLT-29 `Series<T>` in introspection and dynamic clients** · `v0.5.x` — `ww_self::Type::Series` (and
  `Field::series_opaque` for `#[series(opaque)]` fields) in the API model, emitted by codegen; a dynamic decoder in
  `ww_self` reads a `Series` column by column from the type tree alone (`ValueOwned::des_shrink_wrap_dyn` gives the
  items, `des_series_columns_dyn` the columns), with the typed decoder's rules (restarts after enum and `Option` tags,
  evolvable structs both ways, declared defaults) and the decode budget bounding work and memory (a constant column
  costs no bytes); `ww` shows the items, Python decodes the items or numpy columns (`decode_columns`, masked where an
  item is in another variant or `None`). Dyn equals typed on the design's examples, on generated and on fuzzed series
  (fuzz target `series-dynamic`, `tests/series`, `wire_weaver_py/tests/test_offline.py`,
  `docs/serdes/series.md#introspection-and-dynamic-clients`). Writing too: `ValueOwned::ser_shrink_wrap_dyn` (and
  `ser_series_dyn` with a `CodecPolicy`) writes a `Series` from its items, the typed writer's bytes for the same
  policy (codec choice, restarts, struct heads, generics), so a dynamic client and Python write `Series` arguments
  and properties.
- ✅ **CLT-31 Quantities in dynamic clients, `ww` and Python** · `v0.5.x` — a `ww_si::Quantity<U>` (ww_si 0.2 and
  later) is recognized from introspection data alone: a concrete definition records the generic one and arguments it
  was made from (`ww_self::ItemStruct::instance_of`, host-derived, rejected from devices, left out of signatures), the
  unit comes from `ww_si::units::by_name`, the `Per` / `Times` tree, or a marker of the API's own crate by name and
  docs. `wire_weaver_client::quantity` (`DynQuantity`: `3300 mV`, `{:#}` `3.3 V`, `to_si_f64`, `to_value`; `DynUnit`),
  `ww introspect` const values and `ww observe` print with symbols, Python `ww.Quantity` (`value`, `prefix`,
  `exponent`, `storage`, `symbol`, `unit`, `dim`, `docs`, `to_si()`; writes take a `Quantity` or a number in the unit)
  (`docs/std_library/ww_si.md#in-ww-python-and-dynamic-clients`, `tests/quantities`, `wire_weaver_py/tests`). Types
  are still named by their definitions (`Timed<Quantity<Ampere>>`), alias names need ww_self type aliases.
- ✅ **CLT-30 Link setup budget per transport** · `v0.5.x` — the client re-sent `GetDeviceInfo` 5 times 50 ms apart (250 ms
  for every transport), so connecting through a debug probe (~17 ms per RTT write through a J-Link) mostly failed with
  `LinkSetup was not received`. Now a timeout per transport (USB, UDP, WebSocket 1 s, in-process 250 ms, RTT 10 s, one
  place: `event_loop::transport::link_setup_timeout`), retries back off (first after 1/20 of it, doubling up to 1/4),
  `ClientConfig::link_setup_timeout()` and `ww --link-setup-timeout-ms` / `WW_LINK_SETUP_TIMEOUT_MS` /
  `link_setup_timeout_ms` in ww.toml override it (`wire_weaver_client/src/event_loop/transport.rs`, `event_loop/core.rs` tests). On a J-Link + STM32G0 over RTT the old 250 ms was not what failed in the test: connects succeeded with both budgets once TR-13 was fixed.
- 🚧 **CLT-10 Trait calls that work on any device** · `v0.5` — serial, user label, a CLI that works with all devices.
  `ww list` with filters and USB interface string metadata are done, and global trait calls on any device whatever
  its API version: `ClientConfig::traits_only()`, `DynClient::global_attachment()` (served through API-20,
  `tests/cross_version`), `ww dfu reboot`, `ww counters` (STD-25) and `ww info` / `ww report` (CLT-25).
  Missing: CLI commands for the other `ww_stdlib` traits (GPIO, UART, ...).
- 📋 **CLT-11 Typed property handles** · `v0.5` — `RwProperty`/`RoProperty`/`WoProperty` so that
  `.prop_name().read()`, `.prepare_write(1).blocking()`, `.asynch().await` work.
- 📋 **CLT-12 Stream receive timeout** · `v0.5` — and a beat timeout between updates.
- 🔍 **CLT-13 Typed attachments with `TraitMarker`** · `v0.5` — trait attachments exist (`Attachment`,
  `examples/all_gpio`); check whether a typed marker is still wanted.
- 💡 **CLT-14 `no_std` client** (`client = "raw"`) — documented as not working in `ClientModel::Raw`, its test is
  commented out in `tests/traits`. Its generated code serializes arguments into a fixed 128 byte scratch and
  then calls `.to_vec()`, which isn't `no_std`; it should write into a caller-provided buffer instead. The std
  clients (`std_client`, `trait_client`) already serialize with `to_ww_bytes_owned()` and have no size limit.
- ✅ **CLT-15 Multi read/write/call** · `v0.5.x` — servers with `multi_req = true` dispatch `MultiRead`, `MultiCall`
  and `MultiWrite` (`Same` or `Different` args), each index answered on its own (`Either<Error, value>`), and answer
  malformed multi requests with errors instead of panicking (`tests/multi_req`). Client side: generic `MultiRead`/
  `MultiCall`/`MultiWrite` tuple traits (any arity 2..=8, any depth, not just root 2-tuples), `read_array`/
  `call_array`/`write_array` for homogeneous requests across an array's indices (`List`/`Range`/`Mask32`, `Same` or
  `Different` args chosen automatically), and a `DynResource` equivalent for the dynamic client
  (`docs/api/multi_requests.md`, `tests/properties`, `tests/methods`, `tests/dynamic`). Deferred methods inside a multi request are refused with
  `OperationNotSupported`.
- 📋 **CLT-32 Generic definitions from another revision by the evolution rules** · `v0.5.x` — a definition a device
  leaves out that differs from the host's snapshot (an unpublished crate changed without a bump) is found in the
  definition cache and compared by the evolution rules, but only when it is not generic: for an instance of a generic
  definition (`ww_si::Quantity<U>`, `Timed<T>`) `check_instances` compares identity and signature only, so a docs-only
  change reads as incompatible. Compare the two generic definitions field by field with the arguments substituted.
  Found by STD-38 (`wire_weaver_client` `compat::tests::other_revision` now use the marker `units::Volt`).
- ✅ **CLT-34 stm32-rom-dfu: STM32 system bootloader (ROM DFU) as an updater** — `stm32_dfu_host` (`ww_stm32_dfu`), a
  pure-Rust DfuSe host on nusb in a crate of its own (library first, to move to its own repo like `picoboot_host`):
  lists `0483:DF11`, parses the alt setting's memory layout string (`@Internal Flash /0x08000000/08*128Kg`), erases
  only the sectors an image touches, writes in flash words (32 B, padded with `0xFF`), reads back and compares,
  leaves DFU by jumping to an address (the start of flash by default). ELF (`PT_LOAD` by physical address) and binary
  at an address. `dfu-core` / `dfu-nusb` evaluated first and not used: they own the download loop, have no upload
  (no read-back verify), no sans-IO boundary for a simulated device and no say over the seconds a 128 KB erase takes.
  `follow::stm32_dfu_update()` / `stm32_dfu_update_with()` (`wire_weaver_client`, feature `stm32_dfu`, the `_with`
  one needs neither it nor `usb`) wait for the bootloader under the app's serial number, flash, jump and wait for the
  app, tested end to end on `SimStm32Dfu` (`tests/follow_reboot`); `ww dfu stm32 list/info/flash` with a progress bar,
  `--wait`, `--follow`. Facts: AN2606 Rev 70 §59: H72x ROM DFU on PA11/PA12 USB FS, HSI48+CRS (no HSE), BL ID 0x93,
  start 0x1FF09800; ES0491 2.2.5: USB DFU is dead under RDP level 1. On a real H725 (B135 rev B, 21 Oct 2026): the
  ROM serial is `rom_serial` (UID 000F0009 34315102 34363338, serial 344533413431), the version byte at 0x1FF1E7FE
  is 0x93 (V9.3, AN2606 Table 134: only V9.1/V9.2 lock PWR_CR3, `flash` says "unplug and replug" for them), "get
  commands" answers 0x00 first (the command's code, no version), 220 KB erased, written and verified in 7.7 s.
  "Leave" jumps to the firmware with the ROM's clocks and USB still on: the firmware must reset first (B135 does).
  Missing: option bytes / OTP alt settings are listed but not written.
- 📋 **CLT-16 Verify the hash of a cached API bundle** · `v0.5.x` (`wire_weaver_client/src/local_registry.rs`).
- 📋 **CLT-17 Reliable requests over unreliable media** · `v0.5.x` — a second, non-increasing seq number to guard against
  a lost reply for a request that was executed, when executing twice is not OK. Relevant for UDP.
- 🔍 **CLT-18 Backpressure** — commands already go through a bounded channel (`ClientConfig::cmd_queue_size`), so
  user code should block or yield when the queue is full; add tests for it. A request sent while every
  request id is in flight waits for one. Flow control
  towards the device for sinks: API-28 credit-sinks.
- 📋 **CLT-19 Connection priority** in the client config (`TODO` in `config.rs`).
- 📋 **CLT-20 Bound check of generated byte sink writes** · `v0.5.x` — a `sink!(x: MaxLen<RefVec<'i, u8>, N>)` write
  from a generated client is sent as raw bytes without a length check, the device rejects a longer one
  (`ArgsDesFailed`); the dynamic client and Python already check before sending.
- 🚧 **CLT-21 Follow a device across a reboot into other firmware** — for DFU (`ww_dfu` `BootControl::reboot(Updater)`):
  reconnect to the same device by serial number, whatever answers there, with a timeout. Done for USB:
  `wire_weaver_client::follow` (`Follow`, `Bus`, `Found`), and the RP2350 / RP2040 boot ROM as the updater:
  `ww_picoboot` (PICOBOOT host on nusb, `SimRp2350` for tests), `follow::rp2_update()`, `ww dfu rp`, tested end to end
  on a simulated device (`tests/follow_reboot`, `docs/transport/follow_reboot.md`). The PICOBOOT host moved to its
  own repo, [picoboot_host](https://github.com/vhrdtech/picoboot_host) (evaluated the crates.io `picoboot` 0.3.0
  crate first: no sans-IO boundary to put `SimRp2350` behind, no `Image`/UF2 handling, no orchestrated flash+verify,
  no `chip_id`/`GET_INFO` — adopting it would have meant donating most of `ww_picoboot` upstream anyway); WireWeaver
  depends on it as `ww_picoboot` (a path dependency on `../picoboot_host` while its ELF support, IMG-4, is unlanded;
  a git dependency on github.com/vhrdtech/picoboot_host once it lands, until it is on crates.io). `wire_weaver_client`'s
  `picoboot` feature (off by default) gates `follow::rp2_update()` and the `ww_picoboot` re-export, so a host app
  that only talks WireWeaver over USB doesn't pull in PICOBOOT; `ww dfu rp` turns it on, and `follow::rp2_update_with()`
  (what `tests/follow_reboot` runs against a simulated device) needs neither it nor `usb`. `ww dfu rp flash` takes a
  UF2, an ELF (detected by magic bytes, taken by its `PT_LOAD` segments' flash physical address — no `objcopy`/
  `elf2uf2` needed) or a raw binary. Missing: a run against a Pico 2 / rePort; RTT (another control block: a second
  ELF or a RAM scan) and UDP / WebSocket (the port may change); asking for the reboot from the CLI needs CLT-10.
- 📋 **CLT-22 One `Sink::send` for borrowed and owned items** — owned types (`FooOwned`) implement only
  `SerializeShrinkWrapOwned`, so they go through `send_owned`; one `send` for both (a sink item trait, or the derive
  implementing both), then drop `send_owned`.
- 🚧 **CLT-23 Stream positions, gaps and resume on the host** · `v0.5` — `Commander::prepare_stream_open` /
  `prepare_stream_resume` (what generated code calls, `PreparedStreamOpen`, `StreamOpenResult`), `PositionedStream`
  with `StreamEvent::Item` / `Gap` / `Resumed` / `BackfillDone` / `Disconnected`, live and backfill runs and
  `split()`, `StreamStats`, `AckMode::OnReceive` / `Explicit`, cursors; every stream's queue is bounded
  (`ClientConfig::stream_queue_bytes`, plain streams report `RawStreamEvent::Dropped`); after
  `disconnect().keep_streams()` streams wait and `Commander::resume_streams()` resumes them; `DynPositionedStream`
  and the Python `PositionedStream` (`docs/api/stream_reliability.md`, `docs/python.md`). Tested end to end against a hand-written device: S4 host
  stuck, S6 host restart, resume after a reconnect (`wire_weaver_client/src/event_loop/stream_e2e_tests.rs`).
  Generated clients open streams through it (`tests/streams_reliable`, API-31); the dynamic client and Python pass
  the stream function's arguments and return its result. Missing: counted and reliable streams on trait paths
  (events carry the absolute path), the reconnect itself (EX-10), lossy-medium scenarios (TEST-19).
- ✅ **CLT-47 ww-plugins** · `v0.5.x` — `ww NAME ARGS..` runs `ww-NAME ARGS..` from PATH, as `cargo` does: the device
  selection given before NAME reaches the plugin as the `WW_*` variables, its exit code is `ww`'s, an unknown NAME says
  there is no `ww-NAME` on PATH. `ww dfu flash ARGS..` runs `ww-chunky flash ARGS..` (chunky_mcu_updater's `ww_chunky`;
  the error without it names the install step), which cut the repository cycle: `wire_weaver_cli` no longer depends on
  chunky_mcu_updater, so `cargo install --git https://github.com/vhrdtech/wire_weaver wire_weaver_cli --locked` works
  on any PC. The completion lists plugins on PATH and asks them for the words after their name (`ww dfu flash --<TAB>`
  gives the flags of `ww-chunky flash`). `wire_weaver_cli` is a library too: `wire_weaver_cli::plugin` is the plugin
  API (`DeviceArgs`, `Selection`, `take_exclusive`, `complete_if_requested`, `escape`), so a plugin has the connection
  options, the config file and the completion of `ww`. `just check-clean-clone` builds `ww` from a fresh clone.
- 🚧 **CLT-33 `ww dfu flash`: updates of a chunky device** — `ww dfu flash <ELF|sealed image|bin>`, `status`,
  `confirm`, `revert` (`ww_chunky/src/commands.rs` in chunky_mcu_updater; `ww dfu status | confirm | revert` also run in `wire_weaver_cli/src/cmd/dfu.rs`) on the host library `chunky_ww_host` of chunky_mcu_updater
  (NET-2), now the `ww-chunky` plugin of chunky_mcu_updater that `ww dfu flash` runs (CLT-47): seals an ELF or a
  binary for the device (product and seal scheme from `NodeInfo`, which carries `product_id` and `seal` for it,
  STD-12), plans a delta, sends the steps pipelined up to `StageLimits::in_flight` (`PreparedCall::send` →
  `PendingCall`: requests leave in send order, replies awaited later), a progress bar, reboots into the trial,
  follows the device by serial number and waits for its confirm or revert. `reboot --into updater` on an A/B app
  that takes updates itself says it goes to the boot ROM (`ww dfu stm32`, CLT-34). Tested end to end in
  chunky_mcu_updater (`chunky_ww_host/tests/e2e.rs`: simulated USB HS, delta, full image, lost reply, re-plan,
  revert). Open: on hardware (B135, HW-5 there), the version from the ELF, a CLI-level test.
- ✅ **CLT-41 client-metadata** · `v0.5.x` — who connects: `ww_link::ClientMeta` (program, app version, label, kind
  `Cli|Gui|Notebook|Service|Hub|Test`, pid, user, host, purpose, random instance id) in an appended
  `#[default = None]` field of `LinkSetup` (wire-compatible, conformance cases `link_setup.*`).
  `ClientConfig::client_meta(ClientMeta)` overrides; defaults are found by the library (executable name, pid, login
  and host name, kind `Cli` when the parent process is a shell; no new C dependencies: `/proc`, `libc` on macOS,
  `windows-sys` Toolhelp on Windows), `no_client_meta()` sends nothing, a device that can't receive it gets a
  shortened form or none. `DeviceLink::peer_meta()` on the device; `wire_weaver_py` sets `Notebook`. Docs:
  `docs/transport/hub.md`. Tested on Linux; the macOS and Windows parent-process detection compiles by inspection
  only (no such target here). Used by the hub (SRV-13) and the holder lock files (CLT-38).
- 🧪 **CLT-50 Request priority** · `v0.5.x` — every request carries a 4-bit priority (`ww_client_server::Request`'s
  `priority` nibble, `wire_weaver::Priority`: 0..=15, `NORMAL` = 4 by default, `URGENT` = 8, wire change EVO-14).
  Default traffic follows one plain rule on both ends: a frame goes out when it is full or when the device's
  accumulation window is over, never later (CLT-53), also for a request or reply alone. An urgent request (8 and up)
  is flushed at once by the host, and its reply, immediate or deferred (SRV-42), by the device. API:
  `.priority(p)` / `.urgent()` on `PreparedCall`, `PreparedRead`, `PreparedWrite` and `PreparedStreamOpen`,
  `Commander::set_default_priority`, `ClientConfig::default_priority`; a multi request (CLT-15) goes at the highest
  priority of its items. Through the hub (SRV-29) the nibble stays as the client sent it and the hub's link to the
  device flushes an urgent request at once too (`tests/hub` `urgent_call_through_the_hub_goes_at_once`). Replaces
  the lone request/reply fast paths (CLT-37, TR-12's reply flush), which made latency depend on what else happened
  to be queued. Docs: `docs/transport/ww_framer.md#latency-and-priority`; tests
  `wire_weaver_client` `flush_e2e_tests`, `core::tests::urgent_request_flushes_at_once`, `ww_device`
  `async_server::replies_flushed_by_priority`. On hardware: re-measure the rePort B200A round trips (scratch fixes:
  immediate call 2.69 → 0.54 ms p50, deferred `i2c_read` 5.3 → 2.3 ms).
- 🧪 **CLT-53 An accumulation window never costs more than itself** · `v0.5.x` — the host waited for the window with
  `tokio::time::sleep_until`, which rounds up to whole milliseconds: a 1 ms window (rePort B200A,
  `UsbTimings::fs_higher_speed`) cost ~2.2 ms per request. The tx task now waits on a fine timer
  (`event_loop/fine_timer.rs`: one thread per process for deadlines under 2 ms away, tokio's timer for the rest),
  within tens of µs of the deadline. On hardware: part of CLT-50's re-measure.
- ✅ **CLT-38 Who holds a device** · `v0.5.x` — `wire_weaver_client::holders` (`key_for`, `holder_of`, `list`),
  always on, not gated by a feature: before claiming a USB interface or RTT probe, `claim()` takes an exclusive
  `std::fs::File::try_lock` on `<runtime dir>/ww/holders/<key>.lock` (`$XDG_RUNTIME_DIR/ww` on Linux, `$TMPDIR/ww`
  on macOS, `%LOCALAPPDATA%\ww\run` on Windows, created 0700 and refused if not owned by us) and writes `key = value`
  lines (pid, program, exe, `mode = direct` for now, since, ww version; unknown keys ignored by the reader, so later
  fields — CLT-41's client metadata — add without a format change). A crash releases the OS lock immediately, even
  with stale content left on disk: `holder_of`/`list` only ever trust a lock file while its lock is actually held.
  Device keys `usb:<vid>:<pid>:<serial>[:<iface>]`, `rtt:<probe serial>:<chip>`. A busy error from another
  WireWeaver holder names it (`usb/connect.rs`'s `with_claim_hint`, `rtt/event_loop.rs`'s `with_claim_hint`): "held
  by dg, pid 4711, direct, since 14:02:03 UTC"; without a lock file (a non-ww program: probe-rs CLI, minicom,
  dfu-util), Linux scans `/proc/*/fd` for the device node and names the process, elsewhere it just says "another
  program". Tested with a real second holder (a child process, killed to simulate a crash) and a released claim's
  stale content never reported as held.
- ✅ **CLT-36 A call on a link that went down fails at once** — after the device resets or the link goes (its event
  loop exits), a call fails with `Error::Disconnected` until `Commander::reconnect` connects again, instead of waiting
  in the command channel the residual keeps: a keeper task holds the channel between event loops and answers every
  command that comes in (`prepared_connection.rs`'s `keep`). Tested in `tests/methods`
  (`call_while_disconnected_fails_at_once`).
- 🚧 **CLT-39 Hub routing in `connect`** · `v0.5.x` — direct by default, the hub when it pays off: a device the hub
  holds is reached through it, and repeated short connections (N opens within T seconds, shell-launched calls weighted)
  move a device into the hub, so a loop of `ww` calls doesn't re-open it every time. `HubMode` `Auto`, `Prefer`,
  `Required`, `Direct` (`WW_HUB`); hub found or started (start lock, readiness, newest hub executable: `WW_HUB_EXE`,
  the app itself, `ww`); user code doesn't change. Built (feature `hub` of `wire_weaver_client`, on in `ww`): the
  mode (`ClientConfig::hub`, `WW_HUB`), the open counter in `<runtime>/ww/opens/<key>` (3 in 60 s,
  `ClientConfig::hub_opens`, `WW_HUB_OPENS`; kind `Cli` counts 1, others 1/4), the registry asked whether the hub
  holds the device, the tunnel opened before the event loop starts (so a refusal falls back to direct in `Auto` and
  `Prefer`), `hub::ensure_running` (start lock, stale socket removal, the newest of `WW_HUB_EXE` / the app itself via
  `ww_hub::main_if_hub()` / `ww` next to the exe / on `PATH` by `--hub-version`, started detached, readiness by
  polling the socket with the preamble for 2 s), lock files with the holder's client metadata, busy errors that say
  how to get a hub, `ww hub take` (`hub::take`). See `docs/transport/hub.md#reaching-devices-through-the-hub-clt-39`.
  Tested on Linux (`tests/hub_routing`: two programs starting a hub at once get one, stale socket, no executable;
  unit tests for the modes). Missing: the open counter counts USB only (RTT and `ipc_exclusive` local sockets go through the hub when it holds them,
  their holder hands them over (CLT-43) or in `Prefer` / `Required`; the rest connect directly); a routed USB connection through the
  SRV-29 tunnels is only tested on hardware (P2401#21oct-2103e, the routing picks USB devices only, which tests
  can't fake); a check on macOS and Windows (compiles for both).
- 🧪 **CLT-40 Reconnect after a hub restart** · `v0.5.x` — the client reconnects by itself, re-opens its streams and
  observe subscriptions and tells the app (`Reconnected`); requests in flight fail, never replayed. Built
  (`docs/transport/hub.md#reconnect-after-a-hub-restart-clt-40`): a connection through the hub that loses its link
  (hub exited, crashed, killed, handed over, device lent) is reconnected by the keeper of its command channel
  (CLT-36) with backoff, through a hub again (one started if none runs), and resumes counted and reliable streams,
  observed levels and sinks (`resume_streams`); calls fail with `Disconnected` meanwhile; `ConnectionEvent` /
  `GapReason` from `Commander::connection_events()`; `ClientConfig::hub_reconnect(give_up)` (60 s, lent time not
  counted, no hub started while lent). Tested on Linux in `tests/hub_reconnect` (`restart.rs`; `kill.rs`: `SIGKILL`
  twice under load, back in ~90 ms, stream resumed, nothing sent twice); check on macOS and Windows open. Plain
  streams end with the link, as on an unplug.
- 🧪 **CLT-46 Clock steering in every direct connection** · `v0.5.x` — a program that holds a device directly steers
  its clock onto the PC's UTC without code of its own, as the hub does for the devices it holds (SRV-31): on by
  default, `ClientConfig::clock_sync(false)` or `WW_CLOCK_SYNC=off` to turn it off, `clock_sync_options` for the pace;
  `Phase::ThroughHub` (no steering) while the connection goes through a hub, again after a reconnect that reaches it
  directly. The code of `ww_clock_sync` moved into `wire_weaver_client::clock_sync` (feature `clock_sync`, default;
  `ww_clock_sync` re-exports it), `Clock` with `host_sync` found by introspection (`serves_host_sync`, `now()` tried
  without introspection data), rate cache by the device's serial and the connection's `ClientMeta` as steerer, as in
  SRV-31. The task holds a weak `Commander` for one round of calls at a time, so the connection closes with the
  program's last `Commander`. `Commander::clock_sync()` (`ClockSteering`: phase, status, `set_paused`); `ww dfu`
  pauses it while it holds the device directly, `ww sync` and the hub turn it off (they steer with their own sync).
  `docs/std_library/ww_timebase.md#steering-in-every-direct-connection`; tests `tests/io_timebase`
  (`a_direct_connection_steers_by_itself`, `own_steering_never_keeps_the_connection_open`), `tests/hub`
  (`ThroughHub`). Hardware check open: a board (rePort, Donguru on omarchy-m1) steered by a direct `ww` command or
  app, `ww sync` with a hub showing it followed, no second steerer with the hub.
- 🧪 **CLT-43 Polite handover to the hub** · `v0.5.x` — a direct holder that allows it (by default command-line tools,
  notebooks and tests, not GUIs or services) hands its device to the hub when another program wants it, and goes on
  through the hub after a short gap; one that doesn't is named in the other's error. Built
  (`docs/transport/hub.md#polite-handover-from-a-direct-holder-clt-43`): `ClientConfig::allow_handover` (default by
  client kind, only where the holder can reconnect through the hub); a direct holder listens on
  `<runtime>/ww/holders/<key>.sock` (user-only, peer checked, named in its lock file as `handover = <path>`) for the
  frozen `WWHO` request (`ww_local_socket::HandoverRequest` / `HandoverAnswer`, TLV trailer with the client metadata);
  its event loop lets go of the device (`HandedToHub`, streams kept), the claim is shared by both link halves so
  `Released` comes only once the medium is closed, and it reconnects through the hub on CLT-40's path
  (`GapReason::HandedToHub`, `Reconnected`). `connect()` (USB, RTT, `ClientConfig::ipc_exclusive` local sockets) and
  `ww hub take` ask a direct holder before opening when a hub runs or can start; a refusal fails with the holder's
  name and kind. Release and reopen on every OS. Tested on Linux between two processes on a simulated device
  (`tests/hub_reconnect` `polite_handover.rs`: a `Cli` holder hands over with a counted stream resumed, gap 4.6 ms
  median; a `Gui` holder refuses and is named). Real USB (rePort, omarchy-m1, `WW_LIVE_HANDOVER=off`): link gap 9.7 ms
  median, the calls in flight and in the gap failing (104-2068 per run of 20 calls in flight). Open: an RTT probe's
  gap; a check on macOS and Windows (compiles by inspection only).
- 🧪 **CLT-44 Live link handover on Linux** · `v0.5.x` — the hub adopts a direct holder's live link (link, requests
  in flight, streams and subscriptions), so the device never sees a reconnect and the holder goes on through the hub
  without a gap; elsewhere CLT-43's release and reopen. Built (`docs/transport/hub.md#live-link-handover-clt-44`): the
  holder pauses (OUT drained, IN cancelled and reaped into the framer), sends its residual
  (`wire_weaver_client::adopt::HandoverOwned`: the link residual, the same on every medium, and a medium residual only
  the taking medium decodes) to `ww_hub_api::Adopt::prepare`, releases the interface keeping the device open (no
  bus traffic at alternate setting 0, toggles kept), `commit` has the hub open the device itself and go on with the
  link (`PreparedConnection::adopt`), the holder's tunnel attaches by adoption id (`TAG_ADOPT`) and its event loop
  puts the tunnel in its one transport slot (a relink from any medium to any other) keeping seqs and waiters. No fd passing: nusb 0.2 can't give one out, and a shared open
  file's claim goes with the holder's `Interface`. Needs the same `wire_weaver_client` minor in the hub. Rollback when
  the hub dies before it holds the device, cleanup after `HubConfig::adopt_attach` when the holder dies. Real USB on
  Linux (nusb) and the simulated USB device (`usb_sim`). Tested between processes on `usb_sim`
  (`tests/hub_reconnect` `live_handover.rs`: handed over in 8.5 ms median with 21 requests in flight, nothing lost or
  doubled, the device's link up once; hub or holder killed in the commit, interface grabbed in the gap, a prepared
  adoption whose connection went, answers waiting in the device). On a real board (rePort, omarchy-m1, `live_bench`):
  20/20 handovers in 16.7 ms median with 20-21 requests in flight, no failed call, no connection event, no link setup
  or control transfer from the hub (its trace); CLT-43 on the same board: 104-2068 failed calls per run.
  `WW_LIVE_HANDOVER=off` falls back to CLT-43. Open: a usbmon trace of one handover showing nothing at all on the bus
  (root on omarchy-m1: `sudo modprobe usbmon`, `sudo cat /sys/kernel/debug/usb/usbmon/1u > /tmp/usbmon-1u.txt` while
  `live_bench` hands over the rePort on bus 1; no `Ci`/`Co` control transfers in the window).
- ✅ **CLT-45 Lock file names one per device key** — `holders` used to map every character outside
  `[A-Za-z0-9._-]` to `_`, so two keys could share a lock file (`...:a:b` and `...:a_b`) and a USB device with a
  crafted serial could make another one look held. Now lower-case letters, digits and `-` stay and every other byte
  is `_xx`, one file per key also on a case-insensitive filesystem; long names are the key's SHA-256 (the handover
  socket's too, was FNV), and the key inside the file is checked on read.
- 📋 **CLT-48 Fallible methods in multi calls** — `MultiCall` (CLT-15) only takes infallible methods, so a method
  returning `Result` (`ww_i2c::I2c::i2c_read`, any method with `#[errors]`) can't be batched into one round trip;
  found on the rePort B200A bench run (report-i2c-master, 27 Oct 2026), where a write + read burst to a USB-C PD
  controller wants exactly that. Needs each index's answer to carry the method's own `Result` next to the
  per-index `Error`.
- ✅ **CLT-48 Fallible methods in multi calls** · `v0.5.x` — a method returning `Result<T, E>` (any with
  `#[errors]`, e.g. `ww_i2c::I2c::i2c_read`/`i2c_write`) goes into a `MultiCall` tuple (CLT-15) and into
  `call_array`, mixed with methods without `Result`; each position answers what its single `call()` would,
  `Result<T, CallError<E>>` (`Device` checked against the `#[errors]` list, `Unimplemented`, `Client` for the
  index's own framework error), methods without `Result` still `Result<T, ErrorOwned>`. No device change: servers
  already put the method's `Result` into the index's slot. Generated client calls with arguments take `&self` now
  (was `&mut self`), so two of them fit into one tuple. Found on the rePort B200A bench run (report-i2c-master,
  27 Oct 2026): a write + read burst to a USB-C PD controller in one USB round trip. Host-tested (`tests/methods`:
  2- and 3-tuples, `call_array` async and blocking, Ok, device `Err`, `Unimplemented` and `BadIndex` mixed),
  `docs/api/multi_requests.md#methods-returning-result`.
- ✅ **CLT-49 Same method twice in a multi call** · `v0.5.x` — a `MultiCall`/`MultiRead`/`MultiWrite` tuple
  naming the same resource more than once (two `ww_i2c::I2c::i2c_write` in one burst) failed on the client with
  "resources are identical", though the server already handles `MultiIndex::List([id, id])`. The client now sends
  the repeated id, answers come back in tuple order. `call_array` and friends still refuse an accessor that builds
  the same path for every index. Host-tested (`tests/methods` `multi_call_same_method_twice`: 3-tuple at the root,
  2-tuple in an array item, `Same` and `Different` args, Ok and device `Err` mixed, infallible too),
  `docs/api/multi_requests.md#several-resources-at-the-same-level`.
- ✅ **CLT-54 `exact_units::Fixed` in dynamic clients, `ww` and Python** · `v0.5` — a `Fixed<U, P, T, B>` (STD-55)
  read from introspection data alone (`quantity::fixed_of`: unit, scale as a `DynPrefix` with the binary and a
  crate's own scales, `Storage { base, bounds }`), shown as the typed `Fixed` shows itself (`1730000000000000 µs`
  exactly from a `u64`, `4 KiB`), `ww introspect` with `[ms, 1..=5000]` after the type; written exactly into the
  stored scale and type from `q!(5 ms)` or another quantity of the dimension (not whole, overflow or out of bounds:
  an error naming the type), a plain number as the stored one; `Index` / `IndexFrom1` / `Seq` (STD-58) shown
  `#3 (Sample)`, `seq 41 (Frame)` and checked (bounds, an `IndexFrom1`'s 0) in the dynamic codec. Python
  `ww.Quantity` for a `Fixed` (`value`, `prefix`, `storage`, `fixed`, `bounds`, `to_si()`), writes take a `Quantity`
  or the stored number; `ww.requantize` and C `sw_requantize.c` bit for bit with `exact_units`. `tests/quantities`
  (property, method argument and return, stream item, counts through `in_process`), `tests/customary` (the MCO
  `Fixed` field), `wire_weaver_py/tests/test_offline.py`, conformance `stdlib.si.*` green in every consumer (C's
  model-built cases wait on API-26). A GUI's slider limits come from `Storage::bounds`; the widget is TOOL-6's.
  [In clients](docs/std_library/exact_units.md#in-ww-python-and-dynamic-clients).
- 📋 **CLT-52 Host sends waiting requests by priority** — today priority (CLT-50) only decides when a frame is
  flushed; requests waiting for a free seq or for the transport go out in the order they were made. Sort that queue
  by priority (the event loop's, before the framer), so an urgent request overtakes a long queue of default ones.
  `TODO(CLT-52)` in `event_loop/core/tx.rs`.

## Transports and protocols (`TR`)

- ✅ **TR-1 `ww_framer`**: packs many small messages into one packet or splits a big one across packets; U2Head with
  1 byte overhead, CRC, recovery from missed frames, fuzzed.
- ✅ **TR-2 `ww_link`**: link setup, device info, API version exchange.
- ✅ **TR-3 USB**, **RTT**, **WebSocket**, **UDP** and **in-process** (messages, or simulated USB: real USB framing
  and packet sizes, `in_process::usb_device`).
- ✅ **TR-4 USB CDC-NCM**: WebSocket over a USB network adapter, with a DHCP server so the host gets an address by
  itself, ping and a hello page over HTTP (`examples_mcu/rp2`, `ww_ws_ncm`); UDP the same way (`ww_udp_ncm`).
- ✅ **TR-14 Link tap** · `v0.5` — what crosses a link, recorded at the transport edge (`docs/transport/tap.md`):
  `ww_tap` (`no_std` `Tap` / `NoTap`; records are streams of the `LinkTap` trait: session, link events, frames and
  messages each way, client map), taken by every `wire_weaver_client` transport and event loop
  (`ClientConfig::tap`, `tap::default_recorder()`; `TraceEvent` / `RegisterTracer` an adapter over it, the iceoryx2
  USB tracer removed) and by `ww_device`'s `FramedTx` / `FramedRx` (`with_tap`; with `NoTap` a G0 firmware is the
  same size). `ww_framer::pieces(frame)` shares `FramedRx`'s head code, no callbacks in the framer. `Recorder`
  batches into pooled 64 KiB `ww_tape` blocks (STD-42) flushed when full or after `flush_time` (10 ms), no
  allocation in steady state, losses counted in `LinkEvent::Lost`. `wire_weaver_client::tap::Decoder`: frame ↔
  message linkage, cross-check against the message records, framer and link statistics, request/reply pairing by
  seq feeding the TOOL-9 explain code. E2E tests over simulated USB (`tap/e2e_tests.rs`, `wire_weaver_cli`
  `cmd/tap/tests.rs`): shared packets, split replies, a lost packet, stats, captures replayed identically. Open:
  the tap of a ww server's clients (`ClientMap` records are defined, nothing writes them yet).
- ✅ **TR-15 Frame prefix and link setup token** · `v0.5` — stale frames after a reconnect are removed at the
  cause and each medium gets the checks it needs (`docs/transport/ww_link.md#setup-token`,
  `docs/transport/ww_framer.md#frame-prefix`): the host puts a random 64-bit token into link setup, the device
  echoes it and starts its reply in a fresh framer session, the host drops everything before it
  (`previous_session_leftovers_are_dropped`). `ww_framer::prefix` (`FramePrefix`, `NopPrefix`, `SeqPrefix`: 4-bit
  frame counter, 2-bit epoch; lost, duplicate, late and stale frames reported and counted), checksums over prefix
  and head, fuzz target `framer-lossy`. `ww_link::Framing` per medium: USB without a checksum and without a prefix
  (`SeqPrefix` as a diagnostic, `wire_weaver_usb_embassy` feature `seq-prefix`, told by the interface protocol),
  UDP with `SeqPrefix` and a CRC-32 (`ww_block`'s) on every message. `ww stats` / `ww monitor` show the prefix.
  Wire change of the link (`docs/evolution/crate_layers.md`).
- 💡 **TR-5 USB CDC-NCM extras** — test on Windows, macOS, iOS and Android; serve files from an SD card over HTTP;
  mass storage with a README.
- 📋 **TR-6 CAN** · `v0.5.x` — server and client, using CANopen (`docs/transport/overview.md`).
- 📋 **TR-7 UART** · `v0.5.x`.
- 💡 **TR-8 IP / WireGuard** · `v0.5.x`.
- 💡 **TR-9 EtherCAT**.
- ✅ **TR-13 RTT server panicked on the first request** — since TR-12 the async `Server::handle` checked for a waiting
  request with `embassy_futures::poll_once(rx.wait_message())`, a no-op waker; the RTT medium waits on an
  `embassy_time::Timer` while no data is there, and embassy-time panics on a waker not from its executor ("Found
  waker not created by the Embassy executor"). Every RTT device reset on its first request, the host saw no
  `LinkSetup` (`examples_mcu/nucleo_g0b1re` `ww_rtt` and an embedded_bedrock template firmware on Donguru, 16 Oct
  2026). Fixed: the check polls once with the calling task's waker (`ww_device` `server.rs` `poll_once_here`);
  checked on Donguru over RTT through a J-Link. USB was not affected (no timer in its read).
- 🧪 **TR-16 Local socket transport** · `v0.5.x` — client feature `local_socket`: a device served by another process
  on the same PC (`ClientConfig::ipc_path`), over a Unix socket (Linux, macOS) or a named pipe (Windows) through
  `interprocess` (crate `ww_local_socket`): user-only (0700 runtime dir, 0600 socket, owner-only pipe DACL, the peer's
  user checked on both ends), the frozen `WWHB` preamble that picks the hub or a device, then `len | kind | payload`
  messages. Device side `local_socket::device::device_medium()` for `ww_device::Server`
  (`docs/transport/hub.md`). Tested on Linux, end to end with the device e2e suite; check open on macOS and Windows
  (compiles for both).
- 💡 **TR-10 One-shot connect** — no link setup at all: the host sends the API hash and its requests in one go; when
  the hash matches, the device runs them right away, else it answers with its hash and drops them and the normal setup
  follows. With message aggregation both arrive in one packet or frame, so a request over LoRa or LTE-M costs no extra
  round trip (introspection skipped by hash, CLT-7). Open: replayed and duplicate packets (CLT-17), which requests may
  run unannounced, how the hash is versioned. Needed by `ww_dfu` updates over slow links.
- 💡 **TR-11 MQTT / store-and-forward transport** — devices offline most of the time pick up a retained message on
  wake (an update plan, config), replies queued on the broker. Look at MQTT-SN and LwM2M first.
- 🚧 **TR-12 USB throughput on small MCUs** — measured with `ww usb-loopback` on Donguru (STM32G0B1, Cortex-M0+
  64 MHz, USB FS behind the board's HS hub, 15 Oct 2026). Done: U2Head encodes and decodes whole bytes in u32
  instead of bit by bit (`ww_framer` `framed.rs`, was ~15% of the device's CPU per packet); the async device server
  flushed replies at once when no other request waited (`ww_device` `Server::handle`), removed again for request
  priority (CLT-50, SRV-42: a reply goes out at once only when its request is urgent). `WireWeaverClass::with_endpoints()` lets
  firmware allocate the endpoints, e.g. double-buffered bulk ones (Donguru, embassy-rs/embassy#7275: raw bulk 500 ->
  950 kB/s OUT on the G0); `usb_init_with_endpoints()` offers the same choice to callers of the single-function
  `usb_init()` convenience, who can't reach into its private `UsbBuffers`. Next: CRC through a pluggable checksum
  so MCUs can use their CRC unit (~13% of the CPU per packet on the M0+). The lone request and reply flushes once
  planned here are superseded by request priority (CLT-50).

## Introspection (`ww_self`, `INT`)

- ✅ **INT-1 API model AST** with syn-style `Visit`/`VisitMut`, sent by devices as introspection data.
- ✅ **INT-2 Bundles** recording crate versions, signatures of skipped traits and types.
- ✅ **INT-3 Relocated `#[flag]` fields** in introspection data and dynamic serialization.
- ✅ **INT-4 Bit-aligned integers in `ww_self`**: `U1`..`U64`, `I2`..`I64` and nibbles are in the model (`UB` / `IB`,
  `Nibble`) and the dynamic client and Python read and write them, also as struct fields and enum payloads (done
  with SW-48); checked byte for byte by the conformance matrix (TEST-23, `bits.*`, `struct.bits`, `enum.sub_byte.*`).
- 📋 **INT-5 Documentation pass on `ww_self`** · `v0.5` — doc comments are missing (`TODO` in `ww_self/src/lib.rs`).
- 📋 **INT-6 `sized` / `final_structure` on `ww_self` types** · `v0.5` — where a type is unlikely to evolve, to make
  introspection data smaller. Can't be undone later, so decide per type.
- 📋 **INT-9 `TailBytes` and `TailSize` in the API model** — a user type with a `TailBytes` (SW-9) or `TailSize<N>`
  (SW-18) field can't be in an API: the crate walker doesn't find the types (`Type TailSize not found`, `Module
  shrink_wrap not found` for `shrink_wrap::tail_bytes::TailBytes`), and `ww_self::Type` has no form for them, so
  the dynamic client and Python can't read or write them. Conformance cases `tail_bytes`, `struct.tail_size`.
- 📋 **INT-10 `#[default]` values in the API model** — the crate walker records only that a field has a default
  (`Field::default` is a placeholder `Bool(false)`, a `TODO` in `wire_weaver_core`'s `get_default_attr`), so the
  dynamic client and Python read a field missing from older data as its type's default: `#[default = 7] timeout:
  u16` reads as 0. Convert the default expression (literals, `None`, `Some(..)`, unit variants) to a value and use
  it in the dynamic decoder; mind that `Field::default` is part of API signatures. Conformance case
  `evolution.new_reads_old`.
- 📋 **INT-11 `Instance` and `Param` have no conformance case** — found by the coverage guard (TEST-25):
  `ww_self::generics::prepare` (API-43) turns every `Type::Instance` into an `OutOfLine` reference to a
  monomorphized struct/enum before any client sees a bundle (a device's own bundle is prepared too), and
  `Type::Param` only remains inside the now-unreferenced generic definition, so neither is reachable from any
  property's resolved type. Testing them needs a conformance mode over the introspection bundle's own wire
  encoding, not over app-level property values as the corpus does now.
- 💡 **INT-7 Compress `ww_self` strings with a pre-shared dictionary** — partly addressed by leaving snapshot-known
  traits and types out of introspection data.
- 💡 **INT-8 Model enums that tolerate newer variants** — the `ww_self` enums (`Type`, `ApiItemKind`, `Fields`, ...)
  and `ww_numeric::NumericAnyType` are `no_future_version` (SW-47): a host reading a newer model fails as a whole.
  With `Unknown` handled in every walker (visitor, signature, inline, values, compat, CLI, Python), an older host could
  show the rest of a newer device's API and mark the unknown parts.
- 💡 **INT-12 Bounds as a type of the API model** — `Type::Bounded { ty, min, max }` (like `MaxLen`), checked by every
  consumer (the generic decoder, C, Python), shared with API-29's index bounds. Until then bounds are const arguments
  of `ww_si` markers (STD-55) that only `ww_si`-aware clients check; a new `Type` variant needs a model version step.
- ✅ **INT-13 Unit definitions in the API model** · `v0.5` — every unit, scale and origin marker an API uses carries
  its definition in `ww_self` (`ItemStructExtra::marker`, a `MarkerDef`: symbol, `Dim`, exact `Magnitude`; an origin's
  unit, reference, exact offset, modulus and step), named by crate and marker (D16b), so clients know a user crate's
  units (`unit!`, `scale!`, `origin!`) without a built-in table. `ww_self::units::unit_of` gives a canonical sorted
  unit expression (F#-style: `Per<Meter, Second>` equals `Times<Per<Ratio, Second>, Meter>`) with exact
  `factor_to`, `scale_of` and `origin_of` the factor of a scale and the definition of an origin. The walker and
  snapshots fill it, appended with `#[default = None]` so plain structs keep their bytes and signatures and older
  clients skip it. Conformance `model.marker_def.*` / `model.item_struct_extra.*` through derive, dynamic client and
  Python (C: API-26). Firmware: no code cost (markers stay zero-size), introspection bytes only for the crate's own
  markers (`Rpm` 22 B, an origin ~40 B; TEST-27). Levels (STD-67), kinds (STD-68) and sentinels (STD-66) append
  their `MarkerDef` variants when their markers exist. [Units in the API model](docs/std_library/units_in_the_model.md).

## Evolution, versioning and compatibility (`EVO`)

- ✅ **EVO-1 Evolution rules** in `docs/evolution/rules.md`: crate layers and version bumping.
- ✅ **EVO-2 SHA signatures** of APIs and types, hashing only the major version (or `0.minor`), version independent.
- ✅ **EVO-3 Crate version recorded in bundles**.
- ✅ **EVO-4 Global traits referenced by hash**, global type registry (`ww_global`): one id per `crate::Trait` for every
  public `ww_stdlib` trait, by hand in `wire_weaver_gid.json`, renames keep the id through `aliases`, ids never
  reused (`retired`), `assigned_ids.lock` append-only and checked; `ww api check` flags a changed id (API-20).
- ✅ **EVO-5 API snapshots** of `ww_global` and `ww_stdlib` (`wire_weaver_snapshots`, `ww api save`), left out of
  introspection data and put back by the client.
- ✅ **EVO-6 Evolution checker and report**: `ww api check` / `ww api diff` (`docs/evolution/checker_tool.md`).
- ✅ **EVO-9 Skipped definitions from another revision** · `v0.5.x` — codegen keeps every definition it leaves out
  of a device's introspection data in `~/.wire_weaver/definitions/` by signature; a host that doesn't find a skipped
  definition under its signature in its snapshots (a device built from another revision of an unpublished crate)
  takes it from there and compares it by the evolution rules, with a warning instead of an error. Not found anywhere:
  an error with both signatures and the fix. Rust client, `ww` and Python (`docs/std_library/ww_self.md`,
  `wire_weaver_client` `compat::tests::other_revision`). `#[cfg]` enum variants don't change signatures
  (`tests/cfg_variants`).
- ✅ **EVO-11 Evolution check at commit time** — the checker is its own light crate, `wire_weaver_evolution` (parses
  the API crate from source, no client, tokio or probe-rs), which an API crate runs from its own `tests/evolution.rs`
  (`Check::new(env!("CARGO_MANIFEST_DIR")).strict(true).assert()`), so its lock file pins the checker and
  `cargo test`, CI and a pre-commit hook check the same way. Strict mode fails a crate without snapshots, naming the
  `ww api save` to run; every breaking change names the rule it breaks with its docs section, and a failed check the
  version to bump to. `ww api check --strict` runs the same library. Template setup (test, just recipes, CI step,
  pre-commit hook) in `docs/evolution/checker_tool.md#checking-at-every-commit`; `examples/blinky_api` does it.
- ✅ **EVO-12 Current version's snapshot required** — `Check::require_current(true)` (and `ww api check
  --require-current`) fails when the crate's current version has no snapshot or the source differs from it, and checks
  the bump from the previous version's snapshot to it, so a version can't ship without its snapshot (the
  `nucleo_g0b1re_template` 0.1.5 shipped API 0.2.0 without one). The snapshot is saved from the API crate's own test
  (`WW_API_SAVE=1 cargo test --test evolution`, `Check::save`), with the checker the lock file pins; `ww api save`
  refreshes the current version's snapshot too, never an older one
  (`docs/evolution/checker_tool.md#checking-at-every-commit`, `wire_weaver_evolution/tests/check.rs`).
- ✅ **EVO-10 Checker knows the `Unknown(u32)` catch-all** — `ww_self::ItemEnum::future_version` records an enum's
  `#[future_version]` catch-all (SW-47), set by codegen; the evolution checker (`ww api check`), the client's runtime
  compat check and the dynamic decoder use it: a variant appended to an enum with the catch-all is compatible, an
  inserted or reordered one is reported as moved, and without the catch-all a new variant stays breaking. Snapshots
  saved before EVO-10 count as without it until saved again, and compare clean otherwise (EVO-13)
  (`docs/evolution/rules.md#unsized-types`,
  `wire_weaver_evolution` tests `enum_variant_appended_to_the_catch_all`). Found with `ww_timebase::SyncState::Conflict`.
- ✅ **EVO-13 Upgrading wire_weaver never forces an API bump or re-save** — a field of the API model that a snapshot
  file has in no instance (written by an older wire_weaver: `ItemEnum::future_version` before EVO-10) is not
  recorded, and the checker compares both sides without it and says so, instead of the unchanged source counting as
  changed and `require_current` failing (G0's `nucleo_g0b1re_api` 0.4.0 after EVO-10). Read from the file itself, no
  list of fields or `ww_self` versions: the next model field gets it too, also a non-`Option` one, which DER-17 makes
  readable (`docs/evolution/checker_tool.md#the-current-versions-snapshot`, `wire_weaver_evolution/tests/check.rs`
  with G0's old-format snapshots as a fixture).
- ✅ **EVO-14 Priority nibble in `Request`** · `v0.5.x` — **wire break** of `ww_client_server`'s `Request`: a
  `priority: Nibble` after `seq` (CLT-50). A host and a device must both have it; nothing with the old layout was
  released. Conformance cases of `Request` carry the new bytes with `"wire_change": "EVO-14 / ..."`.
- ✅ **EVO-16 `ww_si::Dim` with angle, information and counted bases** · `v0.5` — the wire change of `Dim` that
  STD-60 brings, in `SIExpr` / `AnyQuantity` and in the `ww_si` snapshot: no longer `sized` but `final_structure`,
  nine 4-bit exponents (rad and bit appended to the seven SI ones), a root bit and four optional counted bases
  (crate and marker name); a non-canonical `Dim` is refused on decode. Volt is `d2 1f 00 00 00 00`. `Scale` gains
  `Exact` (an `ExactScale`, a degree is exactly π/180) in the same break. Conformance cases `stdlib.si.dim.*`
  (`tests/conformance/corpus.json`), [ww_si](docs/std_library/ww_si.md#units-known-only-at-run-time).
- ✅ **EVO-15 Transparent wrappers in the checker** · `v0.5` — `T` and a `transparent` wrapper of `T` (DER-18,
  `ww_si::Fixed`) are compatible, reported as the same bytes (`u64 -> Fixed<Second, Micro, u64, Unbounded>, same
  bytes`); a change of a `Fixed`'s unit or prefix is breaking although the bytes stay (a reader takes µs for ms,
  `Rule::Meaning`): any type parameter no field uses is a marker compared by name; a bound narrowed or widened is
  breaking for the side it refuses (`Rule::Bound`, compared by value); `Quantity` → `Fixed` is breaking with a hint
  that it is the intended move. A skipped `ww_si` definition is read from the embedded snapshot, also on prepared
  bundles (the client at connect time)
  (`docs/evolution/rules.md#transparent-wrappers-units-and-scales`, `#bounds`; `wire_weaver_evolution` tests
  `tests::transparent`, `tests::ww_si_fixed`).
- 💡 **EVO-7 Git dirty flag** — not implemented, although the old tracker had it as done: neither `ww_version` nor
  codegen record git state. Uncommitted API changes are still caught, since they change the API's SHA
  signature. Firmware git info (sha, dirty, branch, tag) can be exposed through `ww_firmware_info`'s
  `bedrock_build_info` stream. Decide whether the link or version also needs it.
- 📋 **EVO-8 Snapshots of crates.io crates** — depends on "Load API crates from crates.io" above.
- 📋 **EVO-17 Units compared by meaning** · `v0.5` — the checker compares magnitudes by value (`Milli` → `Pow10<-3>` is
  compatible), reports origin, level, kind, sentinel and overflow policy (`Unbounded` → `Wrapping`, same bytes)
  changes, and calls the same bytes in a different scale (lbf·s vs N·s) breaking.

## Tooling: CLI, GUI, Python (`TOOL`)

- ✅ **TOOL-1 `ww` CLI**: `list` with filters, `introspect` as a resource tree (`--raw-as-sent`, `--types`), dynamic shell
  completions (`docs/cli.md`).
- ✅ **TOOL-2 `ww api save` / `check` / `diff`** for API bundles and snapshots.
- ✅ **TOOL-3 `ww api scaffold`** generates server handler stubs and the matching `ww_codegen!` call.
- ✅ **TOOL-4 Python module** talking to any device, API built from introspection, dynamic values serdes, RTT built in
  (`wire_weaver_py`, `docs/python.md`).
- ✅ **TOOL-5 Runtime literals**: values to bytes at runtime (`eval`, dynamic values).
- ✅ **TOOL-10 `ww usb-loopback` link test** · `v0.5` — round trips with lost and corrupted counts, host → device and
  device → host speed, on the sans-IO event loop (`wire_weaver_client/src/event_loop/core/loopback.rs`, windowed so a
  slow device holds the host back; e2e test against `ww_device::Server` in `device_e2e_tests.rs`), with a summary
  table and tuning options. Works with any device, the link layer answers. Checked on a full-speed board (Donguru,
  3 hubs): about 155 round trips/s, 375 kB/s host → device, 317 kB/s device → host with 1 KiB messages.
- ✅ **TOOL-11 Dynamic observe** — decodes a level's `PropertyUpdate` from introspection alone (CLT-28):
  `DynResource::observe()` / `observe_blocking()` (`observable_properties()` lists what it can ask for), reusing
  the CLT-28 event-loop routing and subscription sharing through a schema-less `DynUpdate`/`DynConfig` pair; `ww
  observe <path> [--rate MS]` prints each update's values, rate and link bytes/s (`wire_weaver_client::DynObserver`,
  `wire_weaver_cli/src/cmd/observe.rs`, `tests/observe`); `wire_weaver_py`'s `Node.observe(cfg)` iterates the same
  updates as dicts. `ww_self` carries the `sparse` flag a dynamic decoder needs (DER-13).
- ✅ **TOOL-12 `ww settings dump`** · `v0.5` — host decoder of a `ww_settings::Store` flash dump, no device needed:
  `ww_settings::dump::analyze` (pages, every item slot valid/cut/garbage/erased, the current record of every key
  cross-checked against a real `Store` over the same bytes through `fake_flash::FakeFlash`) and `decode_group` for
  a caller that knows the concrete `SettingsGroup` type; `ww settings dump FILE --word-size --page-size [--base]
  [--format text|json]` in `wire_weaver_cli` (`docs/cli_reference.md#ww-settings`).
- ✅ **TOOL-14 `ww monitor`, `ww stats`, `ww capture`, `ww explain FILE`** · `v0.5` — the link tap (TR-14) in the
  CLI (`docs/cli_reference.md#ww-monitor`): live decoded print at the frame, link or API layer (`--bytes`,
  `--bits` for the TOOL-9 bit map of every message), framer and link statistics every few seconds or of a capture,
  captures with the device's API bundle in them, read back by `ww explain FILE` exactly as monitored live.
  Blocks come from a `Feed` (`wire_weaver_cli/src/cmd/tap.rs`): the CLI's own connection today, a ww server's tap
  of all its clients later.
  Global `--monitor[=frame|link|api]` (`--monitor-bytes`, `--monitor-bits`) and `--capture FILE` on every command
  that connects print (to stderr) or save that command's own connection, with the same `Monitor`, `LiveFeed` and
  `TapeWriter`; the tap decoder orders a block's records by time so a reply is paired with its request
  (`docs/cli_reference.md#ww-monitor`).
- ✅ **TOOL-18 The hub from Python and notebooks** · `v0.5.x` — the wheel (`wire_weaver_py`) carries
  `wire_weaver_client` with its `hub` feature and `ww_hub`; `python -m wire_weaver.hub` serves the hub, found by
  `wire_weaver_client::hub::ensure_running` the same way as a Rust `ww_hub::main_if_hub()` program (`wire_weaver`
  registers `sys.executable`, `["-m", "wire_weaver.hub"]`, at import). `connect()` takes `hub_device` (straight to
  a device key through the hub) in addition to the usual filters, and follows `WW_HUB` / `WW_HUB_OPENS` the same
  as a Rust client. `wire_weaver.hub()` connects to the hub's own API (`ww_hub_api`) through the dynamic client,
  starting the hub if none runs (devices, clients, `for e in hub().registry.events: ...`). Fixed on the way:
  `connect_blocking()` had no `InterfaceKind::Ipc` branch, so every blocking connect through a local socket or the
  hub silently failed. Tested without hardware (`tests/hub_pytest_device` hosts the hub and an in-process device
  in one binary): a kernel restart reconnects well under a second with the device still open and steered, two
  Python processes share one device through the hub's tunnel (`wire_weaver_py/tests/test_hub.py`).
  `docs/transport/hub.md#python-and-notebooks-tool-18`.
- ✅ **TOOL-19 Escape device strings in `ww` output** — `Untrusted`/`escape` (`wire_weaver_cli/src/untrusted.rs`)
  turn a C0/C1 control character, DEL or a Unicode bidi override/isolate in a device- or hub-supplied string into a
  visible `\u{..}` escape (`\n`/`\r`/`\t` as their short two-character form) before it reaches the terminal, so a
  device can no longer write terminal escape sequences through its USB product/manufacturer/serial strings,
  introspection docs/idents, `UserStr` text, pin names or hub/holder program/label/user/host fields; `--json` is
  untouched (JSON's own escaping already covers it), and the CLI's own ANSI colour codes stay outside the escaped
  text. `docs/cli_reference.md#untrusted-device-and-hub-strings`.
- ✅ **TOOL-20 `ww` over RTT** — every device command reaches a board on a debug probe (probe-rs, `wire_weaver_client`'s
    `rtt` feature): `--rtt CHIP`, `--probe SERIAL|VID:PID[:SERIAL]`, `--elf ELF` (control block address, no RAM scan),
    `--probe-speed-khz`, as env variables (`WW_RTT`, ...) and `ww.toml` keys; USB filters next to it refused or left
    out by source, `--api` checked once connected; `ww list --probes` with who holds each; `ww sync` and `ww dfu` go
    through the hub with the key `rtt:<probe serial>:<chip>`; completion of chip names and live probe serials; a busy
    probe names its holder (lock file, or `/proc` for probe-rs/OpenOCD). `docs/cli.md#through-a-debug-probe-rtt`.
    Tests: `wire_weaver_cli` `device::tests`, `tests/rtt.rs`; the RTT byte path itself in
    `wire_weaver_client/src/event_loop/device_e2e_tests.rs`. Hardware (omarchy-m1, 23 Oct 2026, Nucleo G0B1RE with
    nucleo_g0b1re_template 5bcbba0 through its ST-LINK V2-1): `ww info` in 0.4 s with `--elf`, 1.7 s with the RAM scan;
    `ww counters read` and `ww sync` reach the device and get `TraitNotFound` (the template serves neither yet);
    `held by ww, pid ..` and `held by probe-rs (pid ..)`; `ww hub take rtt:..` and two `ww info` at once through the
    hub. Found and fixed on the way: a stale control block of the previous firmware won the RAM scan, `ww hub take`
    timed out while the hub scanned.
- 🧪 **TOOL-16 `ww sync`** — several devices kept on the host's UTC at once (STD-49, crate `ww_clock_sync`): devices
  by serial or label parts (or every device the selection matches), a live table per device (state, followed source,
  rate in ppm, shortest round trip, the residual offset measured with `shared_now`, samples), `--mode` writes the
  `ww_indication` mode on all, `--duration`, `--poll-ms` / `--steer-ms`; `q` or Ctrl+C stops, saying who steers the devices after (the hub, or nobody: holdover).
  With a ww hub running it shows the hub's steering (SRV-31) and never steers itself; a STEERED BY column
  (checked through the hub on Donguru and B135, 22 Oct 2026).
  Convergence (21 Oct 2026): residual ± half the check's round trip, the device's error bound and slew left, a line
  when each device and then all are within `--within-us` (locked by their word, residual by the host's), the
  phase spread between them; steered at 10 Hz until locked; quiet into a pipe. Hardware (omarchy-m1, 21 Oct 2026, `ww sync` with Donguru B153A, rePort B200 and B135B): all three locked within 2 ms after 1.2 s from a fresh start and 2.4 s after 45 s in holdover; phase spread as ww sync estimates it 30 µs to 1.0 ms apart (±1.5-2.5 ms, rePort's and Donguru's ~1.4-1.8 ms round trips); over 5 min Donguru left once on a -2.0 ms residual (±1.2 ms) and was back in 37 s. No logic analyser on this PC: the phase is not measured on the pins. `docs/cli_reference.md#ww-sync`. Hardware check open: Donguru and rePort on omarchy-m1. Skill tree signal and skill:
  tpm P2618#21oct-1707.
- 🧪 **TOOL-15 `ww pins test` / `ww pins list`** · `v0.5.x` — pin tests on any device serving `ww_pin_diag::PinDiag`
  (STD-47) at any path, found by introspection (`docs/cli_reference.md#ww-pins`): pulls, shorts to GND, to the
  supply and between points, only what each point's caps allow, refusals shown, `release_all` at the end also on
  Ctrl-C and errors, exit 1 on a fault. Points by names, `P<n>` ids, ranges and globs on names and groups, `--watch`
  points only read; plan files with `expect`, narrower `allow`, `[shorts] among` / `same_net`, `--save` into
  `expect`. Library `ww_pin_probe` (`probe::run` over a `Pins` trait, `mock::MockBoard`), end to end in
  `tests/pin_diag`. Hardware check open: on rePort once it serves `PinDiag`. Names sent as ids show as `#<id>`
  (STD-48).
  The live decoder gets the device's API as soon as the command has it (the recorder hands a bundle over at once,
  as the device sent it, snapshot-known traits put back by the decoder), so a live print and the capture's replay
  are the same text; SIGINT and SIGTERM end a flagged command or `ww monitor` / `capture` / `stats` with the
  capture finished, synced and renamed; `ww explain` of a path-like WHAT that is no file says so without
  connecting.
- 🧪 **TOOL-21 `ww pins tui`** · `v0.5.x` — every point live, laid out per connector of a board preset
  (`docs/cli_reference.md#ww-pins-tui`): level and what was set on each pad, keys to drive, toggle, pull, weak
  drive, read all, release, pause; every point released on quit, signals and errors. The preset is boardmap's board
  description read as it is (`[[view.connector]]`, `[aliases]`, `[pins]` roles, pads from its geometry JSON, nets
  joined across fitted solder bridges), no dependency on embedded_bedrock; a preset for ww alone lists nets per
  connector. Devices with `ww_gpio` banks and no `PinDiag` are shown through the banks (modes and pulls put back on
  release). Library `ww_pin_probe::board`, `live`, `bank`; tests on `MockBoard` with the NUCLEO-G0B1RE preset
  (`wire_weaver_cli` `cmd::pins_tui`) and over banks end to end (`tests/io_gpio`). Hardware check open: the Nucleo
  G0B1RE once its firmware serves `PinDiag`. Skill tree signal and skill: tpm P2618#23oct-1334.
- 🚧 **TOOL-6 Dev GUI / web tool** · `v0.5.x` — `wire_weaver_tool` (egui, Trunk) shows the AST and generated code. Goals:
  a web tool like the pest playground, a dynamic UI for any API (MVP prototyped), widgets per type, mockup
  server UI. See `docs/dev_tool.md`. A `Fixed` field's widget shows its unit and takes its slider limits from
  `quantity::fixed_of(..).storage.bounds` (CLT-54).
- 💡 **TOOL-7 Generated Python wheel for a specific API** — nothing is generated for Python yet:
  `examples/blinky_py` is a hand-written pyo3 wrapper around the Rust client, and `wire_weaver_py` is dynamic.
  Generating typed pyo3 bindings from an API crate might be useful (see the `TODO` in
  `docs/api/folder_structure.md`).
- 🚧 **TOOL-17 `ww hub`** · `v0.5.x` — `ww hub serve | status | stop | who | keep` (`docs/cli_reference.md#ww-hub`,
  `docs/transport/hub.md`), `Hub::status()` and the location, label and API hash in `Registry::devices()`; `who`
  names the hub, direct ww programs (lock files) and, on Linux, other programs (`/proc`) per device, with or without
  a hub; `ww list` asks a running hub through the registry (with a `STATE` column, `--direct` to enumerate USB
  anyway); completions of the subcommands, `keep` durations and device keys with help texts; colour off without a
  TTY or with `NO_COLOR`; `lend DEVICE -- CMD` (SRV-32). Open: `ww list` asks a running hub and doesn't start one; the skill tree entry
  (P2618#21oct-2209).
- 📋 **TOOL-13 `Some(None)` in Python values** — Python (and the conformance corpus, which uses its form) writes
  `None` for both `None` and `Some(None)` of an `Option<Option<T>>`, so the inner `None` can't be written or told
  apart when read. Needs a marker for a present `None` (e.g. `ww.Some(None)`). Conformance case
  `option.option.some_none`.
- 📋 **TOOL-8 `ww` inspect / read / write all GPIOs on any device** · `v0.5.x`.
- 🚧 **TOOL-9 Buffer visualizer / "explain" encoding** · `v0.5.x` — each bit traced to the value it belongs to.
  Done: `ww_self::explain` (the dynamic decoder records every value, length, discriminant, flag and padding with
  its front bits and back nibbles); `ww_self::expr` (feature `expr`: values and calls written as Rust expressions,
  `Config { mode: Mode::Fast, .. }`, `motor.set_speed(1.5, Dir::Cw)`, `motor.limit = 3`, read by the type with
  errors naming the field) and `wire_weaver_client::expr` (the same with `ww_si` quantities, `q!(3300 mV)`);
  `wire_weaver_client::envelope` (a whole request or event: the `ww_client_server` envelope field by field, the
  payload by its resource); `ww explain WHAT HEX | --value EXPR | 'call(..)' [--request]`, `ww explain request|event
  HEX`. Captures and the live link: `ww explain FILE` / `ww monitor --layer api --bits` (TOOL-14), replies explained by
  their request's seq; `Observe` requests and their grants read as property names and modes, `PropertyUpdate`s as the
  properties they carry with typed values (units as in `ww observe`); calls through global traits (`GlobalFull`
  paths: `ww info`, `ww report`, ..) are named `ww_user_info::UserInfo.name` and decoded from the snapshots the host
  carries, also by a command that never introspects the device. Open: `GlobalCompact` paths, multi requests; `Series` columns and sparse-struct slot headers in detail; shell completion of types and paths; a
  `ww call` on the same expressions; a GUI view (hex pane + tree, like Kaitai's Web IDE); Python `explain()` and
  expression input.
- 📋 **TOOL-22 Hint for plain fields that carry a unit** · `v0.5` — `ww api check` notes a plain numeric field,
  argument or property whose name (`*_us`, `*_ms`, `*_bytes`, `*_ppb`, ..) or doc comment ("in µs", "bytes",
  "0..=65535 = 0..100 %", "127 = not available") states a unit, scale, range or sentinel, with the `Fixed` (STD-55),
  `Bounds` or niche (STD-66) form it could be: no meaning left in comments. A note, never an error, silenced per item;
  tested on the scout's repos.

- ✅ **STD-1 `ww_client_server`** — the request/event model, pruned of sideband/subscribe/rate, variable length seq
  numbers.
- ✅ **STD-2 `ww_version`**.
- ✅ **STD-3 `ww_si`** and **`ww_numeric`**.
- ✅ **STD-4 `ww_date_time`**.
- ✅ **STD-5 `ww_gpio`**.
- ✅ **STD-16 Common errors** — `ww_error::CommonError` (`Unsupported`, `InvalidArgument`, `InUse`, `Busy`,
  `WrongState`, `NoPower`, `Refused`, `Timeout`, `Fault`, bounded strings), re-exported as
  `wire_weaver::prelude::CommonError`, nested first in an error enum as `#[common] Common(CommonError)` (the derive
  adds `From<CommonError>` and `HasCommon`); recognised by tools by its type (`ww_self::TypeOwned::is_common_error`):
  Python `RemoteError.common` with the doc line as the message, `ww api tree -t` marks it. Display by hand until
  DER-12 takes over. Every `ww_stdlib` API crate with fallible methods uses it except `ww_dfu` (its `Error` has no
  lifetime and predates this; only `Unsupported`/`Busy` would map cleanly, left as a follow-up) and `ww_can_bus`
  (not ready yet); `ww_client_server`'s `ErrorKind` is a framework/link-level error, not an API error, so it is out
  of scope.
  `docs/std_library/ww_error.md`, `tests/common_error`.
- ✅ **STD-19 `ww_pwm`** — `Pwm` / `PwmChannel` outputs (`set_output`, `set_duty`, `stop`, `state`), `PwmCapture` /
  `PwmCaptureChannel` measurements (frequency, duty, pulse width, edge count) over a plain stream with sideband
  `Open` / `Close`. Full rustdoc, mock server and in-process tests (`tests/io_pwm`), docs page
  (`docs/std_library/ww_pwm.md`) (STD-13).
- ✅ **STD-17 `ww_timebase`** — `Clock`: device time as `ww_si::Second` (full width via `now()`, truncated
  elsewhere), sync master / slave (IRIG-B, PPS), `sync_status`, a `time_map` stream mapping device time to
  shared time, `set_time_of_day` for a master. `tests/io_timebase`, `docs/std_library/ww_timebase.md`.
- ✅ **STD-49 `ww_timebase` host sync, one UTC timescale** — devices without a sync line follow the host's wall
  clock (UTC), and every source is ranked: a locked line (IRIG-B, PPS) > host > holdover > free.
  `Clock` gets the optional `host_sync` group (`steer(SteerPoint) -> SyncSource`: offset in UTC µs, rate in ppb and
  the host's error estimate; a line-locked device only notes the point; `shared_now()`; `set_sources` / `sources`: an
  explicit ordered list of allowed sources, `None` keeps the fixed ranking), `SyncSource` (`Host`,
  `Line`), `SyncStatus::source` / `error`, `TimeMapPoint::source` (the seam of a switch, slewed, not stepped),
  `SyncState::Conflict` (two masters on a line), `SyncStatus::slew_us`; `Acquiring` until within the device's lock
  tolerance, then `Locked` (`docs/std_library/ww_timebase.md#states`), the host steering at 10 Hz until then. `configure_sync` stays the line's role, the IRIG-B / PPS model is
  unchanged. Host side: crate `ww_clock_sync` (`Estimator`: min-RTT samples of `now()`, least squares over a 30 s
  window against the monotonic clock; `DeviceTime`; `HostClock`; `ClockSync` for any process holding a connection),
  deterministic tests with simulated device clocks (HSI 1 % fast, a drifting crystal, USB FS jitter), end to end in
  `tests/io_timebase`; `docs/std_library/ww_timebase.md#host-sync-std-49`. Device side: embedded_bedrock
  `bedrock_sync`. Steer points name their steerer and a device follows one at a time, with a per-device rate cache on
  the PC (SRV-31). Checked on omarchy-m1 (21-22 Oct 2026): Donguru, rePort and B135 steered by `ww sync`,
  `sync_status` `Locked` in 1.2-1.6 s, the residual measured with `shared_now` B135 -5..-16 µs (±55 µs), Donguru
  within ±1.2 ms (its 1.4-1.6 ms round trips).
- ✅ **STD-18 `ww_adc`** — `Adc`, `AdcStream` (sideband `Open` / `Close`), `AdcCapture`. Fill-in done:
  full rustdoc, `tests/io_adc` mock server (one test per trait), `docs/std_library/ww_adc.md`, API snapshot
  (STD-13).
- ✅ **STD-20 `ww_onewire`** — `OneWire`: addressed transactions with strong pull-up, reset, paged and
  bit-level (`triplet`) ROM search, the real Dallas/Maxim search algorithm. Full rustdoc, docs page
  (`docs/std_library/ww_onewire.md`), API snapshot, in-process test with a mock bus (`tests/io_onewire`).
- ✅ **STD-23 `ww_si` / `ww_numeric` extended** · `v0.5` — quantity groups and host helpers as cargo features,
  `Second` as a device timestamp (wrap rule, host `Unwrapper`), constructors, conversions and comparisons across
  prefixes and storage, `UVlq32` / `UVlq64` storage, `Baud` for baud rates (STD-13), `Ratio`, `Torque` and
  `Timed<T>` (EX-11). `docs/std_library/ww_si.md`,
  `docs/std_library/ww_numeric.md`. Zero-overhead typed quantities (`Volt<u16, Milli>`) are not part of it.
- ✅ **STD-38 `ww_si` 0.2: one generic `Quantity<U: Unit>`** · `v0.5` — every quantity is
  `Quantity<U> { prefix, value, PhantomData<U> }` with a zero-size marker per unit in `ww_si::units` and the 0.1 names
  as aliases (`KiloGram` → `Gram`); `Dim`, `Scale`, `UnitExpr`, `Unit` (own markers in API crates), `Per` / `Times`;
  constructors, `quantize`, conversions (`to_si_f64` through each unit's scale), `with_prefix`, `same_repr`,
  comparisons, `Display` / `Debug` / `defmt` written once; unit algebra checked by the compiler against a product
  table verified at compile time; `units::by_name`. Same bytes on the wire as 0.1. `docs/std_library/ww_si.md`,
  `ww_stdlib/ww_si/tests`; the 0.2.0 snapshot holds `Quantity`, every marker and `Per` / `Times`, so devices
  leave them out of their introspection data. Other crates' call sites moved to the new constructors; the motor demo's
  own helpers (`units.rs`) and `Load` units are the follow-up piece; `BaseUnit` is replaced by STD-37 (run-time units).
- ✅ **STD-37 `ww_si` units known only at run time** · `v0.5` — `SIExpr<'i>` { `Named { name, symbol, dim, scale }`,
  `Mul`, `Div`, `Pow` } (recursive through `RefBox`, API-44) and `AnyQuantity<'i> { prefix, value, unit }`: the
  marker model on the wire, `Dim` as seven 4-bit exponents (`sized`), `Scale` too. A marker's `UnitExpr` maps one to
  one onto an `SIExpr` (`Unit::SI_EXPR`, `SIExpr::of`, `SIExprOwned::from`; back `==` and `known()`), shown, named
  and converted to SI by the same code as typed units, walks bounded by `MAX_UNIT_DEPTH`. `ww_adc::ChannelInfo::unit`
  is one; `BaseUnit`, `SIExp`, `INib8P3` are removed. `docs/std_library/ww_si.md#units-known-only-at-run-time` (byte
  layouts), `ww_stdlib/ww_si/tests/dynamic.rs`, `tests/io_adc`. Dynamic clients, `ww` and Python show an
  `AnyQuantity` like the typed quantity of its unit (CLT-31's `DynQuantity`, both rendered by the public `ww_si::tree`
  walk), a device's own unit with its dimension and scale; writes take any quantity whose unit has a dimension.
  `tests/quantities` (an `AnyQuantity` property through the in-process transport), `wire_weaver_py/tests`.
- ✅ **STD-43 `ww_si` without wide integers on small targets** · `v0.5` — comparisons, `with_prefix`, the exact
  arithmetic and the integer conversions of quantities work on `ww_numeric::Integer` (sign and a magnitude as wide as
  the widest enabled number type): with types up to 32 bits only `u32` code, no `i64` / `i128` and no `f64` without
  float types. `to_i32_in` / `to_u32_in` / `to_u32_wrapping_in`; `to_i128_in` only with 64- or 128-bit types. 6 KiB
  less flash on Donguru (thumbv6m, `opt-level = "z"`). `docs/std_library/ww_si.md#conversions-and-comparisons`,
  `ww_stdlib/ww_si/tests/narrow.rs` and `ww_stdlib/ww_numeric/tests/integer.rs` (run with 32-bit types only too).
- ✅ **STD-61 `exact_units`: the compile-time unit core as a crate of its own** · `v0.5` — a standalone `no_std`
  crate, no dependencies without features (Roman, 27 Oct 2026): `Dim` (SI, rad, bit, counted bases, root flag), unit
  markers and `Unit` (with `Sqrt<U>`), `ExactScale` (with a power of π), `Fixed`, bounds, const `sqrt_f64`;
  `shrink_wrap` (wire format, `SIExpr`, through `#[wire(..)]` fields the same bytes as before), `std` and `defmt`
  behind features. `ww_si` re-exports it and keeps the runtime side (`Quantity`, `AnyQuantity`, `Timed`); units are
  named `exact_units::..` in the model (a counted base costs 6 bytes more on the wire). Walked and snapshotted like an
  API crate. [exact_units](docs/std_library/exact_units.md), `exact_units/tests`, CI legs without features and on
  thumbv6m.
- ✅ **STD-55 `exact_units::Fixed<U, P, T, B = Unbounded>`: unit, scale and bounds fixed at compile time** · `v0.5` —
  a value of unit `U` at scale `P` stored as `T`, on the wire exactly `T` (DER-18 `transparent`; no prefix, no
  `NumericValue` tag, so a `u64` doesn't turn on `ww_numeric`'s `u64` for everyone); scale markers (every prefix
  name, `Kibi`.., `Pow10<E>`, `Rational<N, D>`), bounds (`Unbounded`, `Range`, `RangeU64`, `RangeDec`) checked on
  decode and construction; information and counting units, exact `to`, `Quantity::to_fixed` / `from_fixed`,
  `core::time::Duration`, checked / wrapping / saturating ops, `Display` (`1500 µs`); in the API model as an instance
  of one generic definition with `B` explicit, so introspection shows the unit. Same code as a plain field on
  Cortex-M (DER-20). [exact_units](docs/std_library/exact_units.md), conformance `stdlib.si.fixed.*`,
  `tests/quantities`.
- ✅ **STD-72 Imperial and US customary units, complete** · `v0.5` — feature `customary` (in `ww_si`'s `quantities`):
  166 markers in `exact_units::units`, each the exact rational of SI from its legal definition (1959 yard and pound,
  NIST SP 811 and Handbook 44, UK WMA 1985, g_n), the source cited per unit and checked against a sourced catalogue
  with SP 811's printed values. No bare ambiguous name (47 groups: US / imperial gallon, fl oz, pint, quart, bushel;
  international / US survey foot and mile; short / long / metric ton; avoirdupois / troy; Btu and calorie variants;
  horsepower variants; thou vs angular mil; inHg / mmHg conventional or at a temperature); `PoundAvdp` and `Slug`
  mass, `PoundForce`, `Kip`, `Poundal` force; °F and °R are origins (STD-64). Measured units carry
  `Provenance::Conventional`. `Fixed::convert` between units of one dimension, exact or `None`, factor at compile
  time; `DynQuantity::convert` and Python `Quantity.convert` the same at run time. The Mars Climate Orbiter case
  (lbf·s read as N·s) in `tests/customary` (typed, dynamic) and Python: converts exactly or is refused; the `Fixed`
  field too, with its unit in the dynamic client (CLT-54). [Imperial and US customary](docs/std_library/customary.md),
  conformance `stdlib.si.fixed.customary`, `stdlib.si.si_expr.pound_force` (derive, dynamic, Python; C waits on
  API-26).
- ✅ **STD-58 `Index`, `IndexFrom1` and `Seq`** · `v0.5` — `exact_units` (feature `counting`, re-exported by
  `ww_si`): `Index<Of, T, B>`, a 0-based position in a table of `Of` (index ± amount = index, index − index = a
  `Fixed<Of, One, T>` distance, index + index and index · x don't compile, checked forms for bounded ones);
  `IndexFrom1<Of, T, B>`, 1-based as people count, refuses 0 on construction and decode, `to_index` / `from_index`;
  `Seq<Of, T>`, a wrapping sequence number (`next`, `wrapping_add`, `distance` as a `Wrapping` amount, RFC 1982
  `is_before` / `serial_cmp` within half the range, no `Ord`, no `Seq + Seq`). All `transparent`, the bytes of `T`; in
  the model instances of `Index` / `IndexFrom1` `[Of, T, B]` and `Seq` `[Of, T]`, shown and checked by clients
  (CLT-54). [Indices and sequence numbers](docs/std_library/indices.md), conformance `stdlib.si.index.*`,
  `stdlib.si.index_from_1.*`, `stdlib.si.seq.*` (derive, dynamic, Python; C waits on API-26).
- ✅ **STD-64 Points and origins** · `v0.5` — `exact_units::Point<O, Fixed<..>>`: a position against an origin, a
  zero-size marker declared with `origin!` carrying its unit, an exact `Offset` (sign and `Magnitude`), its reference
  and an optional modulus. Point ± vector = point, point − point = vector, point + point and point · x don't compile.
  `origins`: `AbsoluteZero`, `Celsius` (273.15), `Rankine`, `Fahrenheit` (45967/180 K, steps of 5/9 K), `Vacuum`,
  `Gauge` (reference: ambient pressure, converts only with `against` at run time), `AboveStandardAtmosphere`,
  `Angle<Turn | Degree | Radian>` (wrap every turn, the difference signed and shortest). Register formulas are origins
  `above` another (LSM6DS `raw/256 + 25 °C`, J1939 −40 °C). `to::<O2, P2, T2>()` between origins of the same unit and
  reference, its factor and offset at compile time, exact or `None`. On the wire a point is its number; `OriginDef`
  holds an origin as data for INT-13. [Points and origins](docs/std_library/points.md), conformance
  `stdlib.si.point.*`, `stdlib.si.offset.*` (derive, dynamic, Python; C waits on API-26).
- ✅ **STD-62 One exact magnitude for every scale** · `v0.5` — `exact_units::Magnitude`, a canonical product of prime
  powers (2 and 5 as powers of 10 and 2) with powers of π, ln 2 and ln 10, whole or half exponents: every scale
  (prefixes, `Pow10<E>`, `Pow2<E>`, `Rational<N, D>` with `u64` parameters factored at compile time, `Kibi`..,
  `FullScale<V, BITS>`, `FullScaleMax<V, BITS>`) is one, equal however it is written (`Milli` = `Pow10<-3>` =
  `Rational<1, 1000>`); closed under `*`, `/`, powers and roots, a product that needs more than 12 primes or leaves
  ±63½ is a compile error. `Fixed::to` computes its factor at compile time: not a fraction of integers, or a multiply
  that doesn't fit 64 bits (storages up to 32 bits) / 128 bits, doesn't compile, never a rounding. Q formats `Q15`,
  `Q31`, `UQ16_16`, binary angles `Bam16`, `Bam32`. Replaces `ExactScale` in `Scale::Exact` (104 bytes in memory, a
  decimal prefix 2 bytes on the wire). [Exact magnitudes](docs/std_library/magnitude.md), conformance
  `stdlib.si.magnitude.*` (derive, dynamic, Python; C waits on API-26).
- ✅ **STD-63 Named rounding and overflow policies** · `v0.5` — `to()` stays exact or `None`; lossy conversions name
  their rounding (`Rounding`: truncate, floor, ceil, nearest ties-to-even, nearest ties-away) and overflow policy
  (`Overflow`: checked, saturate, wrap): `Fixed::to_rounded`, `to_saturating`, `to_wrapping`, `requantize`, exact (the
  true quotient rounded once), π/ln factors through a 64-bit fixed point (`Magnitude::fixed_point`). Arithmetic's
  policy is in the bounds marker: `Wrapping` (`Bam16`, `Bam32`), `Saturating` (`Q15`, `Q31`, `UQ16_16`), `Unbounded`
  checked; same bytes, `Fixed::of` / `with_overflow`. Bounds prove conversions infallible: `into_rounded`, `From` of a
  `Range`-bounded value for a whole factor, a compile error where the proof fails. Run-time `requantize` /
  `Requantizer` for dynamic clients. [Rounding and overflow](docs/std_library/rounding.md),
  `exact_units/tests/rounding.rs`, conformance `requantize.*` (derive, dynamic, Python `ww.requantize`, C `sw_requantize()`).
  `Seq` wrapping comes with STD-58.
- ✅ **STD-60 Angle, information and counted things as dimensions** · `v0.5` — `Dim<'i>` has rad and bit as
  base dimensions and up to four counted bases known by crate and marker name (`Item`, `Sample`, `Frame`, `Packet`,
  `Cycle`, `Block`, `Sector`, `Chunk`, `Char`, `Code`, feature `counting`, an API's own with `ww_si::counted!`;
  `Bit`, `Byte`, `Word`, feature `information`) and a root flag for half exponents (V/√Hz, `Dim::sqrt`): bytes don't
  convert into % or rad, `AngularVelocity` isn't `Hertz`, `Per<Sample, Second> × Second` is a `Sample`, bytes of a
  sample only through `Per<Byte, Sample>`; checked at compile time (`Unit::DIM`) and at run time (`SIExpr`,
  `wire_weaver_client::DynUnit`, Python `Quantity.dim`). `Torque` is J/rad (written N·m). Wire change EVO-16.
  [ww_si dimensions](docs/std_library/ww_si.md#dimensions), `ww_stdlib/ww_si/tests/quantities.rs`.
- ✅ **STD-24 `ww_firmware_info` / `ww_board_info` / `ww_uid` / `ww_user_info` device contract** · `v0.5` — the info
    crates every device implements. `FirmwareInfo` const items `FIRMWARE` (name, version, git, build time, profile),
    `BUILD_ID` (GNU build id, finds the ELF) and optional `BUILD_RECORD`; `build::emit()` + `firmware_summary!()` for
    `build.rs`, `build_id()` reader; `LastReset` (`ResetRecord` with a `Fault` per architecture behind `cortex-m` /
    `riscv`, stack words, `boot_count`, `clear`). `BoardInfo::BOARD` with `RevisionSource` (`Record`, `Strap`, `Assumed`, `Provisional`) and optional `REWORKS`
    (`Rework { id, title, applied }`, up to 16; `ww info` prints them). `Uid::IDS`, first id is
    the primary one (USB serial). `UserInfo` with bounded `name` / optional `notes`, `UserInfoError`, `CAPABILITIES`
    and the persistence rule. Docs `docs/std_library/ww_{firmware_info,board_info,uid,user_info}.md`, tests
    `tests/{firmware_info,board_info,uid,user_info}`. The const items are served by `get_` handlers until constant
    items come from the introspection data; properties cannot carry `#[errors]` yet.
- 🧪 **STD-26 `ww_log` defmt over reliable streams** · `v0.5` — `ww_log_bare_metal` renamed `ww_log`.
    `LogDefmt { const DEFMT (encoding, build id); frames() }`: a reliable stream whose unit is one `defmt` frame,
    whole frames per item (up to 512 B), so `lost` is the exact number of dropped messages and logs from before the
    host connects arrive by resuming the device's session. `LogText { records(); rw level: LevelFilter }` with
    bounded `LogRecord`, `Severity` without `User`, `LogError` on `CommonError`. Device side: new crate
    `ww_defmt_logger` (`global_logger!`, lock-free ring with whole-frame commits, RTT tee that never blocks (whole
    frames or skipped and counted, whatever mode a probe set), `pump()` into the store,
    drops reported in place), ring stress-tested on the host with concurrent producers. `tests/log_defmt` decodes
    with `defmt-decoder` end to end; `examples/defmt_log` (host) and `examples_mcu/rp2`'s `rp2_usb_log` (USB and RTT
    at once, build id in flash). `docs/std_library/ww_log.md`. Tested 16 Oct 2026 on Donguru B153A (STM32G0B1,
    typec_dongle API-10, with a `PersistentRamRing`): logs over USB and RTT at once, `lost` exact under a flood of
    1000 (227 + 773), logs from before connecting and from before a forced HardFault; the non-blocking RTT tee
    with probe-rs attached (it sets the channel to blocking): floods of 1000 go on at ~62 us a line instead of
    ~620 us, RTT shows what fits. Known gap: frames dropped at the end of a burst are reported (`lost`) only with
    the next frame the device logs, so a quiet device shows the loss late. `rp2_usb_log` not run (no Pico 2 probe
    on the mesh). `ww log` in the CLI is a later item.
- ✅ **STD-25 `ww_counters` on `cnt` words** · `v0.5` — the device's `cnt` counters as raw buffer words, decoded on
    the host with the firmware's ELF found by its build id: `Counters::INFO` (format, used/capacity words of RAM and
    backup, build id), `read(buffer, offset)` pages of `UVlq32`, `reset`, optional `watch` stream (not served by
    anything yet); `read_words` / `reset_words` serve `cnt` buffers, feature `host` has `WwSource` (a
    `cnt_core::Source`). `ww counters read|watch|tui|reset|info` shows the `cnt_view` views (CLT-10). In the `cnt`
    repo: `cnt::ram_used_words()`, word sources and ELF lookup by build id in `cnt_core`, `cnt_view`, and the probe
    path checks the build id too. Docs `docs/std_library/ww_counters.md`, tests `tests/counters`; `cnt_core` /
    `cnt_view` 0.5 from crates.io.
- ✅ **STD-27 `reset-record-age`** · `v0.5` — `ww_firmware_info::ResetRecord::boots_since_crash`: boots since the
    crash data was saved (0 = the last reset was the crash), so a record kept across later normal resets shows how old
    it is. `bedrock_crash` counts it (`Crash::boots_since`) and the firmware template fills it.
- ✅ **STD-28 `ww_indication` lightweight rework** — cut to one required item: `mode: Mode` (`AllOff` /
    `OnlyEssential` / `Normal`). `brightness: Brightness` (5 levels, one setting for all indicators),
    `identify(duration)` (blink everything to find the unit on a bench, errors on `CommonError`) and `NAMES`
    (indicator names, API-36) are each their own `#[optional]` item. Dropped `alert_brightness` /
    `AlertBrightness`, `animation_enable`, `test_mode` / `TestMode` and `HAS_NOT_CONTROLLABLE`. Docs
    `docs/std_library/ww_indication.md`, tests `tests/indication` (mock servers with only `mode`, and with every
    optional item).
- 🧪 **STD-50 `ww_indication` animations in phase** — a device with a shared clock (STD-49 or a sync line) aligns
  its periodic animations to period boundaries of shared time (UTC, WSPR-style: `t mod P`), so devices showing the
  same animation are in phase with no epoch passed around, across PCs to NTP precision. A rule in the trait's docs,
  no API item (an explicit epoch was dropped: every server would have had to implement or omit it).
  `docs/std_library/ww_indication.md#animations-in-phase-std-50`; device side embedded_bedrock `bedrock_led` with
  `bedrock_sync`. Hardware check open: Donguru and rePort breathing in phase.
- ✅ **STD-29 `ww_can_bus` on the stdlib idioms** — from simple bridges to bus debuggers. `Can` is one controller:
  six required items (`capabilities()`, `configure(CanConfig)` answering what was set, `config()`, the observable
  `status`, the counted `rx` stream, the `tx` credit sink) and `#[optional]` `set_mode` (incl. `Config` and
  `Sleep`), `recover`, `tx_abort`, `tx_events` (marker, measured queue time), `bus_errors`, `protocol_status`,
  `bus_load`, `detect_timing`, `set_filters`, `set_layout`, `rx_buffer`, `tx_buffer` (once or cyclic),
  transceiver (standby / silent / sleep, wiring faults), terminator, `test_pins`. `CanGroup` (`interface[]: Can`,
  enable / disable, optional clock source and frequency, performance) for controllers sharing clock, power and
  message RAM; `CanTimeTriggered` (TTCAN); `CanXl` behind the `xl` feature. Errors on `CommonError` +
  `OutOfFilters`, `OutOfMemory`, `BusOff`; `defmt` no longer pulls `std`. Docs `docs/std_library/ww_can_bus.md`,
  tests `tests/io_can` (full and minimal node), `tests/io_can_group` (group, TTCAN, XL).
- ✅ **STD-42 `ww_tape`** · `v0.5` — tape blocks, the generic unit of track records for live feeds and log files
  (`ww_tape`, `docs/transport/tap.md#tape-blocks`): a 32 B header, varint records (track, time delta, length,
  item), CRC-32C, never padded; describe blocks with the producer, `TrackDef`s (stable key, API, resource path,
  sticky) and API bundles; `TapeWriter` writes them with database-style durability (`.partial` then rename, file and directory synced, a `SyncPolicy`, a failed write or sync fatal, never retried), `FileBlocks` reads the longest valid prefix (or skips damage, `Recovery::Resync`); tests cut and flip every byte, fuzz target `tape-reader`. Blocks of a later version or an unknown kind are skipped by length, `Describe` is evolvable. Open: splitting into several files by size or time, preallocation (zeros at the end already read as the end). Nothing in it
  knows a producer: the link tap (TR-14) writes it; it is also eventfull's own file format (CORE-19).
- 🧪 **STD-32 `ww_settings` settings store** · `v0.5` — a device's settings in its own NOR flash: `#[derive(Settings)]`
  on a plain struct (`#[setting(slot = N, default = ..)]`, `#[settings(retired(..))]`), stored as a record of the
  fields not at their default, each a byte blob in an `OptionAnyVec`, so firmware versions skip and keep each other's
  slots; `Settings<G, N>` with write-on-change, a debounce and a durable `persist`, factory reset per group (`Key`);
  served through `value_on_changed` properties. Its own `Store` over an `embedded-storage-async` `NorFlash` (per-key
  seq in every item, page headers with crc, current records copied off a page before it is erased, loads never
  write, `maintain()` to erase ahead): power-cut safe at every word written and byte erased in any erase order, which
  `sequential-storage` 8 isn't. Docs `docs/api/settings.md` (pages, erase stalls on G0 / F4 / H7, big-sector
  options) and `docs/evolution/rules.md#settings-stored-on-a-device`, tests `ww_settings/tests` (`just
  test-power-cut`). Hardware check open: real power cuts on a G0B1 (TEST-21), dump decoder (TOOL-12), demo (EX).
- 🧪 **STD-35 Slave data paths in `ww_i2c` and `ww_spi`** · `v0.5.x` — an `#[optional(slave)]` group in `I2c` and
  in `Spi`: opening the counted `slave_events` / `slave_frames` stream with a config starts slave mode (closing stops
  it) and reports every transfer (bytes both ways, register pointer or address, fill bytes, stretch time); the
  answers come from a register map the host fills and reads back (`slave_map_write` / `slave_map_read`, applied
  between two transfers) or from the `slave_answers` credit sink, one answer per read or frame. A host that is too
  slow: I2C `I2cSlaveUnderrun` (`Fill`, `Stretch { max }` with `ReadWaiting` sent at once, `Nack` while empty), SPI
  the idle byte or the last answer again, flagged `underrun`. Limits of clock stretching (SMBus 25 ms, masters
  without it) in the crate docs and `docs/std_library/ww_i2c.md#slave-mode`. `I2cMode::Slave`, `SpiRole::Slave` and
  the `slave` capability flags are gone. API, snapshots, docs, the device-side engine (SRV-26, `ww_bus_slave`) and an
  end-to-end test of both slaves done: `tests/io_i2c` / `tests/io_spi` drive `I2cSlave` / `SpiSlave` byte by byte as a
  simulated master (test-only `slave_bus` / `slave_pump` methods, a fake clock) through the real host and device event
  loops: register maps with page wrap and host updates between transactions, the answer queues, stretching to the
  deadline and past it, `Nack`, `repeat_last`, cleared answers and dropped events. Open: a check on real silicon, with
  the first user, the rePort (usb_io_fw IF-5, v1.0 launch, Roman 16 Oct 2026).
- 🧪 **STD-52 `ww_i2c` master errors and limits** · `v0.5` — what a good I2C master knows reaches the host (found
  by the rePort I2C driver, P2424 i2c-master-driver): `I2cError::Nack { op, at }` (which operation, and the address
  or which data byte, counted as `I2cRecord::nack_at`), `ClockStretch` with `I2cMode::Master`'s `stretch_timeout`
  (set and answered by `i2c_configure`, range in the capabilities), `BusStuck { scl_low, sda_low }` (no silent recovery:
  `i2c_wiggle_clock` on request, at the bus speed up to 100 kHz), and the limits in `I2cCapabilities` (`speeds`,
  `max_ops`, `max_write_read_len`, `zero_length_write`, `stretch_timeout` range; grouped by topic, sizes all `u16`). Transactions are stated as
  a repeated start before every operation (Linux `I2C_RDWR`), not embedded-hal merging; `i2c_configure` answers the
  highest SCL not above the request. Breaking on the wire and in Rust (nothing released or deployed, user 27 Oct
  2026): callers in usb_io_fw and typec_dongle adapt. Docs
  `docs/std_library/ww_i2c.md#transactions`, tests in `ww_i2c` (round trips) and `tests/io_i2c`. Open:
  the check on real silicon with the rePort driver (usb_io_fw IF-2, RP2350 DW_apb_i2c).
- 🧪 **STD-76 `ww_i2c_hl` embedded-hal I2C over `ww_i2c`** · `v0.5` — a stock embedded-hal 1.0 I2C driver,
    blocking or async, runs unchanged on the host against a remote `ww_i2c::I2c` (for P2534 fusb302-sink, a PD sink
    driven from the PC): `I2cMaster` merges contiguous same-direction operations (a repeated start before every
    `ww_i2c` operation, STD-52), so one embedded-hal transaction is one call and one round trip; refuses, never
    splits, what the device's `I2cCapabilities` don't take; `Nack`/`Arbitration`/`Bus`/`Overrun` mapped to
    `ErrorKind`; request priority as a handle setting (`urgent()`, CLT-50). Docs `docs/std_library/ww_i2c_hl.md`,
    tests `tests/io_i2c` (`tmp1x2` async, `eeprom24x` blocking, unchanged). Open: the check on real silicon with
    the FUSB302 over the rePort's I2C (P2534).
- 🚧 **STD-6 `ww_gpio_hl`** — usable, some APIs are still missing. Blocking, async and promise flavors; the blocking
    pins implement `embedded_hal::digital` (`OutputPin`, `StatefulOutputPin`, `InputPin`), the async ones
    `embedded_hal_async::digital::Wait` without a race (stream opened, then level read, then edge awaited) and keep
    the edge's device time (`last_edge_time`), tested in `tests/io_gpio` (`docs/std_library/ww_gpio.md#embedded-hal-ww_gpio_hl`).
- 🔍 **STD-7 Interrupt left enabled in `ww_gpio_hl`** · `v0.5` — the falling edge helper documents that it leaves
  interrupts enabled; decide whether that is the intended fix or a bug.
- 🚧 **STD-8 Prototypes**, to be completed later: `ww_i2c`, `ww_uart`,
  `ww_memory` (was `ww_ll`: one word type per trait, optional watches, errors on `CommonError`, docs page),
  Known gaps are `TODO`s in their sources (e.g. SMBus and 7-bit addresses in `ww_i2c`; `ww_can_bus` is done in
  STD-29). `ww_log` (was `ww_log_bare_metal`) left for STD-26.
- 💡 **STD-9 Stubs**, for later: `ww_spi` (`ww_board_info`, `ww_firmware_info`, `ww_user_info` and `ww_uid` moved to STD-24).
- 🚧 **STD-12 `ww_dfu` firmware update API** — traits `UpdateNode`, `BootControl`, `Inventory`, `Stage`,
  `StageBulk`: each node (app, bootloader, recovery app) implements what it needs; A/B live, in place from a stored
  plan, in place live; step by step or bulk. Carries the octet records of the chunky protocol spec (P2625). End to
  end test `tests/dfu`. Guide with the eight update scenarios (`2C-APP-STEP` Swap Shuttle .. `1L-UPD-BULK` Pit-stop
  Freight), each tested end to end over USB (simulated), WebSocket or UDP with reboots and power cuts, its exchange
  recorded as a golden transcript the docs include (`docs/std_library/ww_dfu.md`, `tests/dfu_scenarios`,
  `just dfu-transcripts`). `StageBulk::plan` is a credit sink (API-28 credit-sinks). Every variable-size field is bounded (SW-19:
  `MAX_PIECE_LEN`, `MAX_HEADER_LEN`, `MAX_CHUNKS_PAGE`, `MAX_PRODUCT_LEN`), so every method has a known largest
  request and reply. `Profile::AB(SlotSwitch::{Copy, Remap})` / `InPlace`. Next: a chunky implementation (chunky
  NET-1).
- 🚧 **STD-13 IO trait family** · `v0.5` — `ww_gpio`, `ww_uart` (optional `Uart::monitor`), `ww_i2c` (+ `I2cWatch`,
  `I2cMonitor`, 10-bit addresses), `ww_spi` (optional `Spi::monitor`), `ww_adc` (STD-18), `ww_pwm` (STD-19),
  `ww_onewire` (STD-20), `ww_timebase` (STD-17), `ww_logic_analyzer` / `ww_signal_generator` (STD-21, STD-22);
  `ww_si` values and timestamps (STD-23): frequencies `Hertz`, baud rates `Baud`. Family overview in
  `docs/std_library/io_family.md`; every crate filled in with full rustdoc, a mock-server test pair `tests/io_<x>`
  and a docs page `docs/std_library/ww_<x>.md`; `examples_mcu` GPIO and UART boards follow. A bus or bank is
  configured with one `configure(XConfig)` answering what was actually set, the IO level inside it
  (`io_level: Option<Volt>`, capabilities `io_levels` / `io_level_min` / `io_level_max`). Every crate's error enum
  starts with `#[common] Common(CommonError)` (STD-16) and keeps only what is specific (`Nack`, `NoPresence`,
  `NoSuchChannel`, ...), methods list their variants with `#[errors(..)]` (API-32). Streams with a configuration are
  counted stream functions (API-31: `AdcStream::samples`, `LogicStream::data`, `Pin::event`,
  `PwmCaptureChannel::capture`, `Spi::monitor`, `I2cMonitor::records`, `Uart::monitor`), `Uart::rx` and
  `I2cWatch::changes` are counted: positions, device losses and the time of the first unit in the event head; UART
  chunks end on line idle (`UartConfig::idle_chars`, one back-to-back burst each) and carry the time of their last
  byte when the device measures it (`RxChunk::end`). Clients take one `?` per call (CLT-24
  `CallError`), `Unsized` enums read newer variants as `Unknown(u32)` (SW-47). Review round 1 (15 Oct 2026) done.
  Open: `*_hl` crates are STD-14.
- 🚧 **STD-21 `ww_logic_analyzer`** · `v0.5.x` — `LogicAnalyzer` (triggered capture into RAM), `LogicStream`;
  `Raw` and per-channel `Transitions` encodings (edges as a `Series<u32>`), staged pattern triggers and protocol
  stages (UART byte, I2C address, SPI first byte). Skeleton written, docs
  page `docs/std_library/ww_logic_analyzer.md`; mock-server test pair still open (STD-13).
- 🚧 **STD-22 `ww_signal_generator`** · `v0.5.x` — `PatternGenerator` (digital, replays analyzer captures through
  a credit sink), `WaveformGenerator` (DAC channels). Skeleton written, docs page
  `docs/std_library/ww_signal_generator.md`; mock-server test pair still open (STD-13).
- ⛔ **STD-31 Observable `ww_gpio` pin level** · `v0.5.x` — `property!(ro+observe level: Level)` on `Pin` at one of its
  reserved indices 4..=6, so a host sees input changes without opening the `event` stream of every pin (slow inputs,
  a dashboard). Blocked on API-29 array-index-bounds: `Pin` sits in `Bank::pin[]` (and banks usually in an array
  too), and levels under a resource array can't be observed until their bound is known (SRV-18).
- 🚧 **STD-34 `ww_recorder`** · `v0.5.x` — control a recorder over WireWeaver: `Recorder` (status, card facts,
  two-step prepare/format with a token, recordings, markers, `sync`, settings with the flush thresholds, counters,
  optional `refresh_usb`) and `Track[]` (info, enable, durability, compression, counters). For the SD card log store
  of P2628 (repo `logring`), usable by any recorder. API crate, snapshot and docs page
  `docs/std_library/ww_recorder.md` done, the scaffold compiles; the first server is the logring's (rePort
  integration).
- 🚧 **STD-39 `ww_block`** · `v0.5.x` — a block device over WireWeaver (SD card, eMMC, flash): `BlockDevice` with
  `geometry` (block size and count, blocks per call, erase unit, erased value), free `read` and device-side `crc`
  (CRC-32, `ww_block::crc32`), `claim` / `release` of a write lease that stops the card's own user, `write` with a
  CRC and `erase` under it, and the optional `bulk` group: the `write_runs` credit sink acknowledged on the counted
  `written` stream, and the counted `read_runs` stream. Who may claim is the firmware's choice per link
  (`Common(Refused)`). Tier T1 of P2628 (logring's PC `LinkMedium`, S2 prepare on the PC). API crate, snapshot,
  docs page `docs/std_library/ww_block.md` and the round trip `tests/block_sd` done, the scaffold compiles; the
  first server is the logring's. `crc32`/`crc32_update` are table-driven, table size (none, `crc-table-256`,
  `crc-table-slice16`) picked by cargo feature.
- 🚧 **STD-40 `ww_sd`** · `v0.5.x` — raw SD card access over WireWeaver: `SdCard` with `capabilities`, `state`, the
  `detect` stream, raw `registers` (CID, CSD, OCR, RCA, SCR, SD Status), `reinit` with a per-step `InitReport`, and
  `command`: one raw command with an optional data phase, answering response bytes, `CardStatus` bits, the
  `Outcome` and timing. Commands are `Free`, need the `ww_block` lease, or are `Restricted` (CMD42, CMD27) unless
  the firmware builds with `raw-sd-unrestricted` (`ww_sd::check_access`). Tier T0 of P2628. API crate, snapshot,
  docs page `docs/std_library/ww_sd.md` and the round trip `tests/block_sd` done, the scaffold compiles; a device
  needs a raw-command hook in its SD driver (not checked yet for embassy-stm32's `Sdmmc`). The docs page says how a
  device in SPI mode fills the answers ("In SPI mode", from rePort's `sd_proto` engine); rePort's simulator serves
  `SdCard` on a simulated card in SPI mode (usb_io_fw IF-8).
- 💡 **STD-33 `ww_settings` single-sector mode** — a store in one sector with a RAM copy, for a part with exactly
  one sector to spare; a power cut during its erase loses every group (the window is the whole erase, 1 to 4 s on a
  128 KiB sector, see `docs/api/settings.md#parts-with-big-sectors`). Only if a device needs it.
- 💡 **STD-10 Device state machine trait** like CANopen.
- 💡 **STD-11 Register description registry** — WireWeaver for describing device registers.
- 💡 **STD-41 Frame splitters for byte streams** — a no_std, sans-IO splitter (bytes in, frames out with their
  time and status: CRC good or bad, truncated, overrun), the same code on the device and the host. Kernels: idle
  gap (today's `RxChunk`), delimiter (`\n`, `\r\n`, any byte), fixed length, length field, sync byte + length +
  checksum, COBS, SLIP, Modbus RTU, NMEA; picked by the host when it opens the stream (`Uart::frames(cfg)`), no new
  firmware. Frames of a bounded length are what a batched stream can take, where a raw byte stream can't (no size
  bound); with API-51, each frame XOR'd with the last one of the same key (Modbus address, NMEA sentence, CAN ID).
- 💡 **STD-44 SPI slave data-ready line** — a GPIO the slave asserts while an answer is queued (or the register map
  changed), as most SPI slave protocols with a computed answer have (ESP32 handshake, nRF IRQ); the master reads only
  when it is set, so the answer queue needs no polling. Part of `SpiSlaveConfig`, the pin named by the board.
- 💡 **STD-45 Live registers in a slave's register map** — address ranges of the I2C or SPI register map whose reads
  the host answers (I2C stretching meanwhile) or that change on read (clear-on-read status, FIFO data registers),
  so a chip with a few such registers can be emulated without moving it all to the answer queue.
- 💡 **STD-46 More I2C slave addressing** — a second address or an address mask (one emulator answering for several
  chips, a 24C16's block bits), general call, SMBus PEC and SMBALERT#/ARA. RP2350's controller has one address only,
  so this is per-device capability.
- 💡 **STD-51 Automatic sync master election** — boards that share a two-way link (CAN, Ethernet, USB through one
  host) choose the master of their sync line themselves, PTP-BMCA style: the best source wins (a GPS or a
  host-steered board with a good error bound), the lowest serial breaks a tie. Not over IRIG-B or PPS: they are
  one-way, so today the master is set explicitly with `configure_sync` (STD-49,
  `docs/std_library/ww_timebase.md#two-pcs-two-boards-one-line`).
- 🚧 **STD-47 `ww_pin_diag` pin tests** · `v0.5.x` — `PinDiag { const POINTS; set(point, PointState); read(point);
  release_all() }`: raw access to a board's digital points (MCU pins, shift register and expander outputs) for
  bring-up, verification and production tests; the firmware keeps its board's rules and answers `NotCapable`,
  `Common(InUse)` or `Refused { rule }`. Done: crate, docs page `docs/std_library/ww_pin_diag.md`, test pair
  `tests/pin_diag` (mock board: short, external pull-up, input-only pin, interlocked outputs, held pin), snapshot;
  the host tests and plan files are TOOL-15 (`ww pins test`). Missing: a device implementing it (rePort).
- 🚧 **STD-48 `ww_interned` strings as ids** · `v0.5.x` — `Interned { Id(UNib32), Text(&str) }` for names and reasons;
  ids resolved on the host from the firmware's ELF by build id. Done: the type, `Display` (`#17` for an unresolved
  id), `ww pins` finds the ELF by build id (`ww_pin_probe::device::Names`). Missing: an accessor for
  `defmt::Str`'s index (upstream, `Table::get_str`), reading the ids from the ELF's table with it.
- 📋 **STD-53 `Volt` with a `u16` value fails to decode on a device** — `I2cConfig { io_level:
  Some(Volt::milli(3300u16)), .. }` from the host gets `ArgsDesFailed` on the rePort B200A firmware, the same level as
  a `u32` value works (report-i2c-master bench run, 27 Oct 2026). Find where the `ww_si` value storage / numeric
  type differs between the host's writer and the no_std reader, fix it there, and add the case to the conformance
  corpus.
- 📋 **STD-54 `ww counters` against the rePort** — `ww counters` fails against the B200A: the firmware sends no build
  id, and its `cnt` format is v0 while the tool reads v2 (report-i2c-master bench run, 27 Oct 2026). Either side may
  be behind (STD-25): bring the firmware's `cnt` and build id up to what the tool needs, or make the tool say which
  format it got and what is missing instead of failing.
- 📋 **STD-56 Plain fields with a unit in their name become `Fixed`** — `shared_us: u64`, `rate_ppb: i32`,
  `ring_capacity_bytes: u64`, `queue_len: u32` and about 80 more across `ww_stdlib`, `ww_link`, `ww_stream`,
  `ww_client_server` and the host crates: same bytes, a Rust-API break per crate. On hold until STD-55 is built.
- 📋 **STD-59 Runtime `ww_si` improvements** · `v0.5` — binary prefixes in `Prefix`, bounds on `Quantity`
  (`Quantity<U, B>`, range in coherent units; a break: `B` explicit in the model), docs on `Quantity` vs `Fixed` vs `AnyQuantity`.
- 📋 **STD-65 Instants and clocks with named epochs** · `v0.5` — `Instant<Clock, ..>`: device-monotonic, Unix, TAI, GPS
  (TAI − 19 s), UTC through a versioned leap-second table on hosts; GPS week rollover as a `Seq` modulo 1024.
- 📋 **STD-66 Sentinels next to bounds** · `v0.5` — named codes in a `Fixed`'s raw value: single codes or code ranges
  (J1939 not-available / error ranges per 1, 2 and 4 bytes, SunSpec not-implemented 0x8000, Matter null, HCI RSSI
  127, reserved ranges) decode to an enum (`Value`, `NotAvailable`, `Error`, `NotImplemented`, a user meaning), `Option`
  only as the one-code shorthand. Stored as the raw `T` (size of `T`, no extra byte), `get()` gives the enum, and
  encoding it gives back the same code, so the bytes don't change. Niche codes and `Bounds` are checked disjoint and
  every code to fit `T` at compile time. Clipped / rail codes are a value with a saturation mark (at least / at most),
  not an absent value. Published in the model and honoured by the dynamic client, Python and C, with conformance cases.
- 📋 **STD-67 Levels: dB, dBm, dBV, dBFS, dBc, Np, LUFS, RSSI** · `v0.5` — `Level<Kind, Ref>` on the point algebra:
  level − level = gain, level + gain = level, references named (1 mW, 1 V, 20 µPa, full scale with its sine or square
  convention).
- 📋 **STD-68 Kinds that share a dimension** · `v0.5` — relative humidity, duty cycle, state of charge, strain, volume
  fraction; Hz vs Bq vs rad/s; electrical vs mechanical angle through pole pairs; colour vs thermodynamic temperature;
  explicit casts between siblings.
- 📋 **STD-69 More counted units** · `v0.5` — Symbol (baud = Symbol/s), Packet, Pulse, Event, Tick, Chunk, Particle,
  each a dimension of its own.
- 📋 **STD-77 Runtime scale parts and reference values** · `v0.5` — what device standards do at run time, without a
  refactor later: a value whose exponent (decimal or binary) is a runtime field next to compile-time unit and scale
  (BLE GATT exponent in the format descriptor, USB HID unit exponent, a SunSpec `*_SF` scale-factor register), and a
  value in % or per-mille of another resource's reading ("% of a reference register"); the model names the field or
  resource the scale comes from, the dynamic client, Python and C apply it exactly. Exponents of 10 or 2 only: a
  free runtime factor (calibration, currency) is a runtime value, not a unit: `ww_si::Quantity` / `AnyQuantity` or the
  application (TEST-28's list).
- 📋 **STD-70 Text unit codes in and out** — UCUM, UNECE Rec 20, QUDT, SenML / RFC 8798, OpenMetrics: exact
  imports, exports that say where they lose something. Later, in a separate optional crate `exact_units_interop`
  (Roman, 27 Oct): this release builds the capability to represent them (STD-77, TEST-28), not the tables.
- 📋 **STD-71 Device-standard units in and out** — BLE GATT 0x2904 / GSS, USB HID, BACnet, OPC UA EUInformation, CAN
  DBC / J1939, IEEE 1451 TEDS, SunSpec, IODD, with exact decimal factors and sentinel tables, from public sources
  only. Later, in `exact_units_interop` like STD-70.
- 📋 **STD-73 One table of unit symbols and spellings** · `v0.5` — display and parsing of every unit from one table,
  shared by Rust, si_dynamic, `ww` and Python, UCUM spelling where there is one.
- 📋 **STD-74 The runtime side catches up** · `v0.5` — `Quantity`, `AnyQuantity` and `SIExpr` get the exact magnitude,
  the °C point / interval split, levels and sentinels.
- 📋 **STD-75 Non-linear scales by name** · `v0.5` — AWG, mpg ↔ L/100 km, Beaufort, Richter, shoe sizes: a named exact
  conversion function each, or out of scope with the reason written down.

## Examples (`EX`)

- ✅ **EX-1 Host examples**: `blinky` (+ evolved API), `all_gpio`, `uart`, `blinky_py` (hand-written pyo3 wrapper).
- ✅ **EX-2 Serialization only**: `minimal_shrink_wrap`, `compare_wire_formats`.
- ✅ **EX-3 MCU examples**: nucleo_g0b1re, nucleo_h743zi2, usb_stm32h725ig, mcu_qemu, rp2 (USB, RTT, WebSocket and UDP
  over NCM).
- ✅ **EX-4 NCM UDP example**: `examples_mcu/rp2` `ww_udp_ncm`, host side `blinky_udp`.
- ✅ **EX-5 Project layout** of paired `<name>_api` / `<name>` crates (`docs/api/folder_structure.md`).
- ✅ **EX-11 Multi-property observe demo**: `motor_demo` (+ `motor_demo_api`), a VESC-like FOC controller on
  `motor_sim` with 52 `+observe` properties in three trait levels by rate tier (25 kHz, 1 kHz, 50 Hz), their values
  `ww_si` quantities (`I32` at a fixed prefix, `U32` totals, `F32` settings), on simulated time faster or slower
  than real time, in-process and over UDP; the `motor_demo` CLI observes chosen sets at chosen rates and shows
  achieved rates, updates and bytes per second against the budget, missed sends and the cost as one `StreamData`
  per value (`docs/examples/motor_demo.md`). The code moved to the [motor_sim](https://github.com/vhrdtech/motor_sim)
  repo (its DEMO-1) on 18 Oct 2026, so the demo can grow GUI dependencies there; `tests/observe` covers observe here.
- ✅ **EX-14 cnt counters in MCU examples**: `cnt` dependency, `-Tcnt.x` and `runner = "cnt run --chip ..."` on every probe-rs board (rp2, nucleo_g0b1re, nucleo_h743zi2, usb_stm32h725ig); `unhandled_exceptions` counter in `DefaultHandler`. `mcu_qemu` has no probe, so no counters.
- ✅ **EX-15 `ww_settings` demo**: `examples/settings_api` (`SettingsDemo`, two `value_on_changed` properties) +
  `examples/settings` (host, a simulated flash over the real client/server codepath, `tests/reboot.rs` checks
  values survive a reboot) + a `settings` bin in `examples_mcu/nucleo_g0b1re` (RTT, last 4 pages of bank 2) and
  `examples_mcu/nucleo_h743zi2` (USB, last 2 sectors of bank 2), both behind a `settings` cargo feature.
  `UserSettings` lives in `settings_api` so every implementation shares the same record shape.
- 📋 **EX-6 Example without async** · `v0.5` — `ww_device::blocking::Server` exists, no example uses it.
- 📋 **EX-7 Example with two media** · `v0.5`.
- 📋 **EX-8 NCM WebSocket + UDP example** · `v0.5` — both media on one stack, sharing the backend.
- 💡 **EX-9 WebSocket over Ethernet example** for some board · `v0.5.x` — the `ws` medium already runs on
  `embassy-net`, so Ethernet needs no transport work, only an example.
- 🚧 **EX-10 Reconnect demo** · `v0.5.x` — keep reconnecting with streams and UI feedback; `disconnect_keep_streams`.
  Done: `Commander::reconnect()` (re-selects the device with the stored config on any transport, restarts the loop
  on the kept command channel, `resume_streams()`). Missing: `connect_keep_trying` (retry until the device is back),
  UI feedback, the demo itself.
- 📋 **EX-12 Hubris based example** with sync code · `v0.5.x`.
- 📋 **EX-13 Advanced example / project template**.

## Documentation (`DOC`)

- ✅ **DOC-1 Docs site** (zensical): wire format, derive macro, showcase, API, evolution rules, transports, CLI,
  Python, std library pages.
- ✅ **DOC-7 Reading levels and a SerDes section by level**: a Basic / Advanced / Nomicon badge on each page from
  its front matter (`docs/levels.md`, `overrides/partials/level.html`); the SerDes nav lists the Basic pages first,
  then Advanced and Nomicon; `derive.md` split into derive, derive reference, size assumptions and layouts,
  `shrink_wrap.md` into an introduction, special types and wire format internals; the built-in types page rewritten
  against the code. A short "what works today" page (`docs/features_today.md`) is generated from this file
  (`just features-page`, `tests/features_page` fails when it is stale); this file is the "Feature tracker", last in
  the nav. The other sections followed in DOC-8.
- ✅ **DOC-8 Reading levels for the rest of the site**: level badges and a Basic-first nav (Advanced and Nomicon
  sub-sections) for Home, API, Evolution, Transport, Examples and Dev tools; the standard library grouped by family
  with an overview. Split by topic and level: `api/streams.md` (+ counted streams, credit sinks, stream protocol),
  `api/traits.md` (+ trait attachments), `evolution/rules.md` (+ crate layers), `transport/usb.md` (+ internals),
  `transport/ww_framer.md` (+ wire format), `ww_dfu.md` (+ A/B, one slot, internals), `ww_can_bus.md` (+ reference),
  `cli.md` (+ command reference), `python.md` (+ advanced). New entry pages for API and transport.
- ✅ **DOC-9 Features page for newcomers**: `docs/features_today.md` ("Features" in the nav) opens with what
  WireWeaver is, then per area a sentence on what it is for and its features grouped Basic / Advanced / Nomicon, each a
  short line with a link to its docs page, in-progress ones marked. Generated by `tests/features_page` from
  `FEATURES.md` (status) and `tests/features_page/entries.md` (area intros, level, line and link per item, several IDs
  per line allowed, `# Omitted` for items left off on purpose); the test fails on a done or in-progress item without a
  line, a line for an item that isn't, and links to pages not in `docs/`.
- 📋 **DOC-2 Run code in docs as tests** · `v0.5` — set up example tests from the `.md` files.
- 📋 **DOC-3 Link every feature to a docs page with an example** · `v0.5` — this file can serve as the index.
- 📋 **DOC-4 README pictures and showcases** · `v0.5` — including a picture showing density compared to MessagePack and
  friends; fill the `TODO`s in `README.md`.
- 📋 **DOC-5 Open docs `TODO`s** — export of all API levels with ids (`api/addressing.md`), splitting traits into
  multiple files (`api/traits.md`), interrupt vs bulk (`transport/usb.md`), `T` → struct containing `T`
  (`evolution/rules.md`).
- 🚧 **DOC-6 Docs for `ww_stdlib` crates and resource doc comments** · `v0.5.x` — done: `#[ww_trait]` leaves a Rust
  trait of the same name for rustdoc, with every resource and its doc comments (properties, streams, sinks and
  sub-traits as `wire_weaver::api_doc` markers, see `api/traits.md`), checked with nightly `--cfg docsrs` on a
  crates.io-shaped copy of `ww_i2c`. Missing: a docs.rs check of crates whose API uses another crate's types or
  traits (`ww_uart`, `ww_gpio`, `ww_si` types); the macro panicked on their `version = ".."` dependencies until
  API-14, try one on a crates.io-shaped copy.
- 📋 **DOC-10 Units docs section** · `v0.5` — choosing `Fixed`, `Point`, `Level`, `Quantity` or `AnyQuantity`, imperial
  / US customary with the ambiguity table, kinds, exactness and rounding rules, and what is out of scope on purpose.

## Testing, CI and performance (`TEST`)

- ✅ **TEST-1 CI on GitHub Actions**: fmt, sort, typos, clippy, rustdoc, nextest, doctests, semver-checks, MCU builds.
- ✅ **TEST-2 Integration tests** against the real host event loop over `in_process`, one crate per API feature.
- ✅ **TEST-3 Device side end-to-end tests** with real framers on both ends.
- ✅ **TEST-4 Timeout tests** for methods, properties and streams (async, blocking, promise).
- ✅ **TEST-5 Method tests**: calls, unimplemented, deferred replies, deferred not answered.
- ✅ **TEST-6 Property tests**: multi-req and single-req servers, user errors.
- ✅ **TEST-7 Stream tests**: byte buffers, basic and user types, open/close, sideband both ways, sinks, arrays of
  streams.
- ✅ **TEST-8 Framer fuzzing** (`fuzz/`).
- ✅ **TEST-23 Wire-format conformance matrix**: one corpus of cases (`tests/conformance/corpus.json`: a type, a
  value in the form Python uses, the golden bytes), run through every consumer of the wire format: the Rust derive
  (owned types, borrowed round trips), the `ww_self` dynamic client and Python (from the API model of
  `tests/conformance_api`), and C (the runtime for single values, the generated `fixed_layout` and `head_tail`
  readers compiled and run). 105 cases: numbers and varints, bit-sized fields and nibbles, relocated flags,
  `Option` / `Result`, enums with sub-byte, nibble and explicit discriminants, unknown and compiled-out variants,
  lists, strings, `ArrayString` / `ArrayVec`, `MaxLen` bounds both ways, nested, recursive, generic, sparse,
  sized and final structs, `PhantomData`, `validate`, `TailBytes` / `TailSize`, fixed layout, `head_tail`, `Series`,
  evolution both ways. A case a consumer can't express carries a gap mark naming the item that fixes it (shown in
  `docs/serdes/conformance_matrix.md`); a missing or stale mark fails. Part of `just test` and CI. It replaces an
  IR for the two layout engines (API-25 dropped). Found and fixed: `fixed_layout` types in APIs (the crate walker
  rejected them), unknown enum variants in Python; filed: INT-9, INT-10, TOOL-13, SW-66. See
  `docs/serdes/conformance.md`.
- ✅ **TEST-24 Frozen golden bytes guard**: a case's `bytes`, `direction` and `bytes_from` don't change, and a case
  doesn't disappear, once they are in the corpus at the merge base with main (`git merge-base HEAD
  origin/master`, falling back to local `master`) — a hand edit or a reset to `"?"` re-sealed silently before this,
  `CONFORMANCE=update` on its own only refuses to overwrite a filled-in case. `"wire_change": "EVO-n / reason"` on
  a case is the escape for a deliberate wire break (needs the matching `EVO-n` item and a `**Breaking:**`
  CHANGELOG entry), shown in the matrix report. Depends only on the repo's own git: without git, or without the
  corpus at the merge base (a shallow clone, or the corpus being new), the guard says so and passes rather than
  failing a check it can't run; CI's `test` and `coverage` jobs fetch full history so it actually runs there.
  `tests/conformance/src/frozen.rs`, `tests/matrix.rs::frozen_bytes`. See `docs/serdes/conformance.md`.
- ✅ **TEST-25 Coverage guard**: every `ww_self::Type` variant reachable from some case's type (an exhaustive
  match, so a new variant fails to compile here until handled) and every wire-relevant `shrink_wrap_derive`
  directive used by a type in `tests/conformance_api` is exercised by at least one case; a kind unlisted and
  unused fails, a kind listed (`TYPE_GAPS` / `DIRECTIVE_GAPS`) but now used fails too. Found two variants a
  prepared bundle never carries (filed INT-11) and two directives with no case yet (filed SW-67).
  `tests/conformance/src/coverage.rs`, `tests/matrix.rs::coverage`. See `docs/serdes/conformance.md`.
- ✅ **TEST-17 `shrink_wrap` code size bench**: `just size-bench` builds one bare firmware image per test case
  (plain struct, enum, list, bit fields, `Option`, decode or encode only, the request/reply codec of a small
  softcore firmware and the same by hand) for RV32I, RV32IC, Cortex-M0 and Cortex-M4F/M7 and prints `.text` +
  `.rodata`, or the largest symbols (`shrink_wrap/size_bench`, results in `docs/serdes/code_size.md`).
- ✅ **TEST-26 Credit sink resume on hardware** · `v0.5` — Donguru's header UART tx credit sink (API-34) on omarchy-m1
  (USART5, 921 600 Bd, jumper IO7-IO8, `donguru-core/examples/uart_resume` in typec_dongle): 1 MB of numbered lines,
  every byte on the line exactly once and in order (0 mismatches) in a plain run (14.3 s, 70 kB/s), with 4 link drops
  (`disconnect().keep_streams()`, 200 ms down, `Commander::reconnect()` 10-22 ms incl. resume, first flush after it
  0 ms; 15.3 s) and with a restarted host process continuing from the saved cursor (resume 2.6 ms, device was 544 B
  past the cursor). Found on the way: a 1 s flush stall after a resume (fixed), and a lost wrapped write in the
  firmware's pipes.
- 📋 **TEST-28 Units coverage against the standards** · `v0.5` — a dev-only test (not shipped) walks the public unit
  lists of UCUM, UNECE Rec 20, BLE GATT units, USB HID, the BACnet engineering-units enum and IODD as facts and shows
  each unit representable exactly in `exact_units`, or lists it on a short written "cannot, because" list for review.
  A capability enters `exact_units` only as a pattern a class of units or a standard's structure uses, never for one
  odd value: non-linear or empirical scales (AWG, Beaufort, Richter, Rockwell, octane, API gravity, shoe sizes),
  ill-defined codes (GATT "decibel" without a reference, month / year durations, unqualified kcal) and data-defined
  scales go on the list, at most as named functions or "unknown, reported"; a free runtime factor (calibration,
  currency) is listed as "runtime value, not a unit", carried by `ww_si::Quantity` / `AnyQuantity` or the application.
- 📋 **TEST-29 Units zero-cost gate** · `v0.5` — the ELF comparison of TEST-27 (`just units-elf`) becomes a gate in
  `just test` / CI: every plain / unit-typed pair (range check, struct encode and decode, Series encode and decode)
  compiles to identical code on every core and firmware profile measured (M0, M4F, M33; `opt-level` `z` and `3`), and
  a pair that differs fails with the per-symbol diff.
- ✅ **TEST-27 Units firmware budget** · `v0.5` — what `ww_si::Fixed` costs on an MCU against the plain fields it
  replaces, one harness any later piece (`Point`, rounding conversions, `Level`) re-runs with `just units-budget`:
  `examples_mcu/mcu_qemu/src/unit_budget.rs` (`qemu-unit-budget`, `stm32h725ig-unit-budget-cycles`) with plain/`Fixed`
  pairs that must write the same bytes, and `examples_mcu/fw_budget` printing instructions (Cortex-M3 under QEMU),
  flash and stack, at the shipped firmware's profile (`fw-z`, fat LTO, `opt-level = "z"`, and `fw-3`). Baseline of
  27 Oct 2026: struct encode/decode, a `Series` column and a `Range` check within ±7 % of the plain instructions at
  `fw-z`, +0 bytes of flash (+68 at `fw-3`), equal stack frames. Hardware check left: DWT cycles on the STM32H725 for the pairs that differ
  (the board was not behind the probe). Proved by ELF instead (`just units-elf`): the pairs compared instruction by
  instruction on thumbv6m, thumbv7em and thumbv8m.main at `fw-z` and `fw-3`: range check and `Series` encode
  identical everywhere, `Series` decode at `fw-3`; struct encode/decode differ by the derive's per-field write for a
  `Fixed` (a few instructions). STD-63's rounding conversions were measured against hand-written ones on M0 and M4F at `z` and `3` (`docs/std_library/rounding.md`, "Cost on a device"). STD-72 adds nothing of its own to measure: its markers are zero-size types and `Fixed::convert` is `Fixed::to`'s compile-time factor, so the `to` measurements cover it. See `docs/articles/units_budget.md`.
- 🚧 **TEST-9 Remaining test coverage** · `v0.5`:
  - [ ] properties: unimplemented, observe stream
  - [ ] streams: promise timeout
  - [ ] methods: return type evolving to a struct, adding arguments
- 🚧 **TEST-10 Binary size tracking** · `v0.5` — flash and RAM per feature for a set of examples, in CI. Consider
  compile-time construction so that only final bytes remain.
- 📋 **TEST-11 Keep an eye on coverage** — around 70% on average as of 2026-10-01, the important crates are at
  80-90%, `ww_framer` at 97%. Don't let it drop: add tests with new features and fixes, and raise the crates
  that are behind.
- 📋 **TEST-12 Coverage with hardware in the loop** · `v0.5.x`.
- 📋 **TEST-13 TODOs round** · `v0.5.x` — go through the `TODO` comments in the source.
- 📋 **TEST-14 Update all dependencies** · `v0.5.x`.
- 📋 **TEST-15 Measure serialization speed** on MCU and host (LE vs BE, array access vs pointers, `UNib32`
  implementations).
- 📋 **TEST-19 Lossy test medium** · `v0.5` — an in-process medium that drops, duplicates, reorders, stalls and
  disconnects on a seeded schedule, with the stream reliability scenarios S1-S6 as end-to-end tests
  (`docs/api/stream_reliability.md`).
- 🐛 **TEST-20 DFU transcript 1L-UPD-STEP races**: after the power cut, the host annotation "the call fails (link
  lost)" and the bootloader's "an update is under way" are recorded in either order, so
  `scenarios::pit_stop_shuttle_1l_upd_step` fails now and then (~2 in 10 alone, more often under the full parallel
  `just test`). Seen before the TR-12 changes too. The recorder needs one order for events of the host and the
  simulated device that happen at the same time. Found 15 Oct 2026.
- 🚧 **TEST-21 Power cuts against a flash store on STM32** — `examples_mcu/ecc_flash_g0b1`, a sequential-storage map on
  `ww_ecc_flash` (SRV-19) in bank 2 of a G0B1:
  - [x] reset loop: every boot loads and checks all keys against what earlier boots committed, then a SysTick
    reset at a random time cuts the stores. 3000 boots on the B153A (J-Link, 15 Oct 2026): 655 stores, resets
    during 28 programs, 453 erases and 1537 reads, every committed value loaded, none corrupted.
    `src/bin/reset_cut.rs` showed why no word ever failed its ECC check: on G0 a system reset waits for the flash
    operation (73 page erases cut 1-20 ms into their ~22 ms all left the page fully erased, programs only stop
    between double words).
  - [ ] real power cut against `ww_settings::Store` on `EccSafeFlash`, so the zeros path of SRV-19 runs on
    hardware: the B129A powered through the Donguru plug
    VBUS switch (U601, AOZ1375), switched from the PC in a loop with Donguru's `plug.vbus_mode` (typec_dongle
    API-11, in progress). The device can't keep its counters across a power cut: a host program reads the loaded
    and committed sequence numbers and the ECC count over SWD (Donguru CMSIS-DAP, session ended before each cut so
    SBU/CC don't back-feed the board) and checks every boot loads at least what was committed before the cut.
- 💡 **TEST-16 Host queue on a slow VM** — does it lead to fewer or no missed packets?

## Dropped and superseded

- **CLT-37 No millisecond per request from the accumulation window** — superseded by request priority (CLT-50)
  and the fine window timer (CLT-53) (user, 27 Oct 2026). CLT-37 flushed whatever accumulated as soon as the tx
  task had nothing else waiting, for windows under 1 ms (`TxInput::Idle`, `TxCore::wants_idle_flush`, removed), so
  a request's latency depended on what else happened to be queued. Now default traffic waits for the window, never
  longer, and a request that must not wait says so with `.urgent()`. Its measurement (B135 rev B, USB HS, 21 Oct
  2026): a property-like call 1.3 ms → 0.17 ms median, a 4 KB SD read 4.5 ms → 1.8 ms, CAN frames through a credit
  sink 3.8k → 7.2k/s.
- **STD-57 `Quantity` used at one fixed scale becomes `Fixed`** — dropped (Roman, 27 Oct 2026): the per-use list of
  `Quantity` fields found none whose scale is fixed by contract, so nothing to change.
- **API-25 Refactor codegen to use an IR** — decided against (18 Oct 2026). There are two wire-layout engines by
  design: `shrink_wrap_derive` keeps its own AST, as `ww_self`'s own types are encoded with `#[derive_shrink_wrap]`
  and the derive can't depend on `ww_self` (a bootstrapping cycle); the dynamic path (`ww_self` to the dynamic
  client, Python, the CLI and GUI) computes the layout again from the introspection model, and generated C (API-26)
  is one more consumer of that model. Instead of an IR, the conformance matrix (TEST-23), one corpus of cases run
  through every consumer, is the contract that keeps them byte-identical; `ww_self` is the shared model for
  everything outside the derive. Fallback if drift ever bites: a dependency-free crate holding only the layout rules
  (bit offsets, flags, nibble alignment), called by both `shrink_wrap_derive` and `ww_self`, breaks no cycle.
- **STD-30 `ww_gpio` port access** — dropped (Roman, 20 Oct 2026), `Bank::read_inputs` / `write_outputs`,
  `PinLevel` and the `simultaneous_*` capabilities removed before `ww_gpio` 0.1.0 was published. `ww_gpio` stays
  simple and low speed, one pin per call (IO family D9): many pins in one round trip are per-pin calls in one multi
  request (CLT-15, `docs/std_library/ww_gpio.md#several-pins`); several pins at one instant or with precise timing
  (bit-banged parallel buses, a strobe with its data lines) belong to the signal generator (STD-22) and the logic
  analyzer (STD-21). The extra methods only cost flash on every GPIO server.
- **SW-31 `Delta` / `DeltaOfDelta`**, **SW-32 `XorFloat`**, **SW-50 compressed sequences in APIs** — superseded by
  `Series<T>` (SW-60): its numeric columns use the SW-31 prefix code with the delta or delta of delta predictor, its
  float columns the SW-32 XOR code, plus packed, constant and plain and the smallest picked per column, in every API
  position and in dynamic clients and Python (CLT-29); the types were removed (18 Oct 2026).
- **API-15 Return streams from functions** — superseded by API-31 stream functions: `#[stream(T)] fn name(args)`
  opens a stream with typed arguments and result.
- **Document things** (old notes) — superseded by `docs/serdes/shrink_wrap.md` and `docs/evolution/rules.md`;
  the notes described an earlier format (field ids, `vlu16n_rev`). Remaining ideas moved to their areas.
- **LEB numbers** — superseded by `UNib32` and `UVlq32` (`ULeb128` is still in the derive AST, unimplemented).
- **Field ids** — decided against for now: fields are identified by position, which works well, and ids would
  add confusion together with flag relocation. The `id` leftovers in `shrink_wrap_derive/src/transform/` can
  go.
- **Option and Result taking two id numbers** — dropped with field ids; flags are separate fields.
- **Put length in front into a specified field** — done as `UVlq32Backfill`.
- **CLI common blocks AST generation** and **excluding common blocks from the AST** — done as API snapshots.
- **Compatibility check using AST signatures on connect** — done.
- **Introspect as a trait, removed from `ww_client_server`** — won't do.
- **WebUSB descriptors** — only some browsers support WebUSB and it needs separate host support; NCM is more
  appealing, especially with pre-programmed unique IPs.
