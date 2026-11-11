## Unreleased

### ⚠️ Breaking

- Typed numbers (STD-56), wire line `w2.0` (EVO-57): `word_bits` of `SpiConfig`, `SpiMonitorConfig` and
  `SpiSlaveConfig` is `word: Fixed<Bit, One, u8>`; `SpiCapabilities::min_word_bits` / `max_word_bits` are
  `min_word` / `max_word`, the same type. `SpiRegisters::addr_bytes` / `dummy_bytes` are `addr` / `dummy`,
  `SpiSlaveCapabilities::max_addr_bytes` is `max_addr`, all `Bytes<u8>`; `SpiRegisters::len`,
  `SpiCapabilities::max_len`, `SpiSlaveCapabilities::map_len` and `SpiOp::Read`'s length are `Bytes<u16/u32>`.
  `Spi::slave_map_write` / `slave_map_read`'s `offset` and `SpiSlaveFrame::addr` are `Index<Byte, u32>`;
  `Spi::slave_clear_answers`'s return and `SpiSlaveCapabilities::queue_len` are `Count<Item, u32>`. Every one of
  these keeps the same bytes (`T` to a transparent wrapper of `T`), so only the renames break the wire for a
  device without `#[since]` tracking (none listed as in use): still bumped with the line below, since they land in
  the same change.
- `SpiRwBit::SetIsRead` / `ClearIsRead`'s `bit` is `Index<Bit, u8, Range<0, 7>>` instead of a plain `u8`: the bound
  (0..=7, the first address byte's bits) is now part of the wire contract and checked on decode, before a handler
  runs.
- `SpiConfig` / `SpiMonitorConfig` / `SpiSlaveConfig`'s `io_level` and `SpiCapabilities::io_levels` /
  `io_level_min` / `io_level_max` are `Fixed<Volt, Milli, u16>` instead of `ww_si::Quantity<Volt>`: a device's IO
  level never changes unit or number type at run time, so `Fixed` sends only the number, not a prefix and number
  type with every value too.
- `SpiRole::Slave` and `SpiCapabilities::slave` are removed (STD-35): slave mode is the new `slave` group of `Spi`
  (below), opened with `Spi::slave_frames`; whether a device has it is in its introspection and
  `Spi::slave_capabilities`.

- `SpiError<'i>`: the first variant is `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`, `InvalidArgument`,
  `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`; `BufferTooBig`
  stays. It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime (`SpiErrorOwned` on
  hosts), has `Display` from its doc lines, and every method lists its variants with `#[errors(..)]`. Devices return
  `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`).
- `SpiMonitor` is gone: `SpiMonitor::frames` is `Spi::monitor`, an `#[optional]` counted stream function of `Spi`
  (STD-13), so an array of buses carries monitoring without a second trait. An adapter without monitoring hardware
  leaves it out with `omit = ["spi.monitor"]`; `SpiCapabilities::monitor` is removed (introspection says whether the
  item is there).
- The IO level is `SpiConfig::io_level: Option<Volt>` instead of the `reference_voltage` property, and
  `Spi::configure` answers the actual `SpiConfig` instead of `SpiActual` (removed) (STD-13). `SpiMonitorConfig` gets
  `io_level`; `SpiCapabilities::io_levels` is bounded by `MAX_IO_LEVELS` and gets `io_level_min` / `io_level_max`.

### 🚀 Features

- `SpiMode` derives `defmt::Format` under the `defmt` feature (requested by `ww_logic_analyzer` to embed it in its
  own defmt-deriving types): no wire change, introspection data unaffected.
- `Spi` is `#[claim(exclusive)]` (SRV-34): a client can claim a controller through the hub. Wire compatible, only the introspection data changes.
- Slave mode (STD-35): an `#[optional(slave)]` group in `Spi` (`omit = ["spi.slave"]` without it).
  `slave_frames(SpiSlaveConfig)` is a counted stream function: opening it makes the device a slave, and it reports
  every frame as a `SpiSlaveFrame` (MOSI, MISO, address, `underrun`, `overrun`). Answers come from the
  `slave_answers` credit sink, one per frame (`SpiSlaveAnswers::Queue { idle, repeat_last }`, cleared with
  `slave_clear_answers`), or from a register map (`SpiSlaveAnswers::Registers(SpiRegisters)`: address bytes, a
  `SpiRwBit`, dummy bytes; filled with `slave_map_write`, read with `slave_map_read`); `slave_capabilities` answers
  `SpiSlaveCapabilities`.

- `Spi::monitor` lists its errors with `#[errors(..)]` (API-32), now that stream functions and properties can carry one,
  instead of a hand-written `# Errors` note: rustdoc, introspection and the client's warning about an unlisted error see
  it.
- The empty `Spi` trait gets its API (STD-13, skeleton): `transfer`, `transaction` (CS held), `write`, `configure` returning the frequency actually set, manual CS, `reference_voltage`; `SpiMonitor` with `frames(config)`, a counted stream function (API-31), one `SpiFrame` per CS assertion (when CS was asserted is the time in the event head).
- Full rustdoc on every trait, method and type, with an `# Errors` section per fallible method (STD-13).
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.
