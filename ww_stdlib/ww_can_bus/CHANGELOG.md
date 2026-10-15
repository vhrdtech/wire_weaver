## Unreleased

### ⚠️ Breaking

- One `Can` trait instead of `CANBusInterfaceGroup` and `CANBusInterface` (STD-29): a controller with its
  transceiver, several of them as `ww_impl!(can[]: ww_can_bus::Can)`. The group's clock and power methods are gone:
  the controller clock is reported in `Capabilities::clock`, powering down is `Mode::Off`, and an instance that
  can't be changed alone answers `Common(InUse)`.
- Six required items, all a simple node implements: `capabilities()`, `configure(CanConfig) -> Result<CanConfig,
  CanError>` answering what was set (bit timing as `Timing::Rate` or `Timing::Segments(BitTiming)`, answered as
  segments; `FdConfig` with data timing, `Tdc` and ISO / non-ISO; `Options`; `bus_off_recovery`), `config()`, the
  observable `status` property (`Status`: `ErrorState`, TEC, REC, last protocol errors), the `#[counted]` `rx`
  stream of `RxFrame` (receive time and lost frames in the event head, the matching filter index in the frame) and
  the `#[credit]` `tx` sink of `TxFrame` (with a marker for TX events). `tx_buffer`, `rx_buffer0` / `rx_buffer1` /
  `rx_dedicated`, the termination methods and `performance()` / `group_performance()` are gone.
- `#[optional]` items for what only some controllers have, left out with `omit = [..]`: `recover`, `tx_abort`,
  `tx_events` (`TxEvent` with marker and `TxOutcome`), `bus_errors` (`BusError`: `ProtocolError`, `Phase`,
  `Direction`), `set_filters` (group `filters`: `FilterConfig` of `Filter` with `FilterMatch` exact / mask / range
  and `FilterAction` FIFO / buffer / reject), `rx_buffer` (group `rx_buffers`), `tx_buffer` with an optional cyclic
  `period` (group `tx_buffers`), `set_termination` + `termination` (group `terminator`), `test_pins`.
- Frames: `Frame { id: CanId, kind: FrameKind, data }` with `FrameKind::{Data, Remote { len }, Fd { brs, esi }}`
  and `MaxLen<_, 64>` data replace `CANFrame`, `CANFrameKind`, `CANRtr`, `CANBrs`, `CANEsi`, `CANEnvelope`; the
  `CAN` prefix is dropped (`CanId`).
- Errors are `CanError<'i>`: `#[common] Common(CommonError)`, `OutOfFilters { max }`, `BusOff`, with `Display`; every
  method lists its variants with `#[errors(..)]`. `CANError`, `CANMode`, `CANCapabilities`,
  `CANTerminationSwitchResult`, `CANNotificationKind` and `CANErrorCode` are replaced by `CanError`, `Mode`,
  `Capabilities`, `Termination`, `Status` / `BusError` and `ProtocolError`.
- No dependency on `ww_numeric` and `ww_version` any more, `ww_si` instead (`Baud`, `Hertz`, `Ohm`, `Second`).

### 🚀 Features

- `embedded-can` feature: `CanId` from and to `embedded_can::Id`, `embedded_can::Frame` for `FrameOwned` (std).
- `BitTiming::bit_rate()`, `sample_point()`, `quanta()`; `CanId::standard()`, `extended()`, `raw()`;
  `Frame::len()`, `is_fd()`.
- Docs page `docs/std_library/ww_can_bus.md`, mock-server tests `tests/io_can` (full and minimal node).

### 🐛 Fixes

- The `defmt` feature enabled `ww_numeric/std` and pulled `std` into `no_std` builds; it now enables `ww_si/defmt`.
