## Unreleased

### ⚠️ Breaking

- Reworked (STD-24, never published): one trait with the const item `IDS`, a list of up to 4 `UniqueId` whose first
  entry is the primary id (the USB serial). `UidKind` (`ChipUnique`, `Mac48`, `Eui64`, `Uuid`, `Other`) and `UidSource`
  are `pub` now (they were private, the crate could not be used); `Other` names are bounded.
- `UidKind`, `UidSource` get `Unknown(u32)` from `derive_shrink_wrap` (SW-47): a variant from a newer version of this crate is read
  into it instead of failing. Exhaustive matches need an `Unknown(_)` arm. Same wire.

### 🚀 Features

- Page `docs/std_library/ww_uid.md`, test pair `tests/uid`.
