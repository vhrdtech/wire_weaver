# Changelog

## Unreleased

### 🐛 Fixes

- `Resolver` scans `CARGO_HOME`'s git checkouts once, not on every lookup (`cargo::Fetched`); the checker's tests ran 10x faster (EVO-80).

### 🚀 Features

- `Resolver` (EVO-80): the chain in the order sources are added, `global()` for hosts (with `fetch`), `local()` for the checker; `cargo::locate_version`; `fetch` from crates.io or GitHub, bounded and checked, misses remembered.
- `build::workspace_set` (EVO-80): a build script writes the `Set` of every crate in its lock that has `api_snapshots/`; `cargo::CargoSources::locked_package_dirs`.
- New crate (EVO-80): snapshots resolved by crate name and version through embedded `Set`s, the cache `~/.wire_weaver/snapshots/`, Cargo's sources and a fetch; every file checked against its SHA-256 header and its crate version; `file`, `dir`, `cache`, `limits`, `Set`.
