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
- ✅ **SW-64 `Series` written straight from a `SeriesStore`**, column by column, the same bytes as the item walk:
  `SeriesStore::columnar` / `write_store` (leaves, `Option`, arrays, tuples, derived structs and enums, by hand for
  `NumericValue`) and `series::write_store_series`, used by `ww_stream`'s batchers on their own. Needs one variant
  per enum and `Option` over the rows (a prefix change falls back to the walk). One FOC level batch (21 x
  `Series<T>` + clock, n = 32, `DeltaPacked`): 74.9 k instructions on a Cortex-M3 instead of 143.5 k, 100 k cycles on
  an STM32H725 instead of 312 k (60 k against 125 k at `opt-level = 3`). See
  `docs/serdes/series.md#from-the-store-column-by-column`, `shrink_wrap/shrink_wrap/src/series/store_write_tests.rs`,
  `examples_mcu/mcu_qemu` (`qemu-store-columns`).
- ✅ **SW-65 Aligned plain columns**: the numeric escape `11 11` and float codec `0010`, values little-endian at
  their byte width after zero bits to a byte, the `AlignedPlain` policy. The cheapest writer from a store (34 k
  cycles per FOC level batch on an STM32H725 at `opt-level = 3`), at 2.3x the bytes of `DeltaPacked`. Readers from
  before it reject the column. See `docs/serdes/wire_format.md#seriest`, `docs/serdes/series.md#cpu-and-flash-on-an-mcu`.
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

## API model and codegen (`API`)

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
  items (the serialized value of each write, never split; `StreamSideband::Credit { have, grant }`), so a receive
  buffer holds as many items as fit whatever their sizes; the host waits when out of credit (async, blocking,
  `try_send`), writes carry the byte position of their first byte so a lossy medium resumes with go-back-N from `have`
  (`Resend`, `CreditQuery`), `flush()` waits for the final ack. No deadlock: generated `<SINK>_MAX_ITEM_BYTES` (from
  `MAX_SIZE`, `MaxLen` bounds included) and `SinkCredit::sized_for()` check at compile time that the buffer holds the
  largest item and cap the re-grant threshold, the host fails an item bigger than the buffer with
  `StreamError::ItemTooBig`. Device state in `ww_client_server::credit::SinkCredit`, dynamic and Python clients too.
  Used by `ww_dfu` `StageBulk::plan`. For bulk transfers (chunky OTA plans over LTE-M/LoRa/UDP), UART bridges, CAN
  tx. See also CLT-18 backpressure (`docs/api/credit_sinks.md`, `tests/credit_sink`).
- ✅ **API-42 Credit-sink positions without a session limit** · `v0.5` — host (`CreditTx`) and device (`SinkCredit`)
  count positions in `u64`, the wire carries them modulo 2^28 (`credit::POS_MODULUS`, at most 4 bytes as a `UVlq32`,
  `UNib32` in the sideband), the receiver extends them from the position it expects (serial number arithmetic). The
  buffer is at most `MAX_WINDOW` (64 MiB, a quarter of the modulus), checked at compile time by `sized_for`, and the
  host never has more in flight; one fixed modulus on every link, nothing negotiated at `Open`. Tested over a seeded
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
  batch, API-41) may be asked for again from a position inside it: a `ResendFrom` there, or a resume from a cursor
  user code confirmed in the middle of it (`AckMode::Explicit`). The device sends the event that holds the position
  whole (`StreamStore::read` returns it for every store), the host's `Receiver` takes it with
  `Verdict::Accept { skip }` (also for ranges it had before a resume), `PositionedStream` gives the new units. Fixed
  with it: `ResendFrom` no longer counts as an acknowledgement on the device (the store kept less than an explicit
  host had confirmed), acknowledgements and resends are valid up to the highest position sent (a late `ResendFrom`
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
  host's `ResendFrom`; `ww api scaffold`, introspection (`ApiItemKind::StreamFn`), compatibility check and API diff
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
  tree and Python show them (`docs/api/optional_items.md`, `tests/traits`). Not yet: `#[optional]` on
  `ww_impl!`, the GUI (`wire_weaver_tool`).
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
- 📋 **API-14 Load API crates from crates.io** · `v0.5` — the crate walker handles path and workspace dependencies;
  `crate = "version"` dependencies hit a `todo!()` in `wire_weaver_core/src/transform/crate_walker.rs`. Also
  dependencies from git (backlog). Every crate downloaded from crates.io has only such dependencies, so until this is
  done a published API crate whose API uses another crate's types or traits doesn't compile for its users or on
  docs.rs (DOC-6).
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
- ✅ **API-39 Batched property samples (observe every sample)** — a `ro+observe+batched` property observed in
  `ObserveMode::Samples { every_us, max_age_us }` sends every sample the device takes as a `Series<T>` in its
  `Batched<T>` slot of the level's update, their times shared by the level (API-49)
  (`docs/api/observing.md#every-sample-batched`, `tests/observe_batched`). In: parsing and the model
  (`ApiItemKind::Property::batched`), the mode
  validated and clamped (`ObserveLimits::samples_max_age_us`), generated `<Level>Sampler::sample(t, ..)` (O(1),
  interrupt-safe after `ObserveBatches::split()`), double-buffered stores and a level outbox encoded in
  `poll_properties` with several updates per call, `missed` for lost samples and dropped batches, typed observers
  per sample, the dynamic client, `ww observe --samples` and Python (`recv()` samples, `recv_batch()` columns).
  Motor demo (`motor_sim` repo) on it, measured through the real generated code: `docs/api/batching.md`. Missing:
  DWT cycle counts of `<Level>Sampler::sample` on hardware (the encode-side counts are piece 4/SW-63's).
- ✅ **API-49 One time column per observe level (shared clock)** — the samples a `sample(t, ..)` call takes for a
  level's `+batched` properties share one time column per update: `EventKind::PropertyUpdate::sample_times`
  (`Series<Second>`) next to `t`, the `Batched::Samples` slots untimed, `Batching::SharedClock` in introspection. A
  level keeps one batcher of rows (time and every value), its `Samples` entries share the finest `every_us` and the
  shortest `max_age_us`. Against a time column per property (21 FOC properties, n = 32, `DeltaPacked`): -36 % bytes,
  -52 % encode cycles on an STM32H725, -46 % instructions on Cortex-M3/M4F
  (`docs/api/observing.md#which-rates-fit-an-mcu`, `tests/observe_batched`).
- 📋 **API-17 Methods arguments to a user struct** · `v0.5.x`.
- 🚧 **API-18 Size analysis** · `v0.5.x` — maximum buffer usage in the worst case, auto-calculated buffer sizes for events,
  arguments and outputs. Done: `MAX_SIZE` of types (SW-19) and per-method `<METHOD>_REQUEST_MAX_LEN` /
  `<METHOD>_RESPONSE_MAX_LEN` consts in `no_alloc` servers, checked at compile time against a buffer in
  `tests/bounded` and `examples_mcu/mcu_qemu`. Missing: consts for properties, streams and sinks, the largest of
  all of them for a whole API (to size `ww_device` buffers such as `UdpBuffers<MAX_MESSAGE_LEN>` automatically),
  paths of nested traits counted exactly (now the worst case of the full path), the same consts on the client.
- 📋 **API-19 Change dispatcher match order** to process common requests faster · `v0.5.x`.
- 🚧 **API-20 Trait paths on the server side** · `v0.5` — servers serve `PathKind::GlobalFull` requests (crate,
  version and trait name) wherever the trait sits in the tree, `ErrorKind::TraitNotFound` otherwise
  (`tests/cross_version`, `docs/api/addressing.md`). Missing: `PathKind::GlobalCompact` (compact ids of
  traits are not in the API model yet, none is assigned), traits under a resource array.
- 📋 **API-21 `no_alloc = false` servers** — don't compile today.
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
  `lost` / `t` fields (IO family D4, D5). Reliable streams add `Ack` / `ResendFrom` / `Query` (go-back-N, grants in
  bytes), keep unacknowledged events in a store, resume by cursor after a reconnect with the latest values first and
  the backlog beside them; a send scheduler with priorities between streams; typed `cnt` counters
  (`docs/api/stream_reliability.md`). Done, the protocol core: wire types in `ww_client_server::stream`
  (`StreamHead` with its hand-laid encoding, byte-verified; `StreamCursor`, `StreamOpened`, `EventKind::StreamItem`,
  `RequestKind::StreamOpen`, the sideband), the `ww_stream` crate (`Counted`, `Reliable`, `Receiver`, `RamRing`,
  `Scheduler`, `StreamCnt`; sans-IO, `no_std`, tested over a seeded lossy link with random disconnects),
  `ww_self::ApiItemKind::StreamFn`, codegen (API-31), the host runtime (CLT-23). Missing: credit sinks on it
  (API-34). The persistent store is done (SRV-15).
- 📋 **API-34 Credit sinks on the stream protocol** · `v0.5` — `Ack` / `Query` / `Resend` shared with reliable
  streams, sessions and resume for credit sinks, so a DFU plan continues after a reconnect; `SinkCredit` moves into
  `ww_stream` (`docs/api/stream_reliability.md`). Sink positions stay as API-42 has them (`u64` in state, modulo
  2^28 on the wire, bounded by the window), streams keep their full `UVlq64` positions (their gaps are unbounded);
  a resume cursor for a sink would carry a full position. A session id in `Open` and its answer would also stop a
  late duplicate `Open` (UDP) from resetting a running session.
- 💡 **API-24 Guarded calls for critical resources** · `v0.5.x` — for safety-critical actions, such as starting a motor
  or switching high power, a request that happens to match predefined bytes must not be enough. Add an extra
  check, e.g. a CRC over the resource identity and arguments (a "strict mode" per resource), or a
  request/response challenge before the call is executed.
- 💡 **API-25 Refactor codegen to use an IR** · `v0.5.x`.
- 🚧 **API-26 Generate C** · `v0.5.x`, and an **FFI example**. The C runtime is there (SW-43 c-runtime); generated
  `.h` / `.c` for an API's types, a device-side dispatcher, introspection as a const blob and `ww_framer` are not.
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

## Server / device side (`SRV`)

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
- 🧪 **SRV-19 ECC-safe STM32 flash for stores** — `mcu/ww_ecc_flash`: `EccSafeFlash` wraps any `NorFlash` (embassy-stm32's
  `Flash`) and reads it one ECC word at a time; a word with an uncorrectable ECC error (power cut while programming
  or erasing) reads as zeros instead of an NMI (STM32G0, `FLASH_ECCR.ECCD`, through `on_nmi()` or feature
  `nmi-handler`) or a bus error (STM32H7, `DBECCERR`, ignored under FAULTMASK + `BFHFNMIGN`), so sequential-storage
  skips it. Never `MultiwriteNorFlash`. Blocking and async. `docs/api/ecc_flash.md`. On hardware: a G0B1 (B153A)
  store survives 3000 resets in the middle of its programs and erases (TEST-21), but a reset never tears a G0 flash
  word, so the zeros path itself still waits for a real power cut (TEST-21); H7 is built, not run on hardware.
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
- 💡 **SRV-13 IPC over USB / multi-app devices / USB multiplexer** · `v0.5.x`.
- 💡 **SRV-14 FPGA server** · `v0.5.x`.

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
  `ww_picoboot` (PICOBOOT host on nusb, `SimRp2350` for tests), `follow::rp2_update()`, `ww bootsel`, tested end to end
  on a simulated device (`tests/follow_reboot`, `docs/transport/follow_reboot.md`). The PICOBOOT host moved to its
  own repo, [picoboot_host](https://github.com/vhrdtech/picoboot_host) (evaluated the crates.io `picoboot` 0.3.0
  crate first: no sans-IO boundary to put `SimRp2350` behind, no `Image`/UF2 handling, no orchestrated flash+verify,
  no `chip_id`/`GET_INFO` — adopting it would have meant donating most of `ww_picoboot` upstream anyway); WireWeaver
  depends on it as `ww_picoboot` (a git dependency on github.com/vhrdtech/picoboot_host until it is on crates.io). `wire_weaver_client`'s `picoboot`
  feature (off by default) gates `follow::rp2_update()` and the `ww_picoboot` re-export, so a host app that only
  talks WireWeaver over USB doesn't pull in PICOBOOT; `ww bootsel` turns it on, and `follow::rp2_update_with()` (what
  `tests/follow_reboot` runs against a simulated device) needs neither it nor `usb`. Missing: a run against a Pico 2 /
  rePort; RTT (another control block: a second ELF or a RAM scan) and UDP / WebSocket (the port may change); asking
  for the reboot from the CLI needs CLT-10.
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

## Transports and protocols (`TR`)

- ✅ **TR-1 `ww_framer`**: packs many small messages into one packet or splits a big one across packets; U2Head with
  1 byte overhead, CRC, recovery from missed frames, fuzzed.
- ✅ **TR-2 `ww_link`**: link setup, device info, API version exchange.
- ✅ **TR-3 USB**, **RTT**, **WebSocket**, **UDP** and **in-process** (messages, or simulated USB: real USB framing
  and packet sizes, `in_process::usb_device`).
- ✅ **TR-4 USB CDC-NCM**: WebSocket over a USB network adapter, with a DHCP server so the host gets an address by
  itself, ping and a hello page over HTTP (`examples_mcu/rp2`, `ww_ws_ncm`); UDP the same way (`ww_udp_ncm`).
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
  flushes replies at once when no other request waits instead of after the accumulation window (`ww_device`
  `Server::handle`; DFU transcripts +1-2 packets). `WireWeaverClass::with_endpoints()` lets
  firmware allocate the endpoints, e.g. double-buffered bulk ones (Donguru, embassy-rs/embassy#7275: raw bulk 500 ->
  950 kB/s OUT on the G0). Next: CRC through a pluggable checksum so MCUs can use their
  CRC unit (~13% of the CPU per packet on the M0+); a host-side flush for lone requests that keeps packetization
  deterministic (flushing whenever the event loop goes idle made the DFU transcripts vary between runs); the same
  reply flush for the blocking server.

## Introspection (`ww_self`, `INT`)

- ✅ **INT-1 API model AST** with syn-style `Visit`/`VisitMut`, sent by devices as introspection data.
- ✅ **INT-2 Bundles** recording crate versions, signatures of skipped traits and types.
- ✅ **INT-3 Relocated `#[flag]` fields** in introspection data and dynamic serialization.
- 📋 **INT-4 Bit-aligned integers in `ww_self`** · `v0.5` — `un`/`in` types (`u1`..`u32`, nibbles) are not supported in
  the introspection model yet, so the dynamic client and Python can't handle them. Add them, with tests
  through dynamic serdes.
- 📋 **INT-5 Documentation pass on `ww_self`** · `v0.5` — doc comments are missing (`TODO` in `ww_self/src/lib.rs`).
- 📋 **INT-6 `sized` / `final_structure` on `ww_self` types** · `v0.5` — where a type is unlikely to evolve, to make
  introspection data smaller. Can't be undone later, so decide per type.
- 💡 **INT-7 Compress `ww_self` strings with a pre-shared dictionary** — partly addressed by leaving snapshot-known
  traits and types out of introspection data.
- 💡 **INT-8 Model enums that tolerate newer variants** — the `ww_self` enums (`Type`, `ApiItemKind`, `Fields`, ...)
  and `ww_numeric::NumericAnyType` are `no_future_version` (SW-47): a host reading a newer model fails as a whole.
  With `Unknown` handled in every walker (visitor, signature, inline, values, compat, CLI, Python), an older host could
  show the rest of a newer device's API and mark the unknown parts.

## Evolution, versioning and compatibility (`EVO`)

- ✅ **EVO-1 Evolution rules** in `docs/evolution/rules.md`: crate layers and version bumping.
- ✅ **EVO-2 SHA signatures** of APIs and types, hashing only the major version (or `0.minor`), version independent.
- ✅ **EVO-3 Crate version recorded in bundles**.
- ✅ **EVO-4 Global traits referenced by hash**, global type registry (`ww_global`).
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
- 💡 **EVO-7 Git dirty flag** — not implemented, although the old tracker had it as done: neither `ww_version` nor
  codegen record git state. Uncommitted API changes are still caught, since they change the API's SHA
  signature. Firmware git info (sha, dirty, branch, tag) can be exposed through `ww_firmware_info`'s
  `bedrock_build_info` stream. Decide whether the link or version also needs it.
- 📋 **EVO-8 Snapshots of crates.io crates** — depends on "Load API crates from crates.io" above.

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
- 🚧 **TOOL-6 Dev GUI / web tool** · `v0.5.x` — `wire_weaver_tool` (egui, Trunk) shows the AST and generated code. Goals:
  a web tool like the pest playground, a dynamic UI for any API (MVP prototyped), widgets per type, mockup
  server UI. See `docs/dev_tool.md`.
- 💡 **TOOL-7 Generated Python wheel for a specific API** — nothing is generated for Python yet:
  `examples/blinky_py` is a hand-written pyo3 wrapper around the Rust client, and `wire_weaver_py` is dynamic.
  Generating typed pyo3 bindings from an API crate might be useful (see the `TODO` in
  `docs/api/folder_structure.md`).
- 📋 **TOOL-8 `ww` inspect / read / write all GPIOs on any device** · `v0.5.x`.
- 📋 **TOOL-9 Buffer visualizer / "explain" encoding** · `v0.5.x` — record operations on buffers so that each bit can be
  traced to an item, show it using the AST and dynamic values (`TODO` in `wire_weaver_core/src/eval.rs`).

## Standard library (`ww_stdlib`, `STD`)

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
- ✅ **STD-24 `ww_firmware_info` / `ww_board_info` / `ww_uid` / `ww_user_info` device contract** · `v0.5` — the info
    crates every device implements. `FirmwareInfo` const items `FIRMWARE` (name, version, git, build time, profile),
    `BUILD_ID` (GNU build id, finds the ELF) and optional `BUILD_RECORD`; `build::emit()` + `firmware_summary!()` for
    `build.rs`, `build_id()` reader; `LastReset` (`ResetRecord` with a `Fault` per architecture behind `cortex-m` /
    `riscv`, stack words, `boot_count`, `clear`). `BoardInfo::BOARD` with `RevisionSource`. `Uid::IDS`, first id is
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
- ✅ **STD-30 `ww_gpio` port access** — `Bank::read_inputs(pins)` answers the input levels of the listed pins (indices
  into `pin[]`) in the same order, `Bank::write_outputs(levels)` sets a list of `PinLevel { pin, level }`, all or
  nothing; both at one instant where the hardware allows (one register read / write), which
  `BankCapabilities::simultaneous_read` / `simultaneous_write` say, pin by pin in list order otherwise. For bit-banged
  parallel buses and strobes from the host, which multi-call can't make simultaneous. Pins by index, no masks on the
  wire (IO family D9). Bank indices 1 and 2 from its reserved range, capabilities appended with `#[default = false]`.
  Docs `docs/std_library/ww_gpio.md#port-access`, tests `tests/io_gpio`, STM32 boards in `examples_mcu` (IDR / BSRR).
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
- 🚧 **STD-6 `ww_gpio_hl`** — usable, some APIs are still missing.
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
  first server is the logring's.
- 🚧 **STD-40 `ww_sd`** · `v0.5.x` — raw SD card access over WireWeaver: `SdCard` with `capabilities`, `state`, the
  `detect` stream, raw `registers` (CID, CSD, OCR, RCA, SCR, SD Status), `reinit` with a per-step `InitReport`, and
  `command`: one raw command with an optional data phase, answering response bytes, `CardStatus` bits, the
  `Outcome` and timing. Commands are `Free`, need the `ww_block` lease, or are `Restricted` (CMD42, CMD27) unless
  the firmware builds with `raw-sd-unrestricted` (`ww_sd::check_access`). Tier T0 of P2628. API crate, snapshot,
  docs page `docs/std_library/ww_sd.md` and the round trip `tests/block_sd` done, the scaffold compiles; a device
  needs a raw-command hook in its SD driver (not checked yet for embassy-stm32's `Sdmmc`).
- 💡 **STD-33 `ww_settings` single-sector mode** — a store in one sector with a RAM copy, for a part with exactly
  one sector to spare; a power cut during its erase loses every group (the window is the whole erase, 1 to 4 s on a
  128 KiB sector, see `docs/api/settings.md#parts-with-big-sectors`). Only if a device needs it.
- 💡 **STD-10 Device state machine trait** like CANopen.
- 💡 **STD-11 Register description registry** — WireWeaver for describing device registers.
- 📋 **STD-35 Slave data paths in `ww_i2c` and `ww_spi`**: `I2cMode::Slave { addr, monitor }` and `SpiRole::Slave`
  exist and capabilities say `slave_supported`, but nothing carries the data yet. Needed: what the device answers
  (a register map or buffer the host fills, or a request the host answers in time), what it received (a stream of
  writes with timestamps, like the monitors), clock stretching and its limits for I2C, and what happens when the
  host is too slow (NACK, idle bytes). First user: the rePort (usb_io_fw IF-5, v1.0 launch, Roman 16 Oct 2026).

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
- 📋 **EX-10 Reconnect demo** · `v0.5.x` — keep reconnecting with streams and UI feedback; `disconnect_keep_streams`.
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
  crates.io-shaped copy of `ww_i2c`. Missing: docs.rs builds of crates whose API uses another crate's types or traits
  (`ww_uart`, `ww_gpio`, `ww_si` types), the macro panics on their `version = ".."` dependencies, needs API-14.

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
- ✅ **TEST-17 `shrink_wrap` code size bench**: `just size-bench` builds one bare firmware image per test case
  (plain struct, enum, list, bit fields, `Option`, decode or encode only, the request/reply codec of a small
  softcore firmware and the same by hand) for RV32I, RV32IC, Cortex-M0 and Cortex-M4F/M7 and prints `.text` +
  `.rodata`, or the largest symbols (`shrink_wrap/size_bench`, results in `docs/serdes/code_size.md`).
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
