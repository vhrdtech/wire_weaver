## Unreleased

### ⚠️ Breaking

- `ChannelInfo::unit`'s `SIExpr` takes exact_units UNIT-7's form (STD-107, EVO-36): a named unit is `SIExpr::Def(UnitDef)` (`SIExpr::of::<ww_si::units::Volt>()` as before) or `SIExpr::Ref(MarkerId)` to a unit the API lists, which a host reads through the API model.
- `ChannelInfo::unit` is a `ww_si::SIExpr<'i>` (STD-37) instead of `ww_si::BaseUnit<'i>`, which is removed: a unit of
  ww_si is `SIExpr::of::<ww_si::units::Volt>()`, a device's own `SIExpr::Named { name, symbol, dim, scale }` with its
  dimension and scale, so a host converts readings of any channel to SI. `ww_adc` re-exports `SIExpr` (and
  `SIExprOwned` with `std`) instead of `BaseUnit`.
- `AdcError<'i>`: the first variant is `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`, `InvalidArgument`,
  `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`; `NoSuchChannel`
  stays. It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime (`AdcErrorOwned` on
  hosts), has `Display` from its doc lines, and every method lists its variants with `#[errors(..)]`. Devices return
  `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`).

### 🚀 Features

- `AdcStream::samples` lists its errors with `#[errors(..)]` (API-32), now that stream functions and properties can
  carry one, instead of a hand-written `# Errors` note: rustdoc, introspection and the client's warning about an
  unlisted error see it.
- New crate (STD-13 IO trait family, STD-18 fill-in): `Adc` (channels with their `ww_si` unit and range, one
  calibrated reading), `AdcStream` (continuous raw sampling: `samples(config)` is a counted stream function, API-31,
  answering the rate actually set; unit sample set, positions, losses and block times in the event head), `AdcCapture` (lossless triggered block capture into device RAM; a
  one-set capture is the simultaneous read). Full rustdoc on every trait and type; docs page
  `docs/std_library/ww_adc.md`; end-to-end test pair `tests/io_adc` / `tests/io_adc_api` with a mock server
  exercising every trait (busy/error paths, stream open/receive/close, polled capture trigger progression).
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.
