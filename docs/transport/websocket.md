# WebSocket

Connects the host to a device on a network: a Linux board, a simulator in another process or on another machine,
or a gateway in front of devices on other media.

## Host side

Enable the `ws` feature of `wire_weaver_client` and select the device by URL:

```rust
let blinky = Blinky::config(|c| c.websocket_url("ws://192.168.1.10:8080/ww"))
    .connect()
    .await?;
```

Everything above the medium is the same as with [USB](usb.md) or [RTT](rtt.md): [link setup](ww_link.md), version
check, introspection, pings, timeouts, streams. There is no discovery, so device filters (serial, label, ...) are not
checked before connecting, and only plain `ws://` is supported, no `wss://`.

## Wire format

Each [ww_link](ww_link.md) message is one binary WebSocket message:

```
| kind: u8 | payload ... |
```

`kind` is `ww_link::Kind`, the payload is what the same message carries over other media. WebSocket has its own
framing over a reliable stream, so [ww_framer](ww_framer.md) is not used: messages are never split or packed
together, and there is nothing to re-synchronize on. Small messages still share TCP segments: the host buffers them
until the event loop flushes (after the accumulation time) and writes them out in one go. The host's socket has
`TCP_NODELAY` set, so Nagle's algorithm doesn't hold back a flush while an earlier segment is waiting for its ACK,
which would add up to a delayed-ACK timeout (tens of ms) to a request.

Text messages are not expected and drop the connection. Ping and Pong are answered by the WebSocket library and
are not needed to keep the link alive, `ww_link` pings do that.

## Device side

The `ws` feature of `ww_device` is the device end: `ww_device::ws::WsTx` / `WsRx` implement `MessageTx` /
`MessageRx` with the format above, on top of any TCP-like `ws::Socket` (accept one client at a time, read, write).
They run the same [Server](ww_link.md#how-the-device-runs-it) loop as over USB. The `embassy-net` feature adds
`EmbassyNetSocket` on an `embassy-net` TCP socket, and `ws_server()` puts it all together:

```rust
let mut socket = TcpSocket::new(stack, rx_buf, tx_buf);
socket.set_timeout(Some(Duration::from_secs(5)));
socket.set_keep_alive(Some(Duration::from_secs(2)));
let conn = WS_CONNECTION.init(WsConnection::new(EmbassyNetSocket::new(socket, 8080)));
let mut server = ws_server(link_config, conn, EmbassyClock, WS_BUFFERS.init(WsBuffers::new()));
server.run(&mut state).await;
```

- No allocation, no extra copies: frames are received and unmasked in place, replies are accumulated in the tx
  buffer until the event loop flushes. `WsBuffers<MAX_MESSAGE_LEN>` holds all of it; the client's HTTP upgrade
  request must fit into the rx buffer too (a few hundred bytes).
- `wait_message()` and `wait_connected()` are cancel-safe: the handshake and partially received frames are kept in
  `WsRx`, so `server.wait()` can be selected on together with anything else.
- Handshake and frame headers come from the sans-IO parts of `edge-http` and `edge-ws`. Any request path is
  accepted, a plain HTTP request gets `400 Bad Request`.
- Unfragmented binary messages only; Ping and Pong are ignored, Text, fragmented messages and Close drop the
  connection. When the link goes down (e.g., ping timeout), the client is dropped and the next one is accepted.
- `EmbassyNetSocket` disables Nagle's algorithm on each accepted connection, for the same reason as on the host.
  Set a timeout on the socket, so that writes to a host that is gone without closing the connection fail.

`examples_mcu/rp2` has a complete example, `ww_ws_ncm`: the board shows up on the host as a USB CDC-NCM Ethernet
adapter, runs `embassy-net` with a static address and a small DHCP server (`edge-dhcp`), so the host is configured
automatically, and serves the blinky API at `ws://192.168.7.1:8080/ww`. Run it with `just run rp2 ww_ws_ncm` from
`examples_mcu/rp2`, then blink from the host with `cargo run -p blinky --features ws --example blinky_ws`.
