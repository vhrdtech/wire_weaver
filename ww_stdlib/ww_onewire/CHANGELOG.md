## Unreleased

### ⚠️ Breaking

- New wire line, `w2.0` (STD-113, EVO-61): numbers are typed instead of bare integers (AGENTS.md
  "Numbers are typed"). `SearchState::last_discrepancy` is `IndexFrom1<RomBit, u8, Range<1, 64>>` instead of a plain
  `u8` (`RomBit` a counted marker of this crate, declared with `unit!`): the added bound is the wire break, since
  nothing lists the stdlib wire as in use yet. Wire-compatible in the same change (no bump needed on their own, same
  bytes): `search`'s `family: Option<u8>` is `Option<FamilyCode>` (`FamilyCode(pub u8)`, a transparent newtype);
  `OneWireTransaction::read_len: u16` is `ww_si::Bytes<u16>`; `OneWireConfig::pin: u8` is
  `ww_si::Index<ww_si::units::Pin, u8>`. `ww_onewire_0_1_0.ron` is saved again in place (never published). `OneWireTransaction::strong_pullup`'s duration and `OneWireConfig::io_level` stay `Quantity` for now (P2401
  Q2, a separate cross-cutting change).
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
