## Unreleased

### ⚠️ Breaking

- Snapshots saved again with definitions' module paths (API-46, ww_self `ItemStruct::path`): the definitions are
  the same, their signatures changed with the format, so a host with these snapshots finds the stdlib definitions of
  a device built before only through the definition cache.
- Snapshots saved again in the ww_self 0.3 format (API-43): the definitions are the same, their signatures changed
  with the format. The `ww_si` 0.1.0 snapshot holds the generic `Timed<T>`.

### 🚀 Features

- `inline_skipped_or_cached(bundle, cache_dir)` (EVO-9): `inline_skipped` that also looks up definitions by signature
  in the definition cache.
- `ww_firmware_info`, `ww_board_info`, `ww_uid`, `ww_user_info` 0.1.0 snapshots hold the reworked device info APIs (STD-24).
- `ww_logic_analyzer` 0.1.0 and `ww_signal_generator` 0.1.0 snapshots (STD-21, STD-22).
- `ww_dfu` 0.1.0 snapshot holds the new update API (STD-12), with `StageBulk::plan` a credit sink (API-28).
- New crate: API snapshots of `ww_global` and `ww_stdlib` crates, embedded at build time. `get()` and `all()` look them
  up by crate name and version, `inline_skipped()` puts the definitions of skipped traits and types known from them
  back into a bundle, `files()` returns the embedded files as saved.
