## Unreleased

### ⚠️ Breaking

- Received chunks end on line idle (STD-13): `UartConfig::idle_chars: u8` and `MonitorConfig::idle_chars` set how
  many idle character times end a chunk (`IDLE_CHARS_DEFAULT` = 2; 0 is taken as 1), and `configure` / `monitor`
  answer what the receiver set, so each chunk is one back-to-back burst. `RxChunk::end` is `Option<Second>`: devices
  that measure the last byte's time set it, others send `None` and hosts take the last byte at
  `first + (n - 1) * frame_bits / baud`. Devices fill `idle_chars` in their configs and wrap `end` in `Some`.
- `Error` is `UartError<'i>`, whose only variant so far is the `#[common] Common(CommonError<'i>)` (STD-16):
  `Unsupported`, `InvalidArgument`, `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from
  `ww_error::CommonError`; `UnsupportedMode`, `UnsupportedReferenceVoltage`, `WrongMode`, `CustomU8`, `CustomU32`,
  `NoIoLevel` are gone. It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime
  (`UartErrorOwned` on hosts), has `Display` from its doc lines, and every method lists its variants with
  `#[errors(..)]`. Devices return `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`);
  stream-function handlers return `UartError<'static>` for now (API-35).
- Received chunks are always timestamped and say so in the API (STD-13): `RxChunk::end: Second` is when the last
  byte arrived, the event head's time (now always set) when the first one did, both on the device's one clock
  (`ww_timebase::Clock`). `Capabilities::rx_timestamps` is removed. Devices set `end` and pass the first byte's time
  to `rx_send` / `monitor_send`.
- `UartMonitor` is gone: `UartMonitor::lines` is `Uart::monitor`, an `#[optional]` counted stream function of `Uart`
  (STD-13), so an array of ports (`ww_impl!(uart[]: Uart)`) carries monitoring without a second trait. A port
  without monitoring hardware leaves it out with `omit = ["uart.monitor"]`; drop `ww_impl!(..: UartMonitor)`, rename
  the `uart_monitor_lines` handlers to `uart_monitor`.
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

- `UartConfig::frame_bits()` / `MonitorConfig::frame_bits()`: bits of one character on the line (start + data +
  parity + stop), for the time of each byte of a chunk (crate docs, "Time").
- `data_bits` property, `baud_rate_actual`, `send_break`, `tx_pending`; `RxFlags` framing, overrun, break; capabilities with buffer sizes.
- `BaudRate`, `Mode`, `Pin`, `StopBits`, `Parity`, `DataBits` now derive `PartialEq`/`Eq` (Rust-side only, no wire change).
- Full rustdoc with an `# Errors` section per fallible method, a mock server and in-process tests per trait
  (`tests/io_uart`), and a docs page (`docs/std_library/ww_uart.md`).
