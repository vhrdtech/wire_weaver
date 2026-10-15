## Unreleased

### ⚠️ Breaking

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
