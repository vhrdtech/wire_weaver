## Unreleased

### ⚠️ Breaking

- Wire line `w2.0`: it follows `ww_gpio`'s `w2.0` (EVO-64), whose `Level` this crate re-exports. No code change.
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
- Wire line `w2.0` (EVO-59): every channel number is an `Index<Channel, u8>` (`read_transitions`,
  `TriggerStage::UartByte::channel`, `I2cAddress::scl`/`sda`, `SpiFirstByte::cs`/`sck`/`data`,
  `ChannelCondition::channel`, `TransitionsChunk::channel`, `CaptureConfig::channels`, `StreamConfig::channels`,
  `RefVec<u8>` -> `RefVec<Index<Channel, u8>>`); `RateLimit::channels` is a `Count<Channel, u8>`;
  `ChannelInfo::gpio_pin` is an `Option<Index<Pin, u8>>`. Every sample position is an `Index<Sample, u32>`
  (`read_transitions`'s `from`, `CaptureStatus::Done::trigger_at`, `SampleRange::first`, `RawChunk::first`,
  `TransitionsChunk::from`/`scanned_to`, and `TransitionsChunk::edges`'s `Series<u32>` -> `Series<Index<Sample,
  u32>>`, same bytes); every sample amount is a `Count<Sample, u32>` (`CaptureConfig::samples`/`pre_trigger`,
  `CaptureActual::samples`, `CaptureStatus::Done::samples`, `SampleRange::count`, `RawChunk::samples`).
  `LaCapabilities::memory` is a `Bytes<u32>`; `RawLayout::width` is a `Fixed<Bit, One, u8>`;
  `TriggerCapabilities::max_stages`/`max_conditions` are `Count<Item, u8>`; `TriggerStage::SpiFirstByte::mode` is a
  new `SpiMode` enum instead of a bare `0..=3` `u8` (its own copy of `ww_spi::SpiMode`'s CPOL/CPHA numbering: that
  type doesn't derive `defmt::Format`, needed here since `TriggerStage` does, so a dependency on `ww_spi` for one
  enum isn't worth it). All transparent wrappers or same-size enums, so the bytes stay the same; `ww_si` gains the
  `information` and `counting` features it was missing for a standalone build (it built only through feature
  unification with another crate before).

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
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

### 🐛 Fixes

- Takes `ww_si` with the features it uses (`time`, `electrical`), so it builds alone without default features; it built only where another crate turned them on.
