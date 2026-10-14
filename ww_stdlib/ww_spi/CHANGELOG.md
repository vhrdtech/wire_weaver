## Unreleased

### 🚀 Features

- The empty `Spi` trait gets its API (STD-13, skeleton): `transfer`, `transaction` (CS held), `write`, `configure` returning the frequency actually set, manual CS, `reference_voltage`; `SpiMonitor` with `frames(config)`, a counted stream function (API-31), one `SpiFrame` per CS assertion (when CS was asserted is the time in the event head).
- Full rustdoc on every trait, method and type, with an `# Errors` section per fallible method (STD-13).
