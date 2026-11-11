## Unreleased

### ⚠️ Breaking

- Wire-compatible (`w3.0` unchanged): channel indices, converter resolution, capture/stream byte and
  capture-read offset counts, and sample-set counts are typed instead of bare integers (AGENTS.md "Numbers are typed",
  P2401#11nov-0948c). Every change is a transparent wrapper of the same bytes, so no wire line moves; only the Rust
  types change. `ww_adc_0_3_0.ron` (never published) saved again in place.
  - `Adc::read`'s `channel`, `Trigger::Rising`/`Falling::channel`, `StreamConfig::channels`,
    `CaptureConfig::channels`: `u8` -> `ww_si::Index<ww_si::units::Channel, u8>`.
    `StreamCapabilities::max_channels`: `u8` -> `ww_si::Count<ww_si::units::Channel, u8>`.
  - `ChannelInfo::bits`: `u8` -> `ww_si::Fixed<ww_si::units::Bit, ww_si::prefix::One, u8>`.
  - `CaptureCapabilities::max_bytes`, `CaptureActual::bytes`: `u32` -> `ww_si::Bytes<u32>`.
  - `AdcCapture::capture_read`'s `offset`: `u32` -> `ww_si::Index<ww_si::units::Byte, u32>`; `len`: `u16` ->
    `ww_si::Bytes<u16>`.
  - `CaptureConfig::sets`, `pre_trigger`, `CaptureStatus::Done::sets`: `u32` -> `ww_si::Count<ww_si::units::Scan,
    u32>` (one set is one reading of every channel, exact_units' `Scan`).
  - `ww_adc` re-exports `Bytes`, `Count`, `Fixed`, `Index`, `ww_si::prefix::One` and `ww_si::units::{Bit, Byte,
    Channel, Scan}` for these.
  - `ww_si` dependency gains the `information` and `counting` feature groups.
  - `Reading::raw` stays a plain `u32` (the converter's raw code before calibration, run-time only: `FullScale`
    needs the reference and bit width at compile time).
- Version 0.3.0 on a new wire line, `w3.0` (EVO-47): `ChannelInfo::unit`'s `SIExpr` carries exact_units UNIT-9's `Dim` with its system's id, which `ww api check` against the 0.2.0 snapshot from before UNIT-9 finds as a wire break. `ww_adc_0_3_0.ron` is new; `ww_adc_0_2_0.ron` and `ww_adc_0_1_0.ron` keep their in-place save (their `SIExpr` signature).
- `exact_units` at UNIT-9 (EVO-47): `ChannelInfo::unit`'s `SIExpr` carries a `Dim` that starts with its system's id; `ww_adc_0_2_0.ron` (never published) and `ww_adc_0_1_0.ron` (its `SIExpr` signature) saved again in place.
- Version 0.2.0 on a new wire line, `w2.0` (EVO-44): `ChannelInfo::unit`'s `SIExpr` carries exact_units' new `Dim` and `UnitNames`. `ww_adc_0_1_0.ron` was saved again in place (its `SIExpr` signature); `ww_adc_0_2_0.ron` is new.
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
