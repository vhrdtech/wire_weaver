# ww_device changelog

## Unreleased

### 🚀 Features

- Stream media support: `StreamTx` / `StreamRx` implement `MessageTx` / `MessageRx` over byte level `StreamSink` / `StreamSource` (RTT, UART, TCP, ...), messages are never split across chunks; `transport::stream_overhead()` to size buffers.
- `rtt` feature: RTT (SEGGER Real-Time Transfer) as a medium on top of `rtt-target` channels — `rtt::RttSink` / `RttSource`, `RttConfig` (poll interval, write timeout), `RttBuffers` and `rtt_server()` returning a ready `RttServer`, e.g., alongside `defmt` on another RTT channel.
- First version: `no_std`, allocation-free device side — sans-IO `DeviceLink`, async `Server` with `wait()` / `handle()` for a user-owned event loop, and `blocking::Server`.
