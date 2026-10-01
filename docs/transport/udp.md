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

The `udp` feature of `ww_device` is the device end: `ww_device::udp::UdpSink` / `UdpSource` send and receive
datagrams on any `DatagramSocket` (receive from anyone, send to an address), and `FramedTx` / `FramedRx` do the
framing on top, as over USB. They run the same [Server](ww_link.md#how-the-device-runs-it) loop. The `embassy-net`
feature adds `EmbassyNetUdpSocket` on an `embassy-net` UDP socket, and `udp_server()` puts it all together:

```rust
let socket = UdpSocket::new(stack, rx_meta, rx_buf, tx_meta, tx_buf);
let socket = EmbassyNetUdpSocket::bind(socket, 9000).unwrap();
let conn = UDP_CONNECTION.init(UdpConnection::new(socket));
let mut server = udp_server(link_config, conn, EmbassyClock, UDP_BUFFERS.init(UdpBuffers::new()));
server.run(&mut state).await;
```

UDP has no connections, so the device serves one host (peer) at a time and replies to it only:

- a host is adopted when it starts link setup: the first message of its datagram is `Nop` or `GetDeviceInfo`, which
  the host sends alone before anything else;
- datagrams from other hosts are dropped, unless they start link setup: then the new host replaces the current
  one, as if another process opened a USB device. The device reports `LinkEvent::Down(DownReason::Transport)`,
  starts over and handles the datagram that started the takeover, so the new host connects without waiting for a
  retry;
- a host that is gone is noticed by the peer timeout, there is nothing to close.

There is no authentication, anyone who can reach the port can take over. Put the device on a trusted network, or
use a medium with access control.

- No allocation: `UdpBuffers<MAX_MESSAGE_LEN>` holds the reassembly, tx datagram and scratch buffers
  (`3 * MAX_MESSAGE_LEN + 2 * UDP_MAX_DATAGRAM_LEN` bytes). Datagrams are received straight into the framer.
- Device datagrams are `UDP_MAX_DATAGRAM_LEN` at most too. The socket's own buffers must fit at least one such
  datagram each way.
- `server.wait()` is cancel-safe, so it can be selected on together with anything else.
- `EmbassyNetUdpSocket` gives up on a send after 1 s (`set_send_timeout`), e.g., when the network interface is
  down, so the link goes down instead of blocking the event loop.
- The medium is not `Send` (the peer is kept in a `Cell`), as with the other `embassy` based media; on `tokio`, run
  it on a `LocalSet`. `wire_weaver_client/src/udp.rs` has a device on `tokio::net::UdpSocket` in its tests.

`examples_mcu/rp2` has a complete example, `ww_udp_ncm`: the board shows up on the host as a USB CDC-NCM Ethernet
adapter, runs `embassy-net` with a static address and a small DHCP server, so the host is configured automatically,
and serves the blinky API at `192.168.7.1:9000`. Run it with `just run rp2 ww_udp_ncm` from `examples_mcu/rp2`,
then blink from the host with `cargo run -p blinky --features udp --example blinky_udp`.
