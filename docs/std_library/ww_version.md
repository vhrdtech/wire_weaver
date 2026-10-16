---
level: advanced
---

# ww_version

`ww_version` is SemVer for `no_std` targets: a nibble-packed `Version`/`FullVersion` pair that compiles into every
`#[ww_trait]`/`#[ww_api_root]` to identify an API's crate and version on the wire, plus `CompactVersion` for
identifying it even more cheaply once it has a [global type id](../api/addressing.md). It is `no_std` by default
and depends only on `shrink_wrap` and `ww_global` (for `GlobalTypeId`).

Add it to a device API crate with:

```toml
[dependencies]
ww_version = "0.2"
```

## Types

* `Version<'i>` - `major` / `minor` / `patch: UNib32`, optional `pre` / `build: Option<&'i str>` (the `-alpha.1` and
  `+zstd.1.5.0` parts of a SemVer string); `final_structure`. `pre` participates in compatibility checks,
  `build` is metadata only.
* `FullVersion<'i>` - `crate_id: &'i str`, `version: Version<'i>`; `final_structure`. Identifies an API crate by
  name and version, the same two pieces of information that make a crate's identity on crates.io.
* `CompactVersion` - `gid: GlobalTypeId`, `major` / `minor` / `patch: UNib32`; `final_structure`. Stands in for a
  `FullVersion` once the API has a [global type id](../api/addressing.md), for requests made often or over a
  bandwidth-limited link.
* `VersionTriplet` - plain `major` / `minor` / `patch: UNib32`, no crate name; `final_structure`. For comparing
  versions where a name isn't needed.
* `ApiHashPair<'i>` - `no_docs` / `with_docs: ApiHash<'i>`; `final_structure`. Two hashes of a device's
  [`ApiBundle`](ww_self.md), with and without doc strings, so a client can detect *any* change to the API (not just
  ones that bumped the version) while still being able to drop doc strings to save flash and fall back to a cached
  full `ApiBundle` for compatibility checks.
* `ApiHash<'i>` - `hash: RefVec<'i, u8>`; `final_structure`. One hash value, `Debug`-formatted as hex.

Every struct derives `PartialEq, Eq` (except `ApiHash`, no `Eq`) and `Clone`; the borrowed variants also derive
`Copy`. Enabling `std` adds owned (`*Owned`) counterparts (`String` instead of `&str`, `Vec<u8>` instead of
`RefVec`).

## Compatibility checks

`Version::is_protocol_compatible` implements SemVer's own rule: once `major >= 1`, two versions are compatible when
`major` matches; below 1.0, `minor` must match too (pre-1.0 minor is the breaking position, same convention this
project's own crates follow). `pre` is not compared by this rule - it answers "can these talk to each other",
not "are these the exact same build". `FullVersion::is_protocol_compatible` additionally requires the same
`crate_id`, and `CompactVersion::is_protocol_compatible` the same `gid`.

## Reading the current crate's version

`Version`, `FullVersion` and `CompactVersion` all have a `const fn new(..)` (plus `Version::full` for `pre`/`build`
and `CompactVersion`/`VersionTriplet` likewise), but code using WireWeaver's codegen rarely builds one directly.
`wire_weaver::full_version!()` and `wire_weaver::compact_version!(gid)` (from `wire_weaver_derive`) expand to a
`FullVersion`/`Option<CompactVersion>` built from the calling crate's own `Cargo.toml` version (`CARGO_PKG_NAME`,
`CARGO_PKG_VERSION_*`) at compile time. `#[ww_trait]` and `#[ww_api_root]` use exactly these macros to emit a
`<TRAIT>_FULL_GID: FullVersion` and `<TRAIT>_COMPACT_GID: Option<CompactVersion>` constant next to every API trait -
`compact_version!` returns `None` unless the trait was given a global type id (`#[ww_trait(gid)]`).

## On the wire

`Version` packs `major`/`minor`/`patch` as `UNib32` (one nibble each for values below 8) with the two `Option`
presence flags for `pre`/`build` relocated next to them via [`#[flag]`](../serdes/showcase.md#relocating-presence-flags-with-flag),
so `0.1.2` with no pre-release or build metadata is **2 bytes** total; a fully populated version only grows by
exactly the bytes its `pre`/`build` strings need. `final_structure` on every type here means no reverse-length is
written for the type itself when it's the last field of something else - see
[size assumptions](../serdes/size_assumptions.md).

`ww_client_server`'s own `Request`/`Event` model uses `CompactVersion` for the common case of addressing a
global trait by id (2 extra bytes when every number is ≤ 7) and falls back to `FullVersion` only for the one-off
case of calling into an arbitrary crates.io trait by name - see [`ww_client_server`](ww_client_server.md).

## Features

| Feature  | What                                                                                      |
|----------|---------------------------------------------------------------------------------------------|
| `std`    | (default) owned (`*Owned`) type variants, plus `hex`-based `Display` for `ApiHashOwned`     |
| `defmt`  | `defmt::Format` on the borrowed types                                                       |
| `serde`  | `serde::Serialize`/`Deserialize` (owned types for `Version`/`FullVersion`; directly on `CompactVersion`/`VersionTriplet`, which hold no borrowed data) |
| `semver` | `TryFrom<semver::Version>`/`TryInto<semver::Version>` for `VersionOwned`                    |
