# Changelog

## Unreleased

### ⚠️ Breaking

- Declares `rust-version = "1.99"`: cargo of an older Rust refuses the crate.
- `file::parse` refuses a snapshot of another model format than `ww_self::VERSION` before reading it, naming the re-save step (EVO-82); new `file::check_format`. Test `model_format_frozen` fails when the model changes without a new format.

### 🐛 Fixes

- Outside a build, `CargoSources` resolves a crate walked in a dependency's checkout by the lock of the workspace at hand (new field `workspace`, the current directory's by default), not the checkout's own; the evolution checker failed on a fresh `CARGO_HOME` (API-14).
- `Resolver::read_error` names a version whose file existed but could not be read (an old or damaged model format), so a caller can tell that apart from truly having no snapshot; `Resolver::snapshot` answering `None` either way once let codegen inline a dependency whose snapshot was merely unreadable (EVO-82).
- `CargoSources` finds a git dependency in the checkout of the URL its lock names; a repository fetched by https and ssh broke the walk (API-14).
- `CargoSources::locked_package_dir` looks up only the named package, not every crate the lock lists (1.8 s to 0.08 s).
- In a proc macro of a crate built as a dependency, `CargoSources` uses the building workspace's `Cargo.lock`, checked against Cargo's dep-info, not the crate's own (API-14).
- A crate from a git checkout builds again when the build directory keeps dep-info of an older commit; its sibling path members count as pinned (API-14).
- `CargoSources` reads a `[patch]` as Cargo does: `include`d config files, `CARGO_HOME/config.toml`, the build's `--config`; config wins over the manifest (API-14).
- The crate walker resolves a dependency that two same-named packages share by its git source: a `Cargo.lock` entry names it without its `#rev`.
- `Resolver` finds an earlier version's snapshot in the `api_snapshots/` of the crate at the version the lock pins (`CargoSources::locate_crate`); the checker no longer needs it in `~/.wire_weaver` (EVO-80).
- `Resolver` scans `CARGO_HOME`'s git checkouts once, not on every lookup (`cargo::Fetched`); the checker's tests ran 10x faster (EVO-80).
- `CargoSources` finds a `[patch]` in `.cargo/config.toml` of any directory above the workspace, as Cargo reads it; a sibling checkout patched from a parent folder had "no source".

### 🚀 Features

- Reads snapshots of model format 0.4.0, from before `Meta::effects` (API-58), hash still checked; new `file::fields_missing_from`.
- `file::parse` reads snapshot files of model format 0.4.0 (API-58); a file whose re-encoded bytes miss its hash is refused with the re-save step.
- `cargo::git_checkout_dir_name`: the `git/checkouts/` directory Cargo makes for a git URL.
- `rustc::RustcBuild`: the build a proc macro runs in, from rustc's arguments; `CargoSources::rustc` sets it (API-14).
- `RustcBuild::cargo_cwd` and `cargo_configs`: where the build's Cargo runs and its `--config` arguments.
- `CargoSources::locked_package_dir`: the directory of one package the lock pins, such as a git dependency's checkout.
- `cache::default_root` honors `WIRE_WEAVER_HOME` (EVO-80), so the cache, `sources.toml` and definitions move with it.
- `Resolver::offline(workspace)` (EVO-80): the embedded sets and the sources a workspace's `Cargo.lock` pins, no cache, no fetch; `Resolver::cache_root()`; `CargoScan::unlocked`, whether a crate the lock doesn't list is looked for in all of `CARGO_HOME` (hosts: yes). `CargoScan` literals need the field.
- `Resolver` (EVO-80): the chain in the order sources are added, `global()` for hosts (with `fetch`), `local()` for the checker; `cargo::locate_version`; `fetch` from crates.io or GitHub, bounded and checked, misses remembered.
- `build::workspace_set` (EVO-80): a build script writes the `Set` of every crate in its lock that has `api_snapshots/`; `cargo::CargoSources::locked_package_dirs`.
- New crate (EVO-80): snapshots resolved by crate name and version through embedded `Set`s, the cache `~/.wire_weaver/snapshots/`, Cargo's sources and a fetch; every file checked against its SHA-256 header and its crate version; `file`, `dir`, `cache`, `limits`, `Set`.
