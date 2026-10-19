# ww_bus_slave changelog

## Unreleased

### 🚀 Features

- New crate (SRV-26): the device side of the `ww_i2c` / `ww_spi` slave modes (STD-35), no_std, sans-IO, no alloc. `I2cSlave` and `SpiSlave` are built in place in a `static` (`const fn new`, sizes as const parameters) and driven by byte-level hooks from the firmware's interrupt handler, with the task side configuring them, updating the map, queuing answers and draining events.
  - `I2cSlave`: the register map (`pointer_bytes` big-endian, auto-increment, wrap at `len`, page wrap on writes, `read_only`), host updates staged during a transaction and applied at its stop; the answer queue, one answer per master read, with `I2cSlaveUnderrun::Fill`, `Stretch { max }` (`ReadByte::Stretch`, `stretch_deadline`, `resume`, `I2cSlaveEvent::ReadWaiting` at once) and `Nack` (`acks_address`); `unsent` for bytes loaded into a TX FIFO the master didn't take; one `I2cSlaveTransfer` per transfer (repeated start, truncated, bus error, fill count, stretch time) through `peek_event` / `pop_event`, losses through `take_dropped`.
  - `SpiSlave`: frame answers from the queue (`idle`, `repeat_last`, `underrun`) or the register map laid out by `SpiRegisters` (address bytes, `rw` bit, dummy bytes before read data, wrap at `len`); `miso` callable ahead of CS for a TX FIFO, `rewind` for a FIFO the firmware emptied; one `SpiSlaveFrame` per CS assertion.
  - `ConfigError` and `MapError` convert into `I2cError` / `SpiError`.
