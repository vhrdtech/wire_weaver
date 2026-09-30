## Unreleased

### ⚠️ Breaking

* `CompactVersion::global_type_id: UNib32` is now `gid: GlobalTypeId`.
* `Debug` for `FullVersion` prints `crate@version` instead of `crate version`.

### 🚀 Features

* `ApiHash` and `ApiHashPair` (hash of the API with and without docs).
* `VersionTriplet`, `CompactVersion::new()`.
* `FullVersionOwned::is_protocol_compatible()`, `filename_friendly()`.
* `Copy` for `Version` and `FullVersion`, optional `serde` support.

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