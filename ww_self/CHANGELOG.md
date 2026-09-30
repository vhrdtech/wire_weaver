## Unreleased

### ⚠️ Breaking

* `ww_self::visitor` replaced by `ww_self::visit` (read-only `Visit<'ast>`) and `ww_self::visit_mut` (`VisitMut`),
  in the style of `syn::visit`. Each hook's default implementation calls the free function of the same name, so an
  override chooses whether to descend. Migration: `visitor::visit_api_bundle_mut(&mut b, &mut v)` →
  `v.visit_api_bundle(&mut b)`; `visit_doc` → `visit_docs`; `visit_method`/`visit_property`/`visit_stream` →
  match on the node in `visit_api_item_kind`; `visit_level`/`visit_item` → `visit_api_level`/`visit_api_item`.

### 🚀 Features

* `inline::inline_skipped()`: put definitions of skipped traits and types back into a bundle, found with a
  `signature::Resolve` callback (e.g. in crate snapshots), together with everything they refer to. Only definitions
  with a matching signature are put back, the rest are returned as `inline::NotInlined`.
* `signature::trait_signature()` and `signature::type_signature()`: hash of a trait or type definition with docs
  and everything it refers to, independent of where it is in the bundle, for the `signature` of `Skipped*`
  locations. Skipped definitions are looked up with a `signature::Resolve` callback (e.g., in crate snapshots), so the
  signature is the same as if nothing was skipped. `std` feature now depends on `sha2`.
* Read-only `visit::Visit<'ast>`, which can keep `&'ast` references into the tree.
* New hooks: `visit_type_location`, `visit_api_level_location`, `visit_argument`, `visit_variant`, `visit_fields`,
  `visit_field`.

### 🐛 Fixes

* Visiting a bundle that contains `Skipped*` type or trait locations no longer panics (it hit `todo!()`).

### ⚙️ Miscellaneous Tasks

* Moved from `ww_stdlib` into the `wire_weaver` repo root, `repository` now points to `vhrdtech/wire_weaver`.

## [0.1.1] - 2026-01-07

### ⚙️ Miscellaneous Tasks

* Bump shrink_wrap version to 0.1.2

## [0.1.0] - 2026-01-07

### 🚀 Features

* Almost fully-featured AST