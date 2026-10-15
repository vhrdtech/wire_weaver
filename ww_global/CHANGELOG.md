## Unreleased

### ⚠️ Breaking

* Global ids are `GlobalTypeId` instead of `UNib32`.
* Id 0 follows its crate's rename (STD-26): `ww_log_bare_metal` is `ww_log` now, the constant `WW_LOG_BARE_METAL`
  is `WW_LOG`. Same id, same wire.

### 🚀 Features

* `WW_CLIENT_SERVER` global id (513).

### ⚙️ Miscellaneous Tasks

* Moved from `ww_stdlib` into the `wire_weaver` repo root, `repository` now points to `vhrdtech/wire_weaver`.

## [0.1.1] - 2026-01-07

### ⚙️ Miscellaneous Tasks

* Bump shrink_wrap version to 0.1.2

## [0.1.0] - 2026-01-07

### #️⃣ Assigned IDs

* ww_log_bare_metal: 0
* wire_weaver_usb_link: 512