# Changelog

## Unreleased

### 🚀 Features

- New crate (STD-76): `I2cMaster` implements `embedded_hal::i2c::I2c` and `embedded_hal_async::i2c::I2c` (7-bit and
  10-bit addresses) over any client serving `ww_i2c::I2c`, so a stock embedded-hal driver runs unchanged on the host.
  One embedded-hal transaction is one `ww_i2c` call, one round trip: contiguous operations of the same direction are
  merged (embedded-hal runs them without a repeated start, `ww_i2c` puts one before every operation), then sent as
  `i2c_write`, `i2c_read` (plain or `RepeatedStart`) or `i2c_transaction`. A transaction past the device's
  `I2cLimits` (from `i2c_capabilities`, read once by `I2cMaster::new` / `new_blocking`, or given to
  `with_limits`) is refused with `Error::TooManyOps` / `WriteTooLong` / `ReadTooLong`, never split. `Error`
  implements `embedded_hal::i2c::Error`: `Nack { at }` is `NoAcknowledge(Address | Data | Unknown)`,
  `Arbitration` `ArbitrationLoss`, `Bus` and `BusStuck` `Bus`, `Overrun` `Overrun`, the rest `Other`.
  `I2cMaster::urgent` / `set_priority` set the priority of the handle's requests (CLT-50) for latency-bound drivers;
  `last_transaction_time` gives the device time of the last transaction.
