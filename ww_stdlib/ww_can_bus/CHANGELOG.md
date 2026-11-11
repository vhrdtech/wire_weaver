## Unreleased

### ⚠️ Breaking

- Typed numbers (STD-114, wire change EVO-62, wire `w1.0` -> `w2.0`): the bare integers of the CAN traits are exact_units types through `ww_si`, each the same bytes as its number unless named below. Users build them with `Fixed::new(n)` / `Index::new(n)` (`try_new` where bounded) and read them with `.get()`:
    - `BitTiming::prescaler` and `TimingLimits::prescaler_min` / `_max` are `Prescaler` (`Fixed<Per<Cycle, TimeQuantum>, One, u16>`); `seg1`, `seg2`, `sjw` and their limits `Fixed<units::TimeQuantum, One, _>`.
    - `Tdc::Manual::offset` and `filter`, `ProtocolStatus::tdc_value` and `XlPwm::short` / `long` / `offset` are `Fixed<units::Cycle, One, u8>` (clock periods).
    - `BusLoad::load` and `Timing::Rate::sample_point` are `Share` (`Fixed<Ratio, Milli, u16, Range<0, 1000>>`, newly bounded); `BusLoad::frames` / `errors` and `tx_abort()`'s answer are `Fixed<units::Frame, One, u32>`.
    - `rx_buffer` / `tx_buffer` take a `BufferIndex`; `FilterAction::Buffer`, `TriggerKind::TxExclusive` / `TxArbitratingStart::buffer` are `BufferIndex`; `FilterAction::Fifo::fifo` and `NonMatching::Accept::fifo` are `FifoIndex`; `RxFrame::filter` and `TriggerKind::RxCheck::filter` are both `FilterIndex` (`Index<units::Filter, UNib32, Range<0, 255>>`; `RxCheck::filter` was a `u16`, a wire break).
    - Counts in `MemoryLayout`, `Capabilities`, `ProtocolStatus` (`error_log`, `memory_errors`), `GroupCapabilities::interfaces` and `CanError::OutOfFilters::max` / `OutOfMemory::available` are counted `Fixed`s (frames, buffers, filters, FIFOs, events, items); `MemoryLayout::element_data` is `Fixed<units::Byte, One, u8, Range<8, 64>>` (newly bounded); `Capabilities::memory` and `GroupCapabilities::shared_memory` are `Fixed<units::Word, One, u32>`; `GroupClock::source` is `Index<units::Item, u8>`.
    - `Performance::min_frames_per_second` / `min_bytes_per_second` are `min_frame_rate: Fixed<Per<Frame, Second>, One, u32>` / `min_data_rate: Fixed<Per<Byte, Second>, One, u32>`.
    - TTCAN: `TtConfig::matrix_cycles` and `Trigger::repeat` are `Fixed<units::Cycle, One, u8, Range<1, 64>>`, `Trigger::offset` and `TtStatus::cycle` `Index<units::Cycle, u8, Range<0, 63>>` (newly bounded), `Trigger::time_mark` `Index<units::Tick, u16>` (NTU); `TtStatus::error_level` is the new enum `TtErrorLevel` (`None`, `Warning`, `Error`, `Severe`) instead of a `u8`, and `TtRole::Master::priority` a `U3` instead of a `u8` (wire breaks).
    - `TxFrame::marker`, `TxEvent::marker` and `XlTxFrame::marker` are `Option<TxMarker>` (`TxMarker(pub UNib32)`), `XlFrame::vcid` is `Option<VcId>` (`VcId(pub u8)`), both `transparent`: the bytes of their number (unit test `typed_numbers_are_their_plain_bytes`).
    - `BitTiming::bit_rate(clock: Hertz) -> Option<Fixed<units::Baud, One, u32>>` takes `Capabilities::clock` as it is (was `clock_hz: u32 -> Option<u32>`); `quanta()` answers `Fixed<units::TimeQuantum, One, u32>`, `sample_point()` a `Share`.
    - Raw protocol fields stay plain, marked `#[no_unit_hint]`: `Status::tec` / `rec`, `TtConfig::ntu_numerator` / `ntu_denominator` / `watchdog`, `XlFrame::sdt` / `af`.
    - `ww_si` features `dimensionless`, `information` and `counting` are on besides `time` and `electrical`.
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
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

### 🐛 Fixes

- The `defmt` feature enabled `ww_numeric/std` and pulled `std` into `no_std` builds; it now enables `ww_si/defmt`.
