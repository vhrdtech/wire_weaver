## Unreleased

### ⚠️ Breaking

- Reworked to the stdlib idioms (STD-29): `Can` is one controller with its transceiver, `CanGroup` the controllers
  that share clock, power and message RAM (`ww_impl!(interface[]: Can)` instead of `CANBusInterfaceGroup` /
  `CANBusInterface`). `CanGroup` has `group_capabilities()`, `enable() -> GroupClock`, `disable()`, the observable
  `group_state` property, and optional `set_clock(GroupClock)` (clock source and frequency, group `clock`) and
  `performance()`; `clock_sources`, `set_clock_source`, `valid_clocks`, `set_clock(u32)` and `group_performance` are
  gone. A bridge without such hardware puts `ww_impl!(can[]: Can)` into its API directly.
- `Can` has six required items, all a simple node implements: `capabilities()`, `configure(CanConfig) ->
  Result<CanConfig, CanError>` answering what was set (bit timing as `Timing::Rate` or `Timing::Segments(BitTiming)`,
  answered as segments; `FdConfig` with data timing, `Tdc` and ISO / non-ISO; `Options`; `bus_off_recovery`),
  `config()`, the observable `status` property (`Status`: `ErrorState`, TEC, REC, last protocol errors), the
  `#[counted]` `rx` stream of `RxFrame` (receive time and lost frames in the event head, the matching filter index
  in the frame) and the `#[credit]` `tx` sink of `TxFrame` (with a marker for TX events). `tx_buffer`, `rx_buffer0`
  / `rx_buffer1` / `rx_dedicated`, the termination methods and `performance()` are replaced.
- `#[optional]` items of `Can`, left out with `omit = [..]`: `set_mode` (explicit mode switches, `Mode::Config` for
  configuration-only steps, `Mode::Sleep`), `recover`, `tx_abort`, `tx_events` (`TxEvent` with marker, `TxOutcome`
  and the measured queue time), `bus_errors` (`BusError`: `ProtocolError`, `Phase`, `Direction`),
  `protocol_status` (raw `ProtocolStatus`: activity, error logging counter, last FD frame flags, protocol exception,
  measured TDC value, memory errors), `bus_load` (measured per window), `detect_timing` (finds the bit timing of a
  running bus), `set_filters` (group `filters`: `FilterMatch` exact / mask / range, `FilterAction` FIFO with high
  priority / buffer / reject), `set_layout` (`MemoryLayout` of the message memory), `rx_buffer` (group
  `rx_buffers`), `tx_buffer` with an optional cyclic `period` (group `tx_buffers`), `set_transceiver` +
  `transceiver_state` (group `transceiver`: standby / silent / sleep, `TransceiverFault`s), `set_termination` +
  `termination` (group `terminator`), `test_pins`.
- New traits `CanTimeTriggered` (TTCAN: `configure_tt(TtConfig)`, `set_schedule(Trigger..)`, `tt_status`) and, behind
  the new `xl` cargo feature, `CanXl` (`configure_xl(XlConfig)`, `rx_xl` / `tx_xl` of 2 KiB `XlFrame`s), placed next
  to the controllers as arrays indexed like them.
- Frames: `Frame { id: CanId, kind: FrameKind, data }` with `FrameKind::{Data, Remote { len }, Fd { brs, esi },
  DataLongDlc { dlc }}` and `MaxLen<_, 64>` data replace `CANFrame`, `CANFrameKind`, `CANRtr`, `CANBrs`, `CANEsi`,
  `CANEnvelope`; the `CAN` prefix is dropped (`CanId`).
- Errors are `CanError<'i>`: `#[common] Common(CommonError)`, `OutOfFilters { max }`, `BusOff`, `OutOfMemory {
  available }`, with `Display`; every method lists its variants with `#[errors(..)]`. `CANError`, `CANMode`,
  `CANCapabilities`, `CANTerminationSwitchResult`, `CANNotificationKind` and `CANErrorCode` are replaced by
  `CanError`, `Mode`, `Capabilities`, `Termination`, `Status` / `BusError` / `ProtocolStatus` and `ProtocolError`.
- No dependency on `ww_numeric` and `ww_version` any more, `ww_si` instead (`Baud`, `Hertz`, `Ohm`, `Second`).

### 🚀 Features

- `Can`, `CanGroup`, `CanTimeTriggered` and `CanXl` are `#[claim(exclusive)]` (SRV-34): a client can claim a controller or group through the hub. Wire compatible, only the introspection data changes.
- `BitTiming::bit_rate()`, `sample_point()`, `quanta()`; `CanId::standard()`, `extended()`, `raw()`;
  `Frame::len()`, `is_fd()`.
- Docs page `docs/std_library/ww_can_bus.md`, mock-server tests `tests/io_can` (full and minimal node) and
  `tests/io_can_group` (group, TTCAN, XL).

### 🐛 Fixes

- The `defmt` feature enabled `ww_numeric/std` and pulled `std` into `no_std` builds; it now enables `ww_si/defmt`.
