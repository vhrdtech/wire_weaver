## Unreleased

### ⚠️ Breaking

- `TransitionsChunk::edges` is a `Series<'i, u32>` instead of a `DeltaOfDelta<'i, u32>` (removed from shrink_wrap).
  On 128 edges it takes 8 B instead of 23 B for a steady clock, 76 B / 129 B with one sample of jitter, 300 B / 398 B
  for sparse random edges, 150 B / 151 B for UART bytes; the worst case is 610 B (`MAX_READ_LEN` 768 still holds a
  chunk, checked at compile time).
- `LaError<'i>`: the first variant is `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`, `InvalidArgument`,
  `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`; `NoSuchChannel`
  stays. It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime (`LaErrorOwned` on
  hosts), has `Display` from its doc lines, and every method lists its variants with `#[errors(..)]`. Devices return
  `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`).
- `TriggerStage::UartByte::baud` is a `ww_si::Baud` instead of a `u32` (STD-13).

### 🚀 Features

- `LogicStream::data` lists its errors with `#[errors(..)]` (API-32), now that stream functions and properties can carry
  one, instead of a hand-written `# Errors` note: rustdoc, introspection and the client's warning about an unlisted
  error see it.
- New crate (STD-21): `LogicAnalyzer` (triggered capture into device RAM: `capture`, `rearm`, `status`, `read_raw`,
  per-channel `read_transitions`, `abort`) and `LogicStream` (continuous capture: `data(config)` is a counted stream
  function, API-31, unit chunk). Encodings `Raw` (bit-packed words) and `Transitions` (per-channel edges as a
  `Series<u32>`, SW-60).
  Triggers: staged `Pattern` conditions (level, edge) and protocol stages `UartByte`, `I2cAddress`, `SpiFirstByte`,
  external input, trigger output.
- Docs page `docs/std_library/ww_logic_analyzer.md` (STD-13).
- `Edge` is `no_future_version` (SW-47): rising, falling and any are all there is, so it has no `Unknown(u32)`
  variant; the crate's other `Unsized` enums have one.
