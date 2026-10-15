## Unreleased

### ⚠️ Breaking

- `TriggerStage::UartByte::baud` is a `ww_si::Baud` instead of a `u32` (STD-13).

### 🚀 Features

- New crate (STD-21): `LogicAnalyzer` (triggered capture into device RAM: `capture`, `rearm`, `status`, `read_raw`,
  per-channel `read_transitions`, `abort`) and `LogicStream` (continuous capture: `data(config)` is a counted stream
  function, API-31, unit chunk). Encodings `Raw` (bit-packed words) and `Transitions` (per-channel edges as SW-31
  `DeltaOfDelta<u32>`, SW-50).
  Triggers: staged `Pattern` conditions (level, edge) and protocol stages `UartByte`, `I2cAddress`, `SpiFirstByte`,
  external input, trigger output.
- Docs page `docs/std_library/ww_logic_analyzer.md` (STD-13).
- `Edge` is `no_future_version` (SW-47): rising, falling and any are all there is, so it has no `Unknown(u32)`
  variant; the crate's other `Unsized` enums have one.
