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
- This file is also rendered as the docs site's "Features" page (`docs/features.md` includes it), so indent
  nested lists and paragraphs inside an item by 4 spaces, as Python-Markdown requires.
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
- ✅ **SW-11 Alloc writer** `BufWriterOwned`, `SerializeShrinkWrap` for `&[u8]`, `String` as UFS.
- ✅ **SW-12 In-place builders** used by the server codegen (event and error builders), and the manual
  patch-the-discriminant-later pattern (`docs/serdes/showcase.md`).
- ✅ **SW-13 Binary format documented** in `docs/serdes/shrink_wrap.md`, worked tricks in `docs/serdes/showcase.md`.
- ✅ **SW-14 Usable standalone** as a dense data storage format (`examples/minimal_shrink_wrap`,
  `examples/compare_wire_formats`).
- ✅ **SW-31 `Delta` / `DeltaOfDelta`**: integer sequences as (differences of) differences in a bit-level prefix code,
  zero-copy `Delta<'i, T>` and `DeltaOwned<T>`, `u8`..`u64` / `i8`..`i64`; the Gorilla timestamp code
  (`docs/serdes/shrink_wrap.md`, `shrink_wrap/src/series/`, `shrink_wrap/tests/series.rs`).
- ✅ **SW-32 `XorFloat`**: `f32` / `f64` sequences as XOR with the previous value (Gorilla), bit-exact,
  `XorFloat<'i, T>` / `XorFloatOwned<T>` (same places as SW-31).
- ✅ **SW-50 Compressed sequences in APIs**: `Delta`, `DeltaOfDelta` and `XorFloat` (borrowed or owned) in every API
  position (arguments, returns, properties, stream and sink items, fields of API types), visible in introspection
  (`ww_self::Type::Delta` / `DeltaOfDelta` / `XorFloat`, appended), read and written by the dynamic client and Python
  (lists of numbers); another codec or element type, or a plain `Vec` instead, is breaking (`ww api check`).
  `tests/series`, `docs/types.md`, `docs/evolution/rules.md`.
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
  format (`shrink_wrap/tests/enum_size.rs`, `docs/serdes/derive.md`). Unblocks reading `RefVec<Sized T>` in place.
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
  `SelfDescribing`, checked at compile time. `docs/serdes/shrink_wrap.md`, showcase.
- 📋 **SW-42 `MAX_SIZE` on owned types** · `v0.5.x` — `SerializeShrinkWrapOwned` has no worst-case size yet, only
  the borrowed trait has (SW-19); host code sizing a buffer for an owned value uses the borrowed type's.
- 📋 **SW-20 Sub-type numbers** · `v0.5.x` — numbers restricted to a range, like `10..=25`, or a set with gaps, like
  `0..=8, 12, 16`; checked on serialization and deserialization, possibly using fewer bits on the wire
  (`docs/types.md`, "Subtypes").
- 📋 **SW-21 Multi-dimensional arrays** (2D, 3D).
- 🚧 **SW-22 Fuzz `shrink_wrap`** — `fuzz/` has one `shrink_wrap` target, `shrink-wrap-tail-size` (`TailSize` types
  round-tripped, random bytes into their readers); `shrink-wrap-series` (SW-31, SW-32) and `shrink-wrap-ref-vec`
  (SW-37) came with their features. Round-trip every built-in type and the derive's features.
- 🔍 **SW-23 Tuple enum variants evolution** — check that tuple variants evolve as `docs/evolution/rules.md` says
  (fields can be appended) and add a test.
- 💡 **SW-24 ZigZag encoding for signed variable length numbers** — consider ZigZag (as in protobuf) so that small
  negative values stay short, e.g. signed counterparts of `UNib32`/`UVlq32`.
- 💡 **SW-25 In-place builder for server handlers** · `v0.5.x` — a new step-by-step / partial builder (e.g. for a
  `RefVec`) might be useful with the new `Context` in server handlers, to write replies and events straight
  into the buffer. It might not work out.
- 💡 **SW-26 Map support** — `HashMap`/`BTreeMap`, probably as a `Vec` of `(K, V)`.
- 💡 **SW-27 In-place editing** of serialized data.
- 🚧 **SW-28 Compression wrappers** · `v0.5.x` — the time-series part is done as SW-31 (`Delta`, `DeltaOfDelta`)
  and SW-32 (`XorFloat`); in-place dictionary compression as `Compression<T>` (strings?) stays an idea.
- 💡 **SW-29 Flatten support** — apply a type's layout across struct/enum boundaries.
- 💡 **SW-30 `derive_shrink_wrap` on a whole module at once**. For definitions in separate files with own syntax, see
  [standalone definition files](#api-model-and-codegen-api).
- ✅ **DER-1 `#[derive_shrink_wrap(..)]` attribute** with clear syntax, no hidden magic and good error messages
  (`docs/serdes/derive.md`); replaced `#[derive(ShrinkWrap)]`.
- ✅ **DER-2 Discriminant checks**: discriminants must fit `ww_repr`, implicit ones are numbered like in Rust.
- ✅ **DER-3 `#[default = ..]` for evolved fields** and `#[flag]` relocation.
- ✅ **DER-4 `TailBytes` must be last**: rejected anywhere but as the last field of a struct or enum variant, and nested
  in `Option`, `Vec`, tuples or arrays (`check_tail_bytes_position`), like `UVlq32Backfill` must be first.
- ✅ **DER-10 Generic arguments on user field types**: `Foo<u8>`, `Wrapper<'i, T>` are emitted as written,
  `Wrapper<'i, T>` maps to `WrapperOwned<T>` in the owned variant (`docs/serdes/derive.md`, type mapping; test
  `shrink_wrap/tests/generic_external.rs`).
- ✅ **DER-11 Field type from a macro_rules `$T:ty` fragment**: accepted like the type written directly, instead of
  rejected as an unsupported `syn::Type::Group` (test `shrink_wrap/tests/macro_ty_fragment.rs`).
- ✅ **DER-12 `Display` from doc comments**: a `display` directive writes `core::fmt::Display` and
  `core::error::Error` (borrowed and owned) from the first doc line of each variant/struct, `{field}` filled in,
  `#[display("..")]` to override; a variant with neither is a compile error. Error enums need no hand-written
  `Display` on hosts (`docs/serdes/derive.md`; test `shrink_wrap/tests/display.rs`).
- 📋 **DER-5 `#[since = "x.y.z"]` on fields** — generate correct evolution code from it (`TODO` in
  `shrink_wrap_derive/src/lib.rs`).
- 📋 **DER-6 Check that `TailBytes` ends the outer type too** — a struct ending in `TailBytes` used as a non-last field
  of an outer `sized` / `final_structure` type ("last in the first `Unsized` type on the way up") is not visible to
  the derive; needs a marker on the type, e.g. an associated const, and a compile-time assertion in the outer type.
- 📋 **DER-7 Prefix enum discriminants** · `v0.5.x`.
- ✅ **DER-8 `#[cfg(..)]` on enum variants** — a variant keeps its discriminant in every configuration, a build without
  it reads `Error::EnumVariantDisabled` (`docs/serdes/derive.md`). First user: `ww_numeric::NumericValue`, one cargo
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
  tx. See also CLT-18 backpressure (`docs/api/streams.md`, `tests/credit_sink`).
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
- ✅ **API-32 Per-method error lists** — `#[errors(Common(InUse), Nack, ..)]` on a trait method: checked against the
  error enum when the trait is parsed (a compile error on a wrong name), in the API model (`ww_self` `Method::errors`),
  generated client and rustdoc `# Errors` with each variant's doc line, `ww api tree`, Python `help()`, `ww api check`
  (compatible); the generated and dynamic clients warn when a device returns a variant outside the list, tests fail
  (`tests_common`). Nothing on the device (`docs/api/methods.md#error-lists`, `tests/methods`). Not yet: the GUI
  (`wire_weaver_tool`).
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
  strings inside a resource's type are bounded with `MaxLen` (SW-19).
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
- 📋 **API-34 Credit sinks on the stream protocol** · `v0.5` — positions in units (64 bit), `Ack` / `Query` / `Resend`
  shared with reliable streams, sessions and resume for credit sinks, so a DFU plan continues after a reconnect;
  `SinkCredit` moves into `ww_stream` (`docs/api/stream_reliability.md`).
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
- ⛔ **SRV-8 One crate per role for firmware and host** · `v0.5` — blocked on generated code using absolute paths.
  Agreed direction, instead of putting everything into `wire_weaver` behind features or splitting by
  std/no_std:
  - `wire_weaver` stays lean (shrink_wrap, macros, `ww_version`, result types) since every API crate and
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
- 🚧 **CLT-10 Trait calls that work on any device** · `v0.5` — serial, user label, a CLI that works with all devices.
  `ww list` with filters and USB interface string metadata are done, and global trait calls on any device whatever
  its API version: `ClientConfig::traits_only()`, `DynClient::global_attachment()` (served through API-20,
  `tests/cross_version`) and `ww dfu reboot`. Missing: CLI commands for the other `ww_stdlib` traits (firmware and
  board info, GPIO, UART, ...).
- 📋 **CLT-11 Typed property handles** · `v0.5` — `RwProperty`/`RoProperty`/`WoProperty` so that
  `.prop_name().read()`, `.prepare_write(1).blocking()`, `.asynch().await` work.
- 📋 **CLT-12 Stream receive timeout** · `v0.5` — and a beat timeout between updates.
- 🔍 **CLT-13 Typed attachments with `TraitMarker`** · `v0.5` — trait attachments exist (`Attachment`,
  `examples/all_gpio`); check whether a typed marker is still wanted.
- 💡 **CLT-14 `no_std` client** (`client = "raw"`) — documented as not working in `ClientModel::Raw`, its test is
  commented out in `tests/traits`. Its generated code serializes arguments into a fixed 128 byte scratch and
  then calls `.to_vec()`, which isn't `no_std`; it should write into a caller-provided buffer instead. The std
  clients (`std_client`, `trait_client`) already serialize with `to_ww_bytes_owned()` and have no size limit.
- 🚧 **CLT-15 Multi read/write/call** · `v0.5.x` — `MultiRead` exists as a prototype; multi write and call are missing,
  and full multi-property support.
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
  on a simulated device (`tests/follow_reboot`, `docs/transport/follow_reboot.md`). Missing: a run against a Pico 2 /
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
- 💡 **TR-10 One-shot connect** — no link setup at all: the host sends the API hash and its requests in one go; when
  the hash matches, the device runs them right away, else it answers with its hash and drops them and the normal setup
  follows. With message aggregation both arrive in one packet or frame, so a request over LoRa or LTE-M costs no extra
  round trip (introspection skipped by hash, CLT-7). Open: replayed and duplicate packets (CLT-17), which requests may
  run unannounced, how the hash is versioned. Needed by `ww_dfu` updates over slow links.
- 💡 **TR-11 MQTT / store-and-forward transport** — devices offline most of the time pick up a retained message on
  wake (an update plan, config), replies queued on the broker. Look at MQTT-SN and LwM2M first.

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
  DER-12 takes over. Moving the stdlib crates' errors to it is still to do.
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
  prefixes and storage, `UVlq32` / `UVlq64` storage (STD-13). `docs/std_library/ww_si.md`,
  `docs/std_library/ww_numeric.md`. Zero-overhead typed quantities (`Volt<u16, Milli>`) are not part of
  it, see `docs/std_library/io_family.md`.
- 🚧 **STD-6 `ww_gpio_hl`** — usable, some APIs are still missing.
- 🔍 **STD-7 Interrupt left enabled in `ww_gpio_hl`** · `v0.5` — the falling edge helper documents that it leaves
  interrupts enabled; decide whether that is the intended fix or a bug.
- 🚧 **STD-8 Prototypes**, to be completed later: `ww_can_bus`, `ww_i2c`, `ww_uart`, `ww_counters`, `ww_indication`,
  `ww_ll`, `ww_log_bare_metal`. Known gaps are `TODO`s in their sources (e.g. SMBus and 7-bit addresses in
  `ww_i2c`, capabilities and properties in `ww_can_bus`).
- 💡 **STD-9 Stubs**, for later: `ww_spi`, `ww_board_info`, `ww_firmware_info`, `ww_user_info`, `ww_uid`.
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
- 🚧 **STD-13 IO trait family** · `v0.5` — `ww_gpio`, `ww_uart` (+ `UartMonitor`), `ww_i2c` (+ `I2cMonitor`,
  10-bit addresses), `ww_spi` (+ `SpiMonitor`), `ww_adc` (`Adc`, `AdcStream`, `AdcCapture`), `ww_pwm`, `ww_onewire`,
  `ww_timebase` (`Clock`), `ww_logic_analyzer`, `ww_signal_generator`; `ww_si` values and timestamps, plain error
  enums with `InUse` / `NoIoLevel` / `Refused` / `Unsupported`. Design approved 14 Oct
  (`docs/std_library/io_family.md`); skeletons of every crate written, `ww_i2c` with `I2cWatch` (autonomous
  reads) and 10-bit addresses (SW-48); filled in: `ww_gpio`, `ww_uart`, `ww_i2c`, `ww_spi`
  (full rustdoc, mock-server tests in `tests/io_<x>`, docs pages `docs/std_library/ww_<x>.md`). Stream loss and
  timestamps wait for ww-stream-reliability, error evolution for ww-errors-plan.
- 📋 **STD-21 `ww_logic_analyzer`** · `v0.5.x` — `LogicAnalyzer` (triggered capture into RAM), `LogicStream`;
  `Raw` and `Transitions` encodings (SW-31). Design pass first (STD-13).
- 📋 **STD-22 `ww_signal_generator`** · `v0.5.x` — `PatternGenerator` (digital, replays analyzer captures),
  `WaveformGenerator` (DAC). Design pass first (STD-13).
- 💡 **STD-10 Device state machine trait** like CANopen.
- 💡 **STD-11 Register description registry** — WireWeaver for describing device registers.

## Examples (`EX`)

- ✅ **EX-1 Host examples**: `blinky` (+ evolved API), `all_gpio`, `uart`, `blinky_py` (hand-written pyo3 wrapper).
- ✅ **EX-2 Serialization only**: `minimal_shrink_wrap`, `compare_wire_formats`.
- ✅ **EX-3 MCU examples**: nucleo_g0b1re, nucleo_h743zi2, usb_stm32h725ig, mcu_qemu, rp2 (USB, RTT, WebSocket and UDP
  over NCM).
- ✅ **EX-4 NCM UDP example**: `examples_mcu/rp2` `ww_udp_ncm`, host side `blinky_udp`.
- ✅ **EX-5 Project layout** of paired `<name>_api` / `<name>` crates (`docs/api/folder_structure.md`).
- ✅ **EX-14 cnt counters in MCU examples**: `cnt` dependency, `-Tcnt.x` and `runner = "cnt run --chip ..."` on every probe-rs board (rp2, nucleo_g0b1re, nucleo_h743zi2, usb_stm32h725ig); `unhandled_exceptions` counter in `DefaultHandler`. `mcu_qemu` has no probe, so no counters.
- 📋 **EX-6 Example without async** · `v0.5` — `ww_device::blocking::Server` exists, no example uses it.
- 📋 **EX-7 Example with two media** · `v0.5`.
- 📋 **EX-8 NCM WebSocket + UDP example** · `v0.5` — both media on one stack, sharing the backend.
- 💡 **EX-9 WebSocket over Ethernet example** for some board · `v0.5.x` — the `ws` medium already runs on
  `embassy-net`, so Ethernet needs no transport work, only an example.
- 📋 **EX-10 Reconnect demo** · `v0.5.x` — keep reconnecting with streams and UI feedback; `disconnect_keep_streams`.
- 📋 **EX-11 Multi-property observe demo** · `v0.5.x` — timers and previous values, analog and digital signals, a
  dynamic rate for each property.
- 📋 **EX-12 Hubris based example** with sync code · `v0.5.x`.
- 📋 **EX-13 Advanced example / project template**.

## Documentation (`DOC`)

- ✅ **DOC-1 Docs site** (zensical): wire format, derive macro, showcase, API, evolution rules, transports, CLI,
  Python, std library pages.
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
- 💡 **TEST-16 Host queue on a slow VM** — does it lead to fewer or no missed packets?

## Dropped and superseded

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
