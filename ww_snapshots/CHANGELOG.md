# Changelog

## Unreleased

### 🐛 Fixes

- The crate walker resolves a dependency that two same-named packages share by its git source: a `Cargo.lock` entry names it without its `#rev`.
- `Resolver` finds an earlier version's snapshot in the `api_snapshots/` of the crate at the version the lock pins (`CargoSources::locate_crate`); the checker no longer needs it in `~/.wire_weaver` (EVO-80).
- `Resolver` scans `CARGO_HOME`'s git checkouts once, not on every lookup (`cargo::Fetched`); the checker's tests ran 10x faster (EVO-80).
- `CargoSources` finds a `[patch]` in `.cargo/config.toml` of any directory above the workspace, as Cargo reads it; a sibling checkout patched from a parent folder had "no source".

### 🚀 Features

- `CargoSources::locked_package_dir`: the directory of one package the lock pins, such as a git dependency's checkout.
- `cache::default_root` honors `WIRE_WEAVER_HOME` (EVO-80), so the cache, `sources.toml` and definitions move with it.
- `Resolver::offline(workspace)` (EVO-80): the embedded sets and the sources a workspace's `Cargo.lock` pins, no cache, no fetch; `Resolver::cache_root()`; `CargoScan::unlocked`, whether a crate the lock doesn't list is looked for in all of `CARGO_HOME` (hosts: yes). `CargoScan` literals need the field.
- `Resolver` (EVO-80): the chain in the order sources are added, `global()` for hosts (with `fetch`), `local()` for the checker; `cargo::locate_version`; `fetch` from crates.io or GitHub, bounded and checked, misses remembered.
- `build::workspace_set` (EVO-80): a build script writes the `Set` of every crate in its lock that has `api_snapshots/`; `cargo::CargoSources::locked_package_dirs`.
- New crate (EVO-80): snapshots resolved by crate name and version through embedded `Set`s, the cache `~/.wire_weaver/snapshots/`, Cargo's sources and a fetch; every file checked against its SHA-256 header and its crate version; `file`, `dir`, `cache`, `limits`, `Set`.
