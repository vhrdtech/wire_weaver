## Unreleased

### ⚠️ Breaking

- `BaudRate`, `Mode`, `Pin`, `StopBits`, `Parity`, `Error` get `Unknown(u32)` from `derive_shrink_wrap` (SW-47): a variant from a newer version of this crate is read
  into it instead of failing. Exhaustive matches need an `Unknown(_)` arm. Same wire.
- `tx` is a `#[credit]` sink of `MaxLen<RefVec<'i, u8>, 256>`; the host never overruns the TX buffer (STD-13).
- `tx_mon` and the `Monitor2` / `Monitor4` modes moved to the `UartMonitor` trait (`line_a`, `line_b`, `configure_monitor`).
- `RxChunk::bytes` is bounded (`UART_CHUNK_MAX`); `Error` gains `InUse`, `NoIoLevel`, `Refused`, `Timeout`.

### 🚀 Features

- `data_bits` property, `baud_rate_actual`, `send_break`, `tx_pending`; `RxFlags` framing, overrun, break; capabilities with buffer sizes.
