## Unreleased

### ⚠️ Breaking

* `TypeOwned::is_unsized()` now means `ElementSize::Unsized` (the value is prefixed with its size when written as a
  field): true only for Unsized structs and enums and `Box`. It was also true for strings, and for arrays, tuples,
  `Option` and `Result` containing Unsized types, which are never size-prefixed.
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

### 🚀 Features

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

### 🐛 Fixes

* Dynamic serialization (`ValueOwned::ser_shrink_wrap_dyn()`, `ser_shrink_wrap_vec_dyn()`, `des_shrink_wrap_dyn()`)
  produces and reads the same bytes as `#[derive_shrink_wrap]`: numbers, strings, `Vec`, arrays, tuples, `Result` and
  ranges were silently not written, `Box`, ranges and `u1`..`u64` / `i2`..`i64` were not read, size prefixes were
  written and expected for the wrong types, enum variants were looked up by position instead of discriminant.
  A named field missing from a value is written as `None` if it is an `Option`, or as its `#[default]`; fields with a
  default are read as such when data ends. Numbers are range-checked and can be given as any integer variant.
  Type mismatches, unknown fields and variants are reported with the field path, instead of being ignored.
* `ValueOwned::default()` no longer panics for `u1`..`u64` / `i2`..`i64`, and returns an error for numeric types that
  are not supported yet.
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