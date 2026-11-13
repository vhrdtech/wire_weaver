## Unreleased

### ⚠️ Breaking

- `NameMax` is `Bytes<u8, Range<0, { MAX_NAME_LEN as i64 }>>` (was a `u16`), `NotesMax` `Bytes<u16, Range<0, { MAX_NOTES_LEN as i64 }>>` (EVO-72, P2401#13nov-0531): the bounds name their consts, and the name cap is in the smallest storage that holds 32. Wire line `w3.0`, as `name_max` is one byte now; never published, the 0.1.0 snapshot saved again in place. `Fixed` and `One` are no longer re-exported (nothing here uses them).
- Reworked (STD-24, never published): `name` is bounded to 32 bytes, `notes` to 256 and `#[optional]`, writes return
  `UserInfoError` (`Common(CommonError)`, `TooLong { max }`). New const item `CAPABILITIES: UserInfoCaps`
  (`name_max`, `notes_max`, `persistent`); a write is saved before the call answers and only when the value changed.
  The crate has `std` and `defmt` features now.

- `UserInfoCaps::name_max` and `notes_max` are `NameMax` / `NotesMax` (`Fixed<units::Byte, One, u16, Range<0, 32>>` /
  `Range<0, 256>>`) instead of a bare `u16`, bounded to what any device can declare; build one with
  `NameMax::bounded(n)` / `NotesMax::bounded(n)`. `UserInfoError::TooLong::max` is `Bytes<u16>` (unbounded: a device's
  own cap). Wire break (new wire line `w2.0`, EVO-60, never published): a bound is part of the contract.

### 🚀 Features

- the `name` and `notes` properties list their errors with `#[errors(..)]` (API-32), now that stream functions and
  properties can carry one, instead of a hand-written `# Errors` note: rustdoc, introspection and the client's warning
  about an unlisted error see them.
- Page `docs/std_library/ww_user_info.md`, test pair `tests/user_info`.
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

