## Unreleased

### ⚠️ Breaking

- Reworked (STD-24, never published): `name` is bounded to 32 bytes, `notes` to 256 and `#[optional]`, writes return
  `UserInfoError` (`Common(CommonError)`, `TooLong { max }`). New const item `CAPABILITIES: UserInfoCaps`
  (`name_max`, `notes_max`, `persistent`); a write is saved before the call answers and only when the value changed.
  The crate has `std` and `defmt` features now.

### 🚀 Features

- Page `docs/std_library/ww_user_info.md`, test pair `tests/user_info`.

