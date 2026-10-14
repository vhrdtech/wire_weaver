## Unreleased

### ⚠️ Breaking

- Calls of `ww_gpio` methods returning `Result` take one `?` (CLT-24): `Error` converts from
  `wire_weaver_client::CallError<ww_gpio::Error>`, and gets `Error::Unimplemented { resource }` for a method or
  property the device doesn't implement. `FlexPromise::set_mode_promise()` returns
  `&Promise<(), CallError<ww_gpio::Error>>`.
### 🐛 Fixes

- Follows `ww_gpio`'s counted `Pin::event` stream function (API-31): `enable_interrupts` opens the pin's event
  stream with the edges to report, `disable_interrupts` closes it (STD-13).
