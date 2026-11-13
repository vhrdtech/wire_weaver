## Unreleased

### ⚠️ Breaking

- `Flex::index` / `FlexBlocking::index`, `Bank::pin` / `pin_get_mode` (blocking and async) and `BankPromise::pin` /
  `pin_ref` / `pin_indices` take or return `PinIndex` (`ww_si::Index<ww_si::units::Pin, u32>`, new alias) instead of
  a bare `u32` (AGENTS.md "Numbers are typed"). `BankPromise::new_hint`'s `valid_pin_indices` is `Arc<Vec<PinIndex>>`.
- `Bank::set_io_level` / `io_level` (blocking and async) take or return `ww_gpio::IoLevel` instead of `ww_gpio::Volt`
  (removed), following `ww_gpio`'s `io_level` change.

- `Error::Gpio` holds `ww_gpio::GpioErrorOwned` (the `CommonError`-based error of `ww_gpio`, STD-16) instead of
  `ww_gpio::Error`; `FlexPromise::set_mode_promise()` returns `&Promise<(), CallError<GpioErrorOwned>>`.
- `BankBlocking::reference_voltage` / `set_reference_voltage` are `io_level` / `set_io_level`, on `ww_gpio`'s
  `Bank::configure` (STD-13); both answer the level in effect, `None` on a bank without level control.
- Calls of `ww_gpio` methods returning `Result` take one `?` (CLT-24): `Error` converts from
  `wire_weaver_client::CallError<ww_gpio::Error>`, and gets `Error::Unimplemented { resource }` for a method or
  property the device doesn't implement. `FlexPromise::set_mode_promise()` returns
  `&Promise<(), CallError<ww_gpio::Error>>`.

### 🚀 Features

- Async flavor `asynchronous::{Flex, PushPullOutput, Input, OpenDrainOutput, Bank}` (STD-6), for code in an async
  task. `Flex`, `Input` and `OpenDrainOutput` implement `embedded_hal_async::digital::Wait` without a race: the
  pin's `event` stream is opened first, then the level read, then an edge awaited; the stream is closed when the
  wait ends or is dropped. `last_edge_time()` keeps the device time of the edge that ended the wait;
  `set_priority()` / `urgent()` set the priority of the pin's requests (CLT-50).
- The blocking flavor implements `embedded_hal::digital::{OutputPin, StatefulOutputPin, InputPin}`
  (`FlexBlocking`, `PushPullOutputBlocking`, `InputBlocking`, `OpenDrainOutputBlocking`), `Error` implements
  `embedded_hal::digital::Error`.
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

### 🐛 Fixes

- Reads the pin index from the commander's `ResourceIndex` base path (STD-115, `wire_weaver_client`'s breaking change).
- Drop the unused `tokio` and `ww_si` dependencies (cargo-machete).
- Follows `ww_gpio`'s counted `Pin::event` stream function (API-31): `enable_interrupts` opens the pin's event
  stream with the edges to report, `disable_interrupts` closes it (STD-13).
