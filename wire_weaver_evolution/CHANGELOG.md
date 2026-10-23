## Unreleased

### 🚀 Features

- New crate (EVO-11): the evolution checker of `ww api check`, moved out of `wire_weaver_client::evolution` so an API crate can run it from its own test without the client (tokio, probe-rs, nusb). `compare`, `diff`, `Report`, `Change`, `Bump` and `Difference` as before; `compat` holds the per-resource and per-type rules shared with the client (`compare_items`, `TypeCheck`, `Bounds`).
- `Check::new(crate_dir).strict(true).assert()` (feature `source`, default): parses the crate from source (`wire_weaver_core`), compares it with the newest fitting snapshot in its `api_snapshots/` and panics when the version is not bumped enough, or, in strict mode, when there is no snapshot. `Check::run()` returns an `Outcome` (`Compared(Comparison)` or `NoSnapshot`) and `Outcome::verdict()` the line or error `ww api check` prints.
- Breaking changes name the rule they break: `Report::breaking` is a list of `Finding { rule, message }`, `Rule` (`Removed`, `Renamed`, `Moved`, `GlobalId`, `Resource`, `Type`, `MaxLen`, `FieldKind`, `Optional`) with `summary()` and the docs section in `doc()`. A failed check says the version needed, that a version with a snapshot is released and never changes, which line of `Cargo.toml` to bump and to record the release with `ww api save`.
- `snapshot_files`: `SNAPSHOTS_DIR`, `parse`, `latest_snapshot`, `triplet`, from `ww api`.
