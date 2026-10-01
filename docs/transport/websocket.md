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

There is no ready-made device side yet. A server accepts a WebSocket connection and implements `ww_device`'s
`MessageTx` / `MessageRx` with the format above (buffering messages until `flush`, and with `TCP_NODELAY` set on the
accepted socket for the same reason as on the host, servers usually don't set it), then runs the same
[Server](ww_link.md#how-the-device-runs-it) loop as over USB. `wire_weaver_client/src/ws.rs` has one in its tests,
on top of `tokio-tungstenite`.
