## Unreleased

### 🚀 Features

- New crate (STD-19): `Pwm` with `channel[]: PwmChannel` (`set_output`, `set_duty`, `stop`, `state`), `PwmCapture`
  with `channel[]: PwmCaptureChannel` (frequency, duty, pulse width, edge count). Full rustdoc on every trait and
  type (`# Errors` sections per method), `cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))` on every
  type, mock server and in-process tests (`tests/io_pwm` + `tests/io_pwm_api`), docs page
  (`docs/std_library/ww_pwm.md`).
