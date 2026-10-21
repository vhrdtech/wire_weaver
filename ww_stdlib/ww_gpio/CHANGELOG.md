## Unreleased

### ⚠️ Breaking

- `Error` is `GpioError<'i>`: the first variant is `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`,
  `InvalidArgument`, `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`;
  the `Unsupported*` variants are `Common(Unsupported)`, `NotImplemented`, `CustomU8`, `CustomU32` are gone,
  `DifferentModes` stays. It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime
  (`GpioErrorOwned` on hosts), has `Display` from its doc lines, and every method lists its variants with
  `#[errors(..)]`. Devices return `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`).
- `Bank::configure(config: BankConfig) -> Result<BankConfig, Error>` with `BankConfig::io_level: Option<Volt>`
  instead of the `reference_voltage` property (STD-13); the answer is the level in effect. `BankCapabilities::voltage`
  is `io_levels: MaxLen<RefVec<Volt>, MAX_IO_LEVELS>`, with `io_level_min` / `io_level_max`.
- `Error` get `Unknown(u32)` from `derive_shrink_wrap` (SW-47): a variant from a newer version of this crate is read
  into it instead of failing. Exhaustive matches need an `Unknown(_)` arm. Same wire.
- `Error` gains `InUse`, `NoIoLevel`, `Refused` (STD-13).
- `Pin::event` is a counted stream function (API-31): `event(enabled: IoPinEnabledEvents) -> Result<(), Error>`
  replaces `configure_events` + the plain `event` stream. The host opens it with the sources to report and closes
  it to stop; events carry their device time and losses in the event head. Servers implement `pin_event` /
  `close_pin_event` / `stream_pin_event` (a `ww_stream::tx::Counted`) and `new_stream_session` instead of
  `pin_configure_events`, and send with `stream_data_ser()...event_send(&mut counted, &event, 1, t, out)`.

### 🚀 Features

- `Bank` is `#[claim(exclusive)]` (SRV-34): a client can claim a bank through the hub. Wire compatible, only the introspection data changes.
- `Pin::event` and the `pull` / `speed` properties list their errors with `#[errors(..)]` (API-32), now that stream
  functions and properties can carry one, instead of a hand-written `# Errors` note: rustdoc, introspection and the
  client's warning about an unlisted error see them.
- `Pin::capabilities` returns `PinCapabilities`: modes, pulls, speeds, events, the pin's name.

### 🐛 Fixes

- `Bank` is `pub` (DOC-6: `#[ww_trait]` now leaves a real Rust trait, which must be visible for `ww_impl!` to use
  it from another crate).
