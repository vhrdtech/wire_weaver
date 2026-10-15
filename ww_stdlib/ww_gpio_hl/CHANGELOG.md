## Unreleased

### ⚠️ Breaking

- `Error::Gpio` holds `ww_gpio::GpioErrorOwned` (the `CommonError`-based error of `ww_gpio`, STD-16) instead of
  `ww_gpio::Error`; `FlexPromise::set_mode_promise()` returns `&Promise<(), CallError<GpioErrorOwned>>`.
- `BankBlocking::reference_voltage` / `set_reference_voltage` are `io_level` / `set_io_level`, on `ww_gpio`'s
  `Bank::configure` (STD-13); both answer the level in effect, `None` on a bank without level control.
- Calls of `ww_gpio` methods returning `Result` take one `?` (CLT-24): `Error` converts from
  `wire_weaver_client::CallError<ww_gpio::Error>`, and gets `Error::Unimplemented { resource }` for a method or
  property the device doesn't implement. `FlexPromise::set_mode_promise()` returns
  `&Promise<(), CallError<ww_gpio::Error>>`.
### 🐛 Fixes

- Follows `ww_gpio`'s counted `Pin::event` stream function (API-31): `enable_interrupts` opens the pin's event
  stream with the edges to report, `disable_interrupts` closes it (STD-13).
