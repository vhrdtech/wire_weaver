# ww_device changelog

## Unreleased

### 🚀 Features

- Handlers get a `wire_weaver::Context` with the request seq, the server's medium (`Server::with_medium()`, same for
  `blocking::Server`) and an `EventWriter` to send events right from the handler. `Server::new()` and
  `blocking::Server::new()` take a second scratch buffer for these events, `RttBuffers` has one more
  `MAX_MESSAGE_LEN` buffer. `sink()` returns an `EventWriter` instead of `(Sink, scratch)`, use it with generated
  `stream_data_ser().<name>_send(.., &mut server.sink())`. `blocking::Sink` implements `BlockingMessageSink` instead
  of `MessageSink`.
- Stream media support: `StreamTx` / `StreamRx` implement `MessageTx` / `MessageRx` over byte level `StreamSink` / `StreamSource` (RTT, UART, TCP, ...), messages are never split across chunks; `transport::stream_overhead()` to size buffers.
- `rtt` feature: RTT (SEGGER Real-Time Transfer) as a medium on top of `rtt-target` channels — `rtt::RttSink` / `RttSource`, `RttConfig` (poll interval, write timeout), `RttBuffers` and `rtt_server()` returning a ready `RttServer`, e.g., alongside `defmt` on another RTT channel.
- First version: `no_std`, allocation-free device side — sans-IO `DeviceLink`, async `Server` with `wait()` / `handle()` for a user-owned event loop, and `blocking::Server`.
