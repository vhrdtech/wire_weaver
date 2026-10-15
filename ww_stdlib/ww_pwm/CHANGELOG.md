## Unreleased

### ⚠️ Breaking

- `PwmError<'i>`: the first variant is `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`, `InvalidArgument`,
  `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`; `SharedTimer` stays.
  It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime (`PwmErrorOwned` on
  hosts), has `Display` from its doc lines, and every method lists its variants with `#[errors(..)]`. Devices return
  `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`).

### 🚀 Features

- `PwmCaptureChannel::capture` lists its errors with `#[errors(..)]` (API-32), now that stream functions and properties
  can carry one, instead of a hand-written `# Errors` note: rustdoc, introspection and the client's warning about an
  unlisted error see it.
- New crate (STD-19): `Pwm` with `channel[]: PwmChannel` (`set_output`, `set_duty`, `stop`, `state`), `PwmCapture`
  with `channel[]: PwmCaptureChannel` (frequency, duty, pulse width, edge count; `capture(config)` is a
  counted stream function, API-31, `set_capture` / `read_capture` poll without a stream). Full rustdoc on every trait and
  type (`# Errors` sections per method), `cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))` on every
  type, mock server and in-process tests (`tests/io_pwm` + `tests/io_pwm_api`), docs page
  (`docs/std_library/ww_pwm.md`).
