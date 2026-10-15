## Unreleased

### ⚠️ Breaking

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

## [0.1.1] - 2026-01-07

### ⚙️ Miscellaneous Tasks

* Bump shrink_wrap version to 0.1.2

## [0.1.0] - 2026-01-07

### 🚀 Features

* Almost fully-featured AST