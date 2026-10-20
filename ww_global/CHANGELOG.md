## Unreleased

### ⚠️ Breaking

* Global ids are `GlobalTypeId` instead of `UNib32`.
* Id 0 follows its crate's rename (STD-26): `ww_log_bare_metal` is `ww_log` now, the constant `WW_LOG_BARE_METAL`
  is `WW_LOG`. Same id, same wire.

### 🚀 Features

* `WW_CLIENT_SERVER` global id (513).
* Trait ids (API-20): one id per `crate::Trait` for every public trait in `ww_stdlib`, in the same number space as
  the crate ids (0-7 `ww_log` and `ww_log::LogDefmt`, 8-63 device contract and common IO, 64-511 the rest, 512+ link
  and protocol crates). `wire_weaver_gid.json` has a new format: `ids` entries with `crate` or `trait`, `aliases`,
  `retired`, plus `reserved` ranges and `no_id`.
* `ENTRIES`, `RESERVED`, `NO_ID` tables and `trait_id`, `crate_id`, `entry` lookups (`Entry`, `EntryKind`).
* `assigned_ids.lock`, the append-only record of every id ever assigned, checked by a test (`just gid-lock` appends).

### ⚙️ Miscellaneous Tasks

* `build.rs` writes into `OUT_DIR` instead of overwriting `src/lib.rs` on every build.
* Moved from `ww_stdlib` into the `wire_weaver` repo root, `repository` now points to `vhrdtech/wire_weaver`.

## [0.1.1] - 2026-01-07

### ⚙️ Miscellaneous Tasks

* Bump shrink_wrap version to 0.1.2

## [0.1.0] - 2026-01-07

### #️⃣ Assigned IDs

* ww_log_bare_metal: 0
* wire_weaver_usb_link: 512