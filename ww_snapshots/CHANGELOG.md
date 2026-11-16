# Changelog

## Unreleased

### 🚀 Features

- `Resolver` (EVO-80): the chain in the order sources are added, `global()` for hosts (with `fetch`), `local()` for the checker; `cargo::locate_version`; `fetch` from crates.io or GitHub, bounded and checked, misses remembered.
- New crate (EVO-80): snapshots resolved by crate name and version through embedded `Set`s, the cache `~/.wire_weaver/snapshots/`, Cargo's sources and a fetch; every file checked against its SHA-256 header and its crate version; `file`, `dir`, `cache`, `limits`, `Set`.
