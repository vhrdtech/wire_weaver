## Unreleased

### ⚠️ Breaking

- `OneWireError<'i>`: the first variant is `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`,
  `InvalidArgument`, `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`;
  `NoPresence`, `CrcMismatch`, `ShortCircuit` stay. It is no longer `self_describing` (newer variants read as
  `Unknown(n)`), carries a lifetime (`OneWireErrorOwned` on hosts), has `Display` from its doc lines, and every method
  lists its variants with `#[errors(..)]`. Devices return `CommonError::Unsupported.into()` (or `?` a helper returning
  `CommonError`).
- `OneWireConfig` gets `io_level: Option<Volt>` and `OneWire::configure` answers the actual `OneWireConfig`
  instead of `()` (STD-13); `OneWireCapabilities` lists `io_levels`, `io_level_min`, `io_level_max`.

### 🚀 Features

- New crate (STD-20): `OneWire` with addressed transactions and strong pull-up, reset, paged ROM search, bit-level `triplet`. Full rustdoc, docs page (`docs/std_library/ww_onewire.md`), API snapshot, and an in-process test (`tests/io_onewire`) with a mock bus running the real Dallas/Maxim search algorithm.
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.
