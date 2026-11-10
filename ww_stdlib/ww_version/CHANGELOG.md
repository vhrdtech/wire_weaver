## Unreleased

### ⚠️ Breaking

* `CompactVersion::global_type_id: UNib32` is now `gid: GlobalTypeId`.
* `Debug` for `FullVersion` prints `crate@version` instead of `crate version`.

### 🚀 Features

* `WireVersion` (EVO-24): an API's wire version, `w2.3` (line 2, revision 3), apart from the crate version. `Display` and `FromStr` in that form only: a bare `2.3`, a three-part `w2.3.0` or a crate version like `0.2.0` is refused with a `WireVersionError` that says why. Ordered by line then revision, `same_line()`, `next_revision()`, `next_line()`; two `UNib32`s on the wire (`w2.3` is one byte).
* `WireVersion::is_design_line()` (EVO-41): line 0, `w0`, the line of an API still being designed.
* `ImageFacts` (SRV-56): what a firmware image chose at build time and its API doesn't show, sent in `ww_link`'s `DeviceInfo` outside the API hash. First field `no_observe_samples`.
* `CompactVersion::is_protocol_compatible()`, same rules as `Version::is_protocol_compatible()` plus the same
  global type id.
* `ApiHash` and `ApiHashPair` (hash of the API with and without docs), `Display` for `ApiHashOwned` (hex).
* `VersionTriplet`, `CompactVersion::new()`.
* `FullVersionOwned::is_protocol_compatible()`, `filename_friendly()`.
* `Copy` for `Version` and `FullVersion`, optional `serde` support.
* Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.

## [0.1.1] - 2026-01-07

### ⚙️ Miscellaneous Tasks

* Bump shrink_wrap version to 0.1.2

## [0.1.0] - 2026-01-07

### 🚀 Features

* Version - major.minor.patch-pre+build (no_std)
  * VersionOwned - alloc variant (only with std feature)
* FullVersion - crate name string + Version
  * FullVersionOwned - alloc variant (only with std feature)
* CompactVersion - global type ID + major.minor.path
* Interop with semver crate (only with std feature)