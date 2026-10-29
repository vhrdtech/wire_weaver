## Unreleased

### ⚠️ Breaking

* `units::CanonicalUnit` has a `kind: units::UnitKind` (`Plain`, `Named`, `RatioOf(DimOwned)`, STD-68), worked out by `exact_units`' rules (`Kind::mul` / `div`): `Per<Volt, Volt>` is a ratio of voltage, `Per<Volt, Ampere>` plain, `Ohm` a resistance. `==` and `factor_to` need the same kind, so a voltage gain no longer converts to a ratio, nor V/A to Ω. Struct literals of `CanonicalUnit` add `kind`.
* `ItemStruct::transparent` (DER-18) and the new marker definition (INT-13) are one appended field, `extra: Option<RefBox<ItemStructExtra>>` (`Option<Box<ItemStructExtraOwned>>` owned, `#[default = None]`), with `struct ItemStructExtra { transparent: bool, marker: Option<MarkerDef> }`: a plain struct writes the single 0 bit it wrote before either existed, so its bytes and its signature stay as they were (a fifth flag after `sparse`, `path`, `instance_of` and `transparent` spilled into a new nibble and moved every struct's signature, breaking deployed devices that refer to stdlib definitions by signature). Read through `ItemStructOwned::is_transparent()` and `ItemStructOwned::marker()`, build with `ItemStructOwned::extra_of(transparent, marker)`; users of `.transparent` switch to `is_transparent()`, struct literals of `ItemStructOwned` set `extra: None` instead of `transparent: false`. Properties a struct gets later go into `ItemStructExtra`. Signatures leave the marker definition out (like `ItemEnum::future_version`): `ww_si::Second` and every trait using it keep their signature. A test pins a plain struct's signature.
* `ItemStructExtra::marker` (INT-13): what a unit, scale or bounds marker means, so a client knows the units of any crate without a built-in table. New `enum MarkerDef { Unit(UnitDef), Scale(ScaleDef), Per, Times, Sqrt, Pow10, Pow2, Rational, FullScale, FullScaleMax, Unbounded, Range, RangeU64, RangeDec }` (evolvable, `ww_repr = unib32`), `struct UnitDef { symbol, dim: exact_units::Dim, magnitude: exact_units::Magnitude }` and `struct ScaleDef { symbol, magnitude }`: the exact definition, the same bytes for equal ones. An older reader skips the appended field; bundle bytes and snapshot hashes change (snapshots saved again). `ww_self` depends on `exact_units` (feature `shrink_wrap`).
* `MarkerDef::Origin(OriginDef)` and `MarkerDef::Angle`, appended (INT-13, STD-64): the origin of `Point`s, `struct OriginDef { unit: UnitDef, symbol, reference: Option<MarkerRef>, offset: exact_units::Offset, modulus: Option<Magnitude>, step: Magnitude, shown: Offset }` (all of `exact_units::OriginDef`, an origin declared `above` another written out whole) and `struct MarkerRef { crate_id, name }`. `units::origin_of(ty, bundle)` resolves the `O` of a `Point<O, F>`, `Angle<U>` with its turn in `U`; `units::marker_name_of`.
* `MarkerDef::Wrapping` and `MarkerDef::Saturating`, appended after `Angle` (STD-63, INT-13): the bounds markers whose arithmetic wraps modulo 2^bits of the storage (`Bam16`, `Bam32`) or stops at its ends (`Q15`, `Q31`, `UQ16_16`).
* `ItemEnum` gets `future_version: Option<FutureVersion>` (`#[default = None]`, EVO-10), new `enum FutureVersion { Off, Variant { ident } }`: whether a reader gets a variant from a newer version as the enum's `#[future_version]` catch-all (SW-47), so the evolution checker and the client's compat check know whether an appended variant is compatible. `None` in a model saved before EVO-10 (treated as no catch-all by the checker). Left out of signatures, so definitions saved before keep theirs. The dynamic decoder (`Value`) reads a newer variant of an enum with `Variant` as `<unknown N>` (also for `sized` / `self_describing` enums with a written catch-all, as the derive does), fails with `Off`, and keeps reading an `Unsized` enum leniently with `None`. Struct literals of `ItemEnumOwned` need the new field.
* `Type::Delta`, `Type::DeltaOfDelta` and `Type::XorFloat` are removed (shrink_wrap dropped the types, SW-31/SW-32
  are `Series` codecs now): an API with such a field describes it as `Type::Series` of the number. The variants after
  them (`Instance`, `Param`, `Series`) move down by three discriminants, so introspection data written before can't be
  read by this version and the other way round (devices and hosts are rebuilt together); snapshots of
  `ww_logic_analyzer` and `ww_signal_generator` saved again.
* `ApiItemKind::Property` gets `batched: Option<Batching>` (`#[default = None]`, API-39, `ro+observe+batched`): a
  host may observe every sample of the property, in batches, and its observe slot is a `Batched<T>`. `Batching`
  gets `SharedClock` (API-49), the form of properties: the samples of a level share the `sample_times` of its
  update; a stream with it is malformed (`StreamFnRef::wire_item_ty` errors). Struct literals of
  `ApiItemKindOwned::Property` need the new field.
* `ApiItemKind::StreamFn` gets `batched: Option<Batching>` (`#[default = None]`, API-41), `enum Batching { Timed,
  Untimed }`: a batched stream's events carry one `Series<Timed<T>>` (`Series<T>` untimed) each. A plain batched
  `stream!` is a `StreamFn`. `StreamFnRef::batched`. API hashes of APIs with streams change; struct literals of
  `ApiItemKindOwned::StreamFn` need the new field.

* `ApiItemKind::StreamFn` gets `batch_item_ty: Option<Type>` (`#[default = None]`, API-41): `Timed<T>` isn't named
  anywhere `ty` alone would reach, so a dynamic decoder (the `ww` CLI, Python) couldn't otherwise find its
  definition to decode a batched timed stream's `Series<Timed<T>>` events. `None` for `Batching::Untimed` (its wire
  item is `Series<T>`, already `ty`, no extra bytes) and for a stream that isn't batched. `StreamFnRef::batch_item_ty`
  / `StreamFnRef::wire_item_ty()` (what one event actually carries). `Timed`'s own definition is a known type from
  the `ww_si` snapshot, so a device only adds one `Instance` reference per batched timed stream (a handful of
  bytes), not `Timed`'s fields.

* `ItemStruct::instance_of` / `ItemEnum::instance_of` (`#[default = None]`, `InstanceOf { type_idx, args }`, CLT-31):
  the generic definition and arguments a concrete definition was made from, set by `generics::instantiate` (and
  `substitute`), so a dynamic client can tell a value of `Quantity<units::Volt>` from its concrete `Quantity<Volt>`.
  Host-derived: always `None` in a bundle from a device or a file, `generics::prepare` rejects one that carries it;
  `validate` checks it as a use of the generic definition, signatures clear it, the visitors walk its arguments. Its
  `None` flag is still written (under a bit per definition), so every signature and API hash changes once and the
  snapshots are saved again. Struct literals of `ItemStructOwned` / `ItemEnumOwned` need the new field
  (`instance_of: None`).
* `ItemStruct::path` / `ItemEnum::path` (`#[default = None]`, API-46): the modules from the crate root to where
  other crates name the definition (`Some(["units"])` for `ww_si::units::Volt`, `None` at the root). A definition's
  identity in its crate is its path and name, `qualified_name()` (`units::Volt`, also the free `qualified_name`):
  `SkippedFullVersion::type_name` holds it, and signature resolution, `inline_skipped*` and the definition cache look
  definitions up by it (cache file names write `::` as `.`). The path is part of signatures, so every signature and
  the introspection bytes change (snapshots are saved again); struct literals of `ItemStructOwned` /
  `ItemEnumOwned` need the new field.
* `Type::Series(RefBox<Type>)` appended (SW-60, CLT-29): `Series<'i, T>` / `SeriesOwned<T>` in APIs, and
  `Field::series_opaque` (`#[default = false]`) for a field marked `#[series(opaque)]`. Exhaustive matches on
  `Type`/`TypeOwned` need a new arm, struct literals of `FieldOwned` the new field (`false`); the introspection
  format changes with them (snapshots are saved again). `ValueOwned::des_shrink_wrap_dyn` reads a `Series` as a
  `Vec` of its items, by the rules of the generated decoder (column by column, restarts, evolution, opaque fields);
  `ValueOwned::des_series_columns_dyn` gives the columns themselves (`DecodedSeries`, `SeriesColumn`), without items:
  an enum's column lists the variants with items only (`VariantColumns::variant_idx` is the position in the
  definition), so a wide enum from a device costs the items, not fields x variants, and the restarts of a struct's
  fields are kept as it goes, not re-walked per field.
  Writing a `Series` from a dynamic value is not supported yet (an error).
* The dynamic decoder bounds memory, not only values (P2401#17oct-1125): a decode takes at most 64 MiB
  (`DECODE_MAX_BYTES`), each value its `size_of`, strings and names their bytes; a larger value is an error. Before,
  64 KB of data could become 4 M values (about 400 MB).
* Self-referential types (API-44): `generics::validate` (and `prepare`) accepts definitions that refer to themselves,
  directly or through others, when none of them has parameters and every cycle goes through a `Box`, a `Vec` or a
  `Series`; a cycle through a generic definition, or one with none of these on it (`struct S { s: Option<S> }`), is
  an error. Walks over types stop at a definition they are already in: `is_lifetime`, `human_definition` (names it
  inside itself), `ValueOwned::default` (the first enum variant that doesn't contain the enum; a type each value of which
  contains another one is an error). Values of such types decode as deep as their bytes say, bounded by the new
  public `MAX_DECODE_DEPTH` (288 nested calls, about 70 levels of a recursive value, at most about 1.25 MiB of
  stack in a debug build), up from 200. The decoder's stack frames are smaller.

* Generic types (API-43), 0.3.0: `ItemStruct` and `ItemEnum` have `generics` (type and const parameters,
  `GenericParam` / `GenericParamKind`); a generic definition's fields use `Type::Param(idx)` and `Length::Param(idx)`,
  a use is `Type::Instance { type_idx, args }` with `GenericArg::{Type, Const, ConstParam}`. `Type::Array::len` and
  `Type::MaxLen::max_len` are a `Length` (`Fixed(n)` or `Param(idx)`; `Length::fixed()` for the number). The
  introspection format changes with it: a host and a device must be built with the same ww_self (an older host can't
  read a newer device's data and the reverse), and every type and trait signature changes, so snapshots and the
  definition cache from an older ww_self don't match (snapshots are saved again).
* Dynamic code works on concrete types: `generics::prepare(&mut bundle)` (validate, instantiate, validate again)
  replaces every instance with a concrete definition (`Timed<Ampere>`); decoding, encoding and defaults of an
  instance or parameter are an error otherwise.

* `ApiItemKind::PositionedStream` is replaced by `StreamFn { ty, delivery, args, return_ty }` (API-31): a stream
  function, `#[stream(T, kind)] fn name(args) -> R` or `#[counted]` / `#[reliable] stream!`. `Delivery` gets
  `Plain` as its first variant. A plain `stream!` stays `Stream { is_up: true }` with its bytes.
  `ApiItemKindOwned::as_stream_fn()` sees every stream (device to host) as a stream function (`StreamFnRef`),
  `delivery()` returns `Plain` for plain streams.
* `Type::Delta`, `Type::DeltaOfDelta` and `Type::XorFloat` appended (SW-50), each with the `NumericBaseType` of
  its elements: the compressed sequences of shrink_wrap (SW-31, SW-32) in APIs. Exhaustive matches on
  `Type`/`TypeOwned` need new arms; introspection data of an API that uses them can't be read by an older ww_self.
  `ValueOwned` reads and writes them as a `Vec` of numbers, with the codecs of shrink_wrap (any integer variant in
  range of the element type is accepted).
* `ApiItemKind::PositionedStream { ty, delivery: Delivery }` appended (API-30): a counted or reliable stream,
  `Delivery::{Counted, Reliable}`. Exhaustive matches on `ApiItemKind` need a new arm; plain streams stay `Stream` and
  keep their bytes. `ApiItemKindOwned::as_stream()` treats it as a stream, `delivery()` tells which kind.
* Per-method error lists (API-32): `ApiItemKind::Method` gets `errors: Option<RefVec<ErrorVariant>>`
  (`#[default = None]`, older bundles and snapshots read as before), the `#[errors(..)]` list of the method's trait,
  each entry a variant of its error enum with, for `Common(InUse, Refused)`, the variants of the enum it holds. Rust
  code matching `Method { args, return_ty }` needs `..`, struct literals the new field (`None`). The `errors` module
  has what tools share: `fmt_error_list()` (as written), `error_list_docs()` (each entry with its variant's doc line),
  `error_list_contains()` (whether a dynamic error value is in a list), `error_value_name()`, `method_err_ty()` and
  `ApiItemKindOwned::error_list()`.
* Error lists of stream functions and properties (API-32): `ApiItemKind::StreamFn` and `ApiItemKind::Property` get
  `errors: Option<RefVec<ErrorVariant>>` too (`#[default = None]`, older bundles read as before); a stream function's
  list is about the `Err` of `return_ty`, a property's about `write_err_ty`. Rust code matching these variants without
  `..` needs it, struct literals the new field (`None`); `StreamFnRef` gets `errors`. `ApiItemKindOwned::error_list()`
  returns the lists of all three, new `ApiItemKindOwned::err_ty()` the error type a list is about.
* Optional items (API-16): `ApiItemKind::Trait` gets `omitted: Option<RefVec<UNib32>>`, `ApiItem` gets
  `optional: Option<Optional>` and `ApiBundle` gets `root_omitted`, all `#[default = None]`, so older bundles and
  snapshots read as before. Rust code matching `Trait { trait_idx }` needs `..`, struct literals of `ApiItemOwned`,
  `ApiBundleOwned` and `ApiItemKindOwned::Trait` the new fields (`None`).
* `Type::MaxLen { max_len, ty }` appended (SW-19): a list or string with a maximum length, same bytes as `ty`.
  Exhaustive matches on `Type`/`TypeOwned` need a new arm; introspection data of an API that uses `MaxLen` can't be
  read by an older ww_self. `TypeOwned::max_len()` returns the bound, `is_byte_slice()` looks through it, and
  `ValueOwned` serialization and deserialization fail on a value longer than the bound.
* `PropertyAccess` and `Repr` are `self_describing` instead of `sized`: their variants differ in size, which a `sized`
  enum can no longer have (SW-35). Same bytes, only `ELEMENT_SIZE` and the type signatures change.
* `TypeOwned::is_unsized()` now means `ElementSize::Unsized` (its size is recorded in the FIFO of sizes at the back of
  the buffer when written as a field): true only for Unsized structs and enums and `Box`. It was also true for
  strings, and for arrays, tuples, `Option` and `Result` containing Unsized types, whose size is never written.
* `ww_self::visitor` replaced by `ww_self::visit` (read-only `Visit<'ast>`) and `ww_self::visit_mut` (`VisitMut`),
  in the style of `syn::visit`. Each hook's default implementation calls the free function of the same name, so an
  override chooses whether to descend. Migration: `visitor::visit_api_bundle_mut(&mut b, &mut v)` →
  `v.visit_api_bundle(&mut b)`; `visit_doc` → `visit_docs`; `visit_method`/`visit_property`/`visit_stream` →
  match on the node in `visit_api_item_kind`; `visit_level`/`visit_item` → `visit_api_level`/`visit_api_item`.
* `VERSION` (stored in `ApiBundle::ww_self_version`) follows the crate version, it was a fixed `0.1.1`, so saved
  bundles and introspection data record the format they were written in. It is part of the API hash, which changes
  for every API.
* Trait and type signatures no longer depend on `VERSION` (the canonical bundle carries `0.0.0`), so a device and a
  host built with different ww_self versions agree on them. All signatures change once, saved snapshots have to be
  re-saved (`just save-snapshots --force`).
* `ItemStruct` gets `sparse: bool` (`#[default = false]`) and `Field` gets `slot: Option<UNib32>`
  (`#[default = None]`) (TOOL-11): a `#[derive_shrink_wrap(sparse, ..)]` struct (DER-13) is no longer described as a
  plain struct of `Option`s, so a dynamic decoder knows to read it as an `OptionAnyVec` instead of sequential
  fields. `slot` is the field's `#[slot = N]`, or the previous field's slot + 1 (0 for the first), mirroring
  `shrink_wrap_derive`. Struct literals of `ItemStructOwned`/`FieldOwned` need the new fields, every signature
  changes once (saved snapshots re-saved with `just save-snapshots`).

### 🚀 Features

* `units::scale_symbol_of` (a scale marker's symbol, `Ki`, empty for `Pow2<E>` and other unnamed scales), `units::storage_of` (a wrapper's number type), `expr::int_value` (an integer as a number of a given type, range checked) and `expr::base_name` (`u32`, `ub4`) are public, for clients converting into `exact_units` wrappers (CLT-54).
* `FutureVersion::Variant::bits: Option<u8>` (`#[default = None]`, DER-25): the width of a written `#[future_version]` variant's field when narrower than `u32` (`Unknown(u16)`), `None` for `u32` and in models written before, whose bytes don't change. The dynamic reader fails on a discriminant beyond it, as the derive does.
* The dynamic encoder writes a value `<unknown N>` (as the dynamic decoder reads an enum's `#[future_version]` catch-all) back as the bare tag `N`, the bytes the derive writes for `Unknown(N)` (DER-25): a client passes a scale of a newer version (`exact_units::Prefix::Unknown`) on unchanged instead of failing.
* `MarkerDef::ConventionalUnit(ConventionalUnitDef { unit: UnitDef, source, year })`, appended after `Saturating` (STD-72): a unit fixed by a physical property, whose `magnitude` is the value a source prints (`source: "NIST SP 811", year: 2008` for inHg at 32 °F), not its definition. A variant of its own, so a defined unit's `UnitDef` keeps its bytes; a reader from before it reads `Unknown(17)`, a marker it can't convert. `MarkerDefOwned::unit_def()` (the `UnitDef` of either unit variant) and `MarkerDefOwned::provenance()` (`Conventional` or `Defined`).
* `MarkerDef::KindUnit(KindUnitDef { unit: UnitDef, kind })`, appended after `ConventionalUnit` (STD-68): a unit of a kind of quantity of its own (`kind "resistance"` on a `unit!` line: `exact_units`' `Ohm`, `Siemens`), which doesn't convert to or from another unit of its dimension (`Per<Volt, Ampere>`). A variant of its own, so a plain unit's bytes don't change; a reader from before it gets `Unknown` and treats the marker as one it can't convert. `MarkerDefOwned::kind()`, `unit_def()` also for it. Conformance case `model.marker_def.unit_kind`.

* `units::wrapper_of` / `wrapper_of_struct` (CLT-54): the `exact_units` number wrapper a type is (`enum Wrapper { Fixed, Index, IndexFrom1, Seq }` with its type arguments); `units::bounds_of` resolves a bounds marker (`Unbounded`, `Wrapping`, `Saturating`, `Range`, `RangeU64`, `RangeDec`) to `NumBounds { min, max: Option<Bound> }`. The dynamic decoder and encoder (`Value`, so also `explain` and Python) check a wrapper's rules as the derive does: a `Fixed`, `Index` or `IndexFrom1` outside its bounds, or an `IndexFrom1` of 0, doesn't decode or encode (`units::check_wrapper`).
- New `units` module (std, INT-13): `unit_of(ty, bundle)` resolves a unit type through the bundle's marker definitions (named units, `Per`, `Times`, `Sqrt`) to a `CanonicalUnit`: the sorted product of the named units in it (`MarkerName` crate + qualified name, powers doubled for roots), its `dim` and exact `magnitude`; equal when the same units to the same powers (F#-style), `factor_to` converts between units of one dimension, `Display` writes `m·s⁻¹`, `(rad/s)⁻¹·N·m`. `scale_of(ty, bundle)` gives the exact factor of a scale marker (named, `Pow10`, `Pow2`, `Rational`, `FullScale`, `FullScaleMax`).
- `ItemStruct::transparent` (`#[default = false]`, appended, DER-18; now `ItemStructExtra::transparent`, see above): the struct is its one field on the wire (`#[derive_shrink_wrap(transparent)]`), with that field's size class and Series columns; `size` is `UnsizedFinalStructure`, which a reader without the flag decodes to the same bytes inside a parent (only a top-level value of a transparent wrapper around an `Unsized` type differs for it). `ItemStructOwned::transparent_field()`; `TypeOwned::is_unsized`, the dynamic value decoder and encoder (`ValueOwned`) and the dynamic Series reader and writer go through the field. Struct literals of `ItemStructOwned` need the new field.
- `snapshot` module: `encode` / `decode` a saved `ApiBundleOwned` as shrink_wrap bytes, `sha256_hex`, and the `// shrink_wrap sha256: ` header line (`hash_line`, `saved_hash`) that RON snapshots carry so a build can prove the RON round trip lossless.
- `ww_self::Meta` (SRV-34): typed attributes beyond the wire contract, on `ApiItem::meta` and `ApiLevel::meta` (both `#[default = None]`): `claim: Option<ClaimMode>` (`Exclusive`, `Shared`, `Observe`), `shares: Vec<Shares { index, pins, peripherals }>`, `labels: Vec<String>`; `ApiItemOwned::claim_mode(&bundle)` (the `ww_impl!`'s own claim, else its trait's), `ApiItemOwned::label(index)`, `ClaimMode::name`. Their `None` flags are written, so every signature and API hash changes once and the snapshots are saved again; struct literals of `ApiItemOwned` and `ApiLevelOwned` need `meta: None`.
- `ApiLevel::compact_id` (`#[default = None]`, API-20): the trait's id in the `ww_global` registry. Trailing and `None` for user traits, so introspection bytes don't change; signatures leave it out, assigning an id changes no signature. Snapshots record it.
* Dynamic `Series` writer (CLT-29): `ValueOwned::ser_shrink_wrap_dyn` writes a `Series` (top level or inside
  another value) from a `ValueOwned::Vec` of its items with `CodecPolicy::SMALLEST`, and `ValueOwned::ser_series_dyn`
  with a given policy: the bytes of `Series::with_policy(items, policy)` (codec choice, restarts, evolvable struct
  heads, opaque fields compared as the typed writer compares them, generics). Limits as for decoding: at most
  `SERIES_MAX_ITEMS` items, `MAX_DECODE_DEPTH` levels, `ENCODE_MAX_VALUES` column values. Writing a `Series` was an
  error before.
* `SeriesColumn::from_items` (API-41): the same tree `ValueOwned::des_series_columns_dyn` reads from `Series`
  bytes, built instead from already-decoded items (no `Budget`: nothing untrusted to bound, every item already
  decoded under its type). For code with items, not bytes to read, e.g. a batched stream's items
  (`DynBatching::decode`'s `Vec<BatchItem<ValueOwned>>`), which `wire_weaver_py`'s `recv_batch()` pivots into numpy
  columns this way instead of a per-item Python conversion. `fuzz_series::check_items` now also checks it against
  the byte decoder's own tree, for every existing and fuzzed case.
* `generics` module (API-43): `validate` bounds what walks over a bundle's types rely on (indices, argument count
  and kinds, parameters only where they are defined, no definition referring to itself, nesting at most `MAX_DEPTH`),
  `instantiate` adds at most `MAX_INSTANCES` concrete definitions of at most `MAX_NODES` type nodes, `substitute`
  fills in one instance, `params_human_name` writes `<T, const N: u64>`. `TypeOwned::human_name_in` names
  parameters. Introspection data is untrusted: malformed is an error, never a panic.

- `definition_cache` (EVO-9): definitions of skipped traits and types by signature in `~/.wire_weaver/definitions/`
  (`store`, `load`, `default_dir`, `Kind`). An entry is the canonical bundle's bytes as hashed, checked against the
  signature when loaded, so entries written by an older `ww_self` still read.
- `inline::inline_skipped_or_cached` (EVO-9): like `inline_skipped`, a definition the snapshots don't have under its
  signature is taken from the definition cache; returns `Inlined` (`not_inlined`, `from_cache: Vec<FromCache>`).
- `signature::canonical_type_bytes` / `canonical_trait_bytes` (the bytes a signature hashes) and `signature_of`.
* `TypeOwned::is_common_error()`, `ValueOwned::common_error()` (STD-16): recognise `ww_error::CommonError` by its
  type (also when its definition was left out), and find it in an API's error value. `ValueOwned::doc_message()`: the
  first doc line of an enum value's variant with `{field}` / `{field:#x}` filled in, the message tools show.
* `ValueOwned` reads and writes `NumericBaseType::UVlq32` / `UVlq64` numbers (STD-23).
* `ApiItemKind::FlowSink { ty, flow: FlowControl }` for sinks with flow control (`#[credit] sink!(..)`, API-28),
  appended, so existing bundles and signatures are unchanged; `ApiItemKindOwned::as_stream()` for code treating
  streams and sinks of both kinds alike.
* `ValueOwned::des_shrink_wrap_vec_dyn()`: read the fields of an evolvable struct (e.g., method arguments), the
  counterpart of `ser_shrink_wrap_vec_dyn()`.
* `inline::inline_skipped()`: put definitions of skipped traits and types back into a bundle, found with a
  `signature::Resolve` callback (e.g. in crate snapshots), together with everything they refer to. Only definitions
  with a matching signature are put back, the rest are returned as `inline::NotInlined`.
* `signature::trait_signature()` and `signature::type_signature()`: hash of a trait or type definition with docs
  and everything it refers to, independent of where it is in the bundle, for the `signature` of `Skipped*`
  locations. Skipped definitions are looked up with a `signature::Resolve` callback (e.g., in crate snapshots), so the
  signature is the same as if nothing was skipped. `std` feature now depends on `sha2`.
* Read-only `visit::Visit<'ast>`, which can keep `&'ast` references into the tree.
* `signature::api_hash()`: API hash of a serialized bundle, the same function is used by codegen and by
  `wire_weaver_client` for downloaded bundles.
* New hooks: `visit_type_location`, `visit_api_level_location`, `visit_argument`, `visit_variant`, `visit_fields`,
  `visit_field`.
* `FieldOwned::is_flag()`: the field is a relocated `#[flag]` of the `Option` or `Result` field with the same name.
* `ValueOwned::des_shrink_wrap_field_dyn()` (CLT-15): read one value of a dynamic type from a shared `BufReader`,
  bounded to its own size first when unsized (as a field of an evolvable struct is read), for item types not known
  ahead of a sequence of several values packed together (e.g. one `EitherAnyVec` slot, through
  `EitherAnyVecIter::next_with`) - `des_shrink_wrap_dyn()` only works for a whole top-level response.
* `ValueOwned` (de)serialization of a `sparse` struct (TOOL-11): `TypeOwned::Struct` whose `ItemStruct::sparse` is
  set reads and writes its fields as an `OptionAnyVec`, one slot per field (`Field::slot`) read with
  `OptionAnyVecIter::read_slot_with()`, instead of the sequential field encoding. The field's value is still a plain
  `ValueOwned::Option`; only the wire layout differs. Dynamic tools (the `ww` CLI, `wire_weaver_py`, the observe host
  client) now decode a sparse struct correctly, e.g. a trait level's generated `<Trait>Update` (SRV-18).

### 🐛 Fixes

* `ApiBundleOwned::get_ty` is an error for a type entry that isn't a struct or an enum (P2401): an entry referring
  to itself (`types[i] = OutOfLine(i)`) sent `is_unsized`, value encoding and defaults into endless recursion on an
  untrusted bundle, before `prepare` checked it.
* `TypeOwned::human_definition` is bounded in bytes too (16 MB, counting nested text), not only in types: 2000 uses of
  a type with a 100 KB name wrote 200 MB.
* Dynamic decoding counts field and variant names into its value budget (a value per 32 bytes) and the variants
  looked through to find the one read (a value per 16): each value has its own copy of its name, so a 100 KB field
  name in a device's types made a 2-byte message decode into 3 GB of names (a `Vec` of zero-bit structs). Relocated
  flags are found in a map, not by a scan per field.
* Names of instances of generic types are bounded (`generics::MAX_NAME_LEN`, 1024 bytes): a name repeats the names of
  its arguments, so a few hundred bytes of introspection data with nested instances named types with gigabytes and
  `generics::prepare` exhausted the host's memory; now an error. New `TypeOwned::human_name_bounded`;
  `human_name` of an out-of-line reference to something that is not a struct or an enum is an error instead of
  following it (it looped forever on a reference to itself in a bundle that wasn't validated).
* Dynamic decoding (`ValueOwned::des_shrink_wrap_dyn` and friends) and `ValueOwned::default` are bounded: a value
  budget per input byte and a nesting limit, so an array of empty tuples or deeply nested options in a device's types
  is an error instead of exhausting memory or the stack. `human_definition` writes at most
  `HUMAN_DEFINITION_MAX_NODES` types, and no longer doubles the `{` of a struct.

- `ValueOwned` deserialization of a field with `#[default]` falls back to the default only when the field starts at
  the end of the data and runs out of it (SW-15), any other error is reported. An unknown discriminant of an
  `Unsized` enum (a variant from a newer version, SW-16) reads as variant `<unknown N>` without fields instead of
  failing.
* Dynamic serialization (`ValueOwned::ser_shrink_wrap_dyn()`, `ser_shrink_wrap_vec_dyn()`, `des_shrink_wrap_dyn()`)
  produces and reads the same bytes as `#[derive_shrink_wrap]`: numbers, strings, `Vec`, arrays, tuples, `Result` and
  ranges were silently not written, `Box`, ranges and `u1`..`u64` / `i2`..`i64` were not read, sizes were
  written and expected for the wrong types, enum variants were looked up by position instead of discriminant.
  A named field missing from a value is written as `None` if it is an `Option`, or as its `#[default]`; fields with a
  default are read as such when data ends. Numbers are range-checked and can be given as any integer variant.
  Type mismatches, unknown fields and variants are reported with the field path, instead of being ignored.
* `ValueOwned::default()` no longer panics for `u1`..`u64` / `i2`..`i64`, and returns an error for numeric types that
  are not supported yet.
* Dynamic serialization reads and writes relocated `#[flag]`s (`TypeOwned::Flag` fields): the flag is written where
  it is declared, taken from the `Option` or `Result` field with the same name, which is then written without its
  own flag. Flags are not a part of `ValueOwned` fields. `ValueOwned::default()` no longer fails for types with them.
  `TypeOwned::Flag`, `Option` and `Result` docs describe this, instead of a flag stack.
* Visiting a bundle that contains `Skipped*` type or trait locations no longer panics (it hit `todo!()`).
* `TypeOwned::human_name()` names a type whose definition is left out (`TypeLocationOwned::SkippedFullVersion`),
  instead of failing.

### ⚙️ Miscellaneous Tasks

* Moved from `ww_stdlib` into the `wire_weaver` repo root, `repository` now points to `vhrdtech/wire_weaver`.

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.

## [0.1.1] - 2026-01-07

### ⚙️ Miscellaneous Tasks

* Bump shrink_wrap version to 0.1.2

## [0.1.0] - 2026-01-07

### 🚀 Features

* Almost fully-featured AST