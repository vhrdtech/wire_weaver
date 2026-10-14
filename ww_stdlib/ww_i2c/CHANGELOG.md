## Unreleased

### 🐛 Fixes

- `i2c_read` returns `Result<I2cReadEnvelope<'i>, I2cError>`, the lifetime was hidden. Same wire format.
