# ww_device changelog

## Unreleased

### ⚠️ Breaking

- The device's hello is a packet of its own outside the framer (EVO-23): new `Transmit::Hello(Hello)` (code matching `Transmit` handles it), new `MessageTx::write_hello` (a default writes it framed, `FramedTx` sends the medium's frozen frame); `FramedRx` and `blocking::Server::on_packet` check every packet with `Framing::find_hello` before the framer, so a host whose framer is of another generation is still answered. Packets shorter than the frozen frame take the hello framed.
- Frozen hello (EVO-23): `DeviceLink` answers the host's `Message::Hello` with its own hello, then `DeviceInfo` only when every generation matches; a `LinkSetup` after a mismatched hello is refused with `Disconnect(IncompatibleVersion)`, as is a host from before the hello (its bare token). UDP: the datagram that starts link setup may be 24 bytes now (was 16). Wire break: rebuild firmware and hosts together.
- `LinkConfig::new` takes `image_facts: ImageFacts` after `api_hash`, and `LinkConfig` has that field (SRV-56): pass the generated `image_facts()` from beside `api_hash()`, or `ImageFacts::default()` for a hand-written server. What an image chose at build time reaches the host in `DeviceInfo`, outside the API hash.
- Replies follow the request's priority (CLT-50, SRV-42): `Server::handle` (and `blocking::Server`) no longer flush a reply at once when no other request waits; a default-priority reply waits for the accumulation window like every message, a reply to an urgent request is flushed at once. Deferred replies sent through `Server::sink()` do the same (`Sink` implements `MessageSink::send_reply`): before, they always waited for the window.
- `PacketSink` / `PacketSource` (and `blocking::PacketSink`) have an associated `type Framing: ww_link::Framing` (TR-15): `FramedTx` / `FramedRx` / `blocking::Server` frame in the medium's framing instead of always the USB one, so the UDP medium now uses `UdpFraming` (frame prefix, CRC-32 on every message). Implementations add `type Framing = ww_link::UsbFraming;` (or the medium's own). `TxFramer` / `RxFramer` take the framing as a parameter. The UDP medium only takes a datagram with a good CRC as the start of a link setup, so a corrupted one can't take over.
- Link setup token (TR-15): `DeviceLink` echoes the host's token in `DeviceInfo` and `LinkReady`, takes `LinkSetup` only with the token of the last `GetDeviceInfo`, and answers a host without a token with `Disconnect(IncompatibleVersion)`. On every `GetDeviceInfo`, `poll_transmit` first returns the new `Transmit::NewSession`: `Server` and `blocking::Server` call `MessageTx::new_session()` (new, `reset()` by default; `FramedTx` / `StreamTx` start the framer's next session), so a frame being filled and a half-sent message of the previous session are dropped and the reply starts in a fresh frame. Code matching `Transmit` handles the new variant.
- `PeerMeta` has a new field `app_version` (CLT-55): the program's own version from `ClientMeta`, cut to 24 bytes; code building one with a struct literal adds it.

### 🚀 Features

- `generic_error_reply` writes its error event in place (`ww_client_server::util::ser_err_event`), without linking `Event`'s serializer (SRV-54).
- `Sink::flush` (async and blocking) is public: firmware sends what waits in the frame now, e.g. after a deferred reply (SRV-42). `DeviceLink::on_flushed` for sinks that flush on their own.
- `DeviceLink::peer_meta()` and `PeerMeta` (CLT-41): who the host said it is in link setup (program, user, host cut to 24 bytes, kind, pid, instance id), set before `LinkEvent::Up`, for firmware that wants to log it. About 90 bytes of RAM in `DeviceLink`.

- `FramedRx::prefix_stats()`: lost, duplicated, reordered and previous-session frames seen so far (UDP, USB with `UsbSeqFraming`).
- `Server::wait_until(at: Option<Instant>)` (SRV-25): `wait()` that also returns when `at` comes, `Ready::is_due()`, so the event loop sleeps until the device's own next deadline (`ObserveState::next_wake`) instead of waking on a fixed tick; `handle` does nothing with a due `Ready`. `Server::now()`, the server's clock.
- `GENERIC_ERROR_SEQ` is `ww_client_server::err_seq::GENERIC` (SRV-22), same value.
- `FramedTx::with_tap` / `FramedRx::with_tap` (and `tap_mut`): frames and messages go to a `ww_tap::Tap` as they cross the medium (TR-14), tx frames at submit time. `FramedTx<'a, P, T = NoTap>` / `FramedRx<'a, P, T = NoTap>`: `new` keeps `NoTap`, whose calls compile away (a G0 USB firmware is the same size).
- The link's message limit is the smaller of the device's and the host's (CLT-55, `DeviceLink::agreed_max_message_len`): `check_send` refuses longer messages, and `Server` / `blocking::Server` answer a request whose reply is over it with `ResponseSerFailed` at `err_seq::REPLY_OVER_LIMIT` (`over_limit_reply`) instead of dropping the reply, so the host fails the call at once with both limits named. Before, the device only kept to the host's limit.
- `DeviceLink::peer_version()` and `PeerVersion` (CLT-55): the user API version the host connected with (crate id cut to 32 bytes, major, minor, patch), kept per session with `PeerMeta::app_version` so that version skew between a device and its hosts can be seen. About 70 bytes of RAM in `DeviceLink`.

### 🐛 Bug Fixes

- `PeerMeta`'s 24-byte cut no longer slices a `str` with a panic path (CLT-41): the panic's `char` formatting pulled core's unicode tables into every firmware, 6.0 KB of flash on B135 (thumbv7em, opt-level 3).
- UDP: the datagram that starts link setup may be 16 bytes now (was 8), `GetDeviceInfo` carries the 8-byte token (TR-15).
- RTT devices no longer panic on their first request ("Found waker not created by the Embassy executor"): the async
  `Server`'s check for another waiting request polls with the task's own waker instead of a no-op one, which
  embassy-time refuses once the medium waits on a `Timer` (TR-13, broken since TR-12).

### ⚡ Performance

- `Server::handle` polls its state machine in a function of its own (SRV-55), so a firmware's server task doesn't take in the dispatcher's root and what it calls: Donguru's task was 6.2 KB larger with it inlined.
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
