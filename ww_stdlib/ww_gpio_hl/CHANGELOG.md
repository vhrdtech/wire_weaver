## Unreleased

### ⚠️ Breaking

- Calls of `ww_gpio` methods returning `Result` take one `?` (CLT-24): `Error` converts from
  `wire_weaver_client::CallError<ww_gpio::Error>`, and gets `Error::Unimplemented { resource }` for a method or
  property the device doesn't implement. `FlexPromise::set_mode_promise()` returns
  `&Promise<(), CallError<ww_gpio::Error>>`.
