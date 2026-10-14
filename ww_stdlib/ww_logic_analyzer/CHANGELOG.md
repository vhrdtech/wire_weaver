## Unreleased

### 🚀 Features

- New crate (STD-21, design in `docs/std_library/logic_analyzer.md`): `LogicAnalyzer` (triggered capture into
  device RAM: `capture`, `rearm`, `status`, `read_raw`, per-channel `read_transitions`, `abort`) and `LogicStream`
  (continuous capture, `configure_stream`, sideband `Open` / `Close`). Encodings `Raw` (bit-packed words) and
  `Transitions` (per-channel edges as SW-31 `DeltaOfDelta<u32>`; carried as its serialized bytes until SW-50).
  Triggers: staged `Pattern` conditions (level, edge) and protocol stages `UartByte`, `I2cAddress`, `SpiFirstByte`,
  external input, trigger output.
