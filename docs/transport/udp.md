# UDP

Connects the host to a device on a network over UDP: low overhead and no connection state, for telemetry-heavy
devices, where a lost stream event now and then is better than a stall while TCP retransmits.

## Host side

Enable the `udp` feature of `wire_weaver_client` and select the device by address:

```rust
let blinky = Blinky::config(|c| c.udp_addr("192.168.1.10:9000"))
    .connect()
    .await?;
```

Host names are resolved when connecting (`my-device.local:9000`), the first address is used. IPv6 works too
(`[fe80::1%2]:9000`).

Everything above the medium is the same as with [USB](usb.md) or [WebSocket](websocket.md): [link setup](ww_link.md),
version check, introspection, pings, timeouts, streams. There is no discovery, so device filters (serial, label, ...)
are not checked before connecting.

The host socket is connected to the device address: datagrams from anyone else are dropped by the OS, and if nothing
listens on the device port, the ICMP error fails the connection right away (on Linux and macOS) instead of after the
link setup retries.

## Wire format

Each datagram is one [ww_framer](ww_framer.md) frame, with `ww_link`'s UDP configuration: `UdpHead` (`U2Head`),
`UdpChecksum` (CRC-16 on split messages only) and `UdpTail` (none). Same as over USB, small messages accumulated by the
event loop share a datagram and big ones are split across several.

The host sends datagrams of at most `ww_link::UDP_MAX_DATAGRAM_LEN` (1452 B: Ethernet MTU minus IPv6 and UDP headers,
so that nothing is fragmented over IPv4 or IPv6), the device must be able to receive datagrams of this size. The
host accepts datagrams of any size, the device picks its own frame size.

## Reliability

Nothing is retransmitted:

- a lost request (or its reply) times out, as a request to a busy device would;
- a lost stream event is gone;
- a message split across datagrams is dropped if any piece of it is lost or reordered, the framer detects it from the
  remaining length and the CRC, and continues with the next one;
- link setup survives losses, `GetDeviceInfo` is retried;
- if the device goes away silently, the peer timeout ends the session.

Keep messages that must not be lost small enough to fit in one datagram, or use [WebSocket](websocket.md) if the
device can run TCP.

## Device side

There is no ready-made device side yet. A server waits for the first datagram, answers to its sender (e.g., connects
its socket to it) and runs the same [Server](ww_link.md#how-the-device-runs-it) loop as over USB, on top of
`ww_device`'s `FramedTx` / `FramedRx` with a `PacketSink` / `PacketSource` that send and receive datagrams.
`wire_weaver_client/src/udp.rs` has one in its tests, on top of `tokio::net::UdpSocket`.
