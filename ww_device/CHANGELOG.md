# ww_device changelog

## Unreleased

### 🐛 Bug Fixes

- RTT devices no longer panic on their first request ("Found waker not created by the Embassy executor"): the async
  `Server`'s check for another waiting request polls with the task's own waker instead of a no-op one, which
  embassy-time refuses once the medium waits on a `Timer` (TR-13, broken since TR-12).

### ⚡ Performance

- Async `Server` flushes replies at once when no other request is already waiting, instead of at the end of the accumulation window; back to back requests still share packets, events written from the main loop keep the window. `DeviceLink::flush_now()` (TR-12). On Donguru (STM32G0B1, USB FS): 64-byte loopback round trips 647/s -> 2000/s, 1 KB 247/s -> 330/s.

### 🚀 Features

- `ws` feature: WebSocket as a medium, `| kind | payload |` per binary message as the `ws` transport of
  `wire_weaver_client` expects. `ws::WsTx` / `WsRx` (cancel-safe handshake and receive, frames unmasked in place)
  over any `ws::Socket`, sharing it through a `WsConnection`; `WsBuffers` and `ws_server()` returning a ready
  `WsServer`. Handshake and frame headers from the sans-IO parts of `edge-http` / `edge-ws`.
- `udp` feature: UDP as a medium, one `ww_framer` frame per datagram as the `udp` transport of `wire_weaver_client`
  expects. `udp::UdpSink` / `UdpSource` (packet IO for `FramedTx` / `FramedRx`) over any `udp::DatagramSocket`,
  sharing it and the current peer through a `UdpConnection`; one host at a time, adopted when it starts link setup,
  another host takes over the same way. `UdpBuffers` and `udp_server()` returning a ready `UdpServer`.
- `embassy-net` feature: sockets for the enabled media — `ws::EmbassyNetSocket`, a `ws::Socket` on an `embassy-net`
  TCP socket, and `udp::EmbassyNetUdpSocket`, a `udp::DatagramSocket` on an `embassy-net` UDP socket (with a send
  timeout). It no longer enables `ws` by itself, enable `ws` and / or `udp` next to it.
- Handlers get a `wire_weaver::Context` with the request seq, the server's medium (`Server::with_medium()`, same for
  `blocking::Server`) and an `EventWriter` to send events right from the handler. `Server::new()` and
  `blocking::Server::new()` take a second scratch buffer for these events, `RttBuffers` has one more
  `MAX_MESSAGE_LEN` buffer. `sink()` returns an `EventWriter` instead of `(Sink, scratch)`, use it with generated
  `stream_data_ser().<name>_send(.., &mut server.sink())`. `blocking::Sink` implements `BlockingMessageSink` instead
  of `MessageSink`.
- Stream media support: `StreamTx` / `StreamRx` implement `MessageTx` / `MessageRx` over byte level `StreamSink` / `StreamSource` (RTT, UART, TCP, ...), messages are never split across chunks; `transport::stream_overhead()` to size buffers.
- `rtt` feature: RTT (SEGGER Real-Time Transfer) as a medium on top of `rtt-target` channels — `rtt::RttSink` / `RttSource`, `RttConfig` (poll interval, write timeout), `RttBuffers` and `rtt_server()` returning a ready `RttServer`, e.g., alongside `defmt` on another RTT channel.
- First version: `no_std`, allocation-free device side — sans-IO `DeviceLink`, async `Server` with `wait()` / `handle()` for a user-owned event loop, and `blocking::Server`.
