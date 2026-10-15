## Unreleased

### ⚠️ Breaking

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
