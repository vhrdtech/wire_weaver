## Unreleased

### ⚠️ Breaking

- One `configure(config: UartConfig) -> Result<UartConfig, Error>` instead of the `baud_rate`, `mode`,
  `stop_bits`, `parity`, `data_bits`, `prevent_back_feed` and `reference_voltage` properties and `baud_rate_actual()`
  (STD-13): the answer is what was actually set (baud rate after the divider, IO level in effect).
  `UartConfig::io_level: Option<Volt>` replaces `reference_voltage`; `MonitorConfig` gets `io_level` too and
  `UartMonitor::lines` answers the actual `MonitorConfig` instead of a `Baud`. `Capabilities::voltages` is
  `io_levels` (bounded by `MAX_IO_LEVELS`), with `io_level_min` / `io_level_max`.
- Baud rates are `ww_si::Baud` (STD-13): the `BaudRate` enum is gone, `baud_rate`, `MonitorConfig::baud_rate`,
  `baud_rate_actual()`, `UartMonitor::lines` and `Capabilities::min_baud_rate` / `max_baud_rate` take or return a
  `Baud` (`Baud::baud(115_200u32)`) instead of `BaudRate` or a `u32`.
- `BaudRate`, `Mode`, `Parity`, `DataBits`, `Error` get `Unknown(u32)` from `derive_shrink_wrap` (SW-47): a variant from a newer version of this crate is read
  into it instead of failing. Exhaustive matches need an `Unknown(_)` arm. Same wire.
- `tx` is a `#[credit]` sink of `MaxLen<RefVec<'i, u8>, 256>`; the host never overruns the TX buffer (STD-13).
- `tx_mon` and the `Monitor2` / `Monitor4` modes moved to the `UartMonitor` trait: `lines(config)`, a counted stream
  function (API-31) of `MonitorChunk`s with both lines of the watched link in arrival order, answering the baud rate
  actually set.
- `rx` is a counted stream (API-31, unit byte): `RxChunk::timestamp` is gone, the event head carries the chunk's
  position, the time of its first byte and the bytes the device dropped. Servers keep a `ww_stream::tx::Counted`
  (`stream_rx`), implement `new_stream_session` and send with `rx_send(&mut counted, &chunk, len, t, out)`.
- `RxChunk::bytes` is bounded (`UART_CHUNK_MAX`); `Error` gains `InUse`, `NoIoLevel`, `Refused`, `Timeout`.

### 🚀 Features

- `data_bits` property, `baud_rate_actual`, `send_break`, `tx_pending`; `RxFlags` framing, overrun, break; capabilities with buffer sizes.
- `BaudRate`, `Mode`, `Pin`, `StopBits`, `Parity`, `DataBits` now derive `PartialEq`/`Eq` (Rust-side only, no wire change).
- Full rustdoc with an `# Errors` section per fallible method, a mock server and in-process tests per trait
  (`tests/io_uart`), and a docs page (`docs/std_library/ww_uart.md`).
