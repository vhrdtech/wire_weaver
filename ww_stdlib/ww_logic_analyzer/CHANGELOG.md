## Unreleased

### 🚀 Features

- New crate (STD-21): `LogicAnalyzer` (triggered capture into device RAM: `capture`, `rearm`, `status`, `read_raw`,
  per-channel `read_transitions`, `abort`) and `LogicStream` (continuous capture: `data(config)` is a counted stream
  function, API-31, unit chunk). Encodings `Raw` (bit-packed words) and `Transitions` (per-channel edges as SW-31
  `DeltaOfDelta<u32>`, SW-50).
  Triggers: staged `Pattern` conditions (level, edge) and protocol stages `UartByte`, `I2cAddress`, `SpiFirstByte`,
  external input, trigger output.
- Docs page `docs/std_library/ww_logic_analyzer.md` (STD-13).
