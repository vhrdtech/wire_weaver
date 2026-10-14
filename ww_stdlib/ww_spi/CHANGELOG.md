## Unreleased

### 🚀 Features

- The empty `Spi` trait gets its API (STD-13, skeleton): `transfer`, `transaction` (CS held), `write`, `configure` returning the frequency actually set, manual CS, `reference_voltage`; `SpiMonitor` with a `frames` stream, one `SpiFrame` per CS assertion.
