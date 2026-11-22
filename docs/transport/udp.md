---
level: basic
---

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

## Device side

The `udp` feature of `ww_device` is the device end: `ww_device::udp::UdpSink` / `UdpSource` send and receive
datagrams on any `DatagramSocket` (receive from anyone, send to an address), and `FramedTx` / `FramedRx` do the
framing on top, as over USB. They run the same [Server](ww_link.md#how-the-device-runs-it) loop. The `embassy-net`
feature adds `EmbassyNetUdpSocket` on an `embassy-net` UDP socket, and `udp_server()` puts it all together:

```rust
let socket = UdpSocket::new(stack, rx_meta, rx_buf, tx_meta, tx_buf);
let socket = EmbassyNetUdpSocket::bind(socket, 9000).unwrap();
let conn = UDP_CONNECTION.init(UdpConnection::new(socket));
let mut server = udp_server(link_config, &*conn, 0, EmbassyClock, UDP_BUFFERS.init(UdpBuffers::new()));
server.run(&mut state).await;
```

### Several hosts on one port

UDP has no connections, so the device tells hosts (peers) apart by their address and serves each from a slot of a
`UdpPeers<Socket, PEERS>`: one socket, up to `PEERS` hosts, chosen at build time. `UdpConnection` is the one-host
table. Each slot has its own `UdpSink` / `UdpSource`, framers and server, so each host has a link of its own, with
its own pings, peer timeout and `LinkEvent`s:

- a host gets a slot when it starts link setup: its datagram is a `Nop` first or the
  [frozen hello](ww_link.md#the-frozen-hello), which the host sends alone before anything else. That slot's server
  reports `LinkEvent::Down(DownReason::Transport)` if it served another host, starts over and handles the datagram
  that started it, so the new host connects without waiting for a retry;
- datagrams from a host without a slot are dropped;
- with every slot taken, the host heard from least recently gives its slot up. A host that is gone stops pinging, so
  it is the first to go; with one slot, any host that starts link setup takes it over, as if another process opened
  a USB device.

A firmware runs one server per slot for good and selects on all of their `wait()`s, passing the same backend to
`handle()`, as with [several media](ww_link.md#how-the-device-runs-it):

```rust
static PEERS: StaticCell<UdpPeers<EmbassyNetUdpSocket<'static>, 2>> = StaticCell::new();
let peers = &*PEERS.init(UdpPeers::new(socket));
let mut a = udp_server(link_config.clone(), peers, 0, EmbassyClock, BUFFERS_A.init(UdpBuffers::new()));
let mut b = udp_server(link_config, peers, 1, EmbassyClock, BUFFERS_B.init(UdpBuffers::new()));
loop {
    match select(a.wait(), b.wait()).await {
        Either::First(ready) => _ = a.handle(ready, &mut state).await,
        Either::Second(ready) => _ = b.handle(ready, &mut state).await,
    }
}
```

In a table of several slots, a slot served for good is up from the start: its server reports the medium up before
any host is heard, and serves the first host whose datagrams reach it (`UdpConnection` waits for its first host). A host that starts link setup waits in its slot until that slot's server
takes it, so hosts that connect at the same time don't push each other out.

A program on a computer instead takes a connection per host, with a backend of its own, as a WebSocket server
accepts clients. `wire_weaver_server`'s `UdpListener` (feature `udp`) does it all, see the
[host server](../api/host_server.md#hosts-on-udp). By hand: `peers.accept(&mut buf)` hands out the slot of each new
host and its address, the program makes that slot's server and drops it when its link goes down, which frees the
slot. With every slot taken, a new host ends the connection heard from least recently and gets the slot from its
next hello, once that connection is dropped.

On a computer the table is a `LockedUdpPeers` (feature `std`): a mutex, held only between awaits, makes it `Sync`, so
connections run on any thread of a multi-threaded runtime. `OwnedFramedTx::owned` and `OwnedFramedRx::owned` are
framers that own their buffers, so a connection holds no borrow of the program's memory and can be spawned:

```rust
let peers = Arc::new(LockedUdpPeers::<_, 8>::new(socket));
let mut buf = Box::new([0u8; UDP_MAX_DATAGRAM_LEN]); // received into while no connection does
loop {
    let (slot, from) = peers.accept(&mut buf).await?;
    // the source first: it holds the slot, and adopts the host when its server starts
    let rx = OwnedFramedRx::owned(UdpSource::new(peers.clone(), slot), MAX_MESSAGE_LEN);
    let tx = OwnedFramedTx::owned(UdpSink::new(peers.clone(), slot), UDP_MAX_DATAGRAM_LEN);
    tokio::spawn(serve(tx, rx, from)); // a server on them until LinkEvent::Down
}
```

The `std` feature adds `StdUdpSocket`, a `DatagramSocket` on the standard library's `UdpSocket` for such a program,
on any event loop: the socket is nonblocking, and the event loop says when it is readable through `Readiness`
(calloop's `Async`, tokio's `readable()`, `async-io`), usually on a clone of the socket (`UdpSocket::try_clone`).
Sends never wait: a datagram the kernel has no room for is dropped, as the network may drop it.

```rust
struct Calloop(RefCell<calloop::io::Async<'static, UdpSocket>>);

impl Readiness for Calloop {
    async fn readable(&self) -> io::Result<()> {
        self.0.borrow_mut().readable().await
    }
}

let socket = UdpSocket::bind("0.0.0.0:7591")?;
let watched = Calloop(RefCell::new(handle.adapt_io(socket.try_clone()?)?));
let peers = Rc::new(UdpPeers::<_, 8>::new(StdUdpSocket::new(socket, watched)?));
```

One socket is read by one slot at a time. With one slot, it receives straight into its framer. With several, the
slot that waits receives into its own framer too; a datagram for another slot goes into the table's handover buffer
and that slot copies it out. Until it does, nothing more is received, so a server that does not wait holds up the
others (no datagram is lost to it). A host waiting for `accept()` holds up nobody. `wire_weaver_client/src/udp.rs`
serves two hosts both ways in its tests, `tests/host_server` many hosts on one port.

There is no authentication, anyone who can reach the port can take a slot. Put the device on a trusted network, or
use a medium with access control.

- No allocation: `UdpBuffers<MAX_MESSAGE_LEN>` holds a slot's reassembly, tx datagram and scratch buffers
  (`3 * MAX_MESSAGE_LEN + 2 * UDP_MAX_DATAGRAM_LEN` bytes). A table of several slots adds one datagram buffer
  (`UDP_MAX_DATAGRAM_LEN`) and about 60 bytes per slot.
- Device datagrams are `UDP_MAX_DATAGRAM_LEN` at most too. The socket's own buffers must fit at least one such
  datagram each way.
- `server.wait()` is cancel-safe, so it can be selected on together with anything else.
- `EmbassyNetUdpSocket` gives up on a send after 1 s (`set_send_timeout`), e.g., when the network interface is
  down, so the link goes down instead of blocking the event loop.
- `UdpPeers` and `UdpConnection` are not `Send` (no lock, one executor), as with the other `embassy` based media;
  on `tokio`, run them on a `LocalSet`, or use `LockedUdpPeers`. `wire_weaver_client/src/udp.rs` has a device on
  `tokio::net::UdpSocket` in its tests.

## Simulated UDP in tests

`wire_weaver_client::in_process::udp_device(path)` registers a device that speaks UDP datagrams without the kernel
(TR-21). The host connects with `ClientConfig::in_process_path(path)` and packs messages exactly as the `udp`
transport does: frame counter, CRC-32 on every message, datagrams of at most `UDP_MAX_DATAGRAM_LEN`. The device serves
the returned `UdpPort` with `UdpConnection`, `UdpSink` and `UdpSource` (slot 0), as it would a real socket. Every connection is
a new host address, so a reconnect is a peer takeover, as over UDP.

A test on tokio's paused clock (`start_paused`) can't use a real socket: the runtime cannot see a datagram that is
still in the kernel, so the clock can jump past it. On the simulated socket a datagram is in the device's queue as soon
as it is sent. The paused clock then drives both ends' accumulation windows and the host's link setup retries
exactly, so which messages share a datagram, and every frame counter after that, is the same on every run, however
loaded the machine is. ww_stdlib's `tests/dfu_scenarios` pins its UDP transcripts this way (`1S-APP-BULK`, `2C-APP-BULK`) and
keeps one run of the same scenario over real sockets, which checks that the update gets through. WebSocket has the
same, see [Simulated WebSocket in tests](websocket.md#simulated-websocket-in-tests).

For loss and reordering, `tests_common::start_device_async_udp` serves a test server on the simulated socket with
every datagram from the host going through an `Impair`, a closure that gives each datagram its `Fate`: `Deliver`,
`Drop`, or `Delay(n)`, delivered after `n` later ones (TEST-19). `tests/totals` drops every 4th datagram and holds
every 7th back behind 3 others, and shows what the frame counter lets through when one is held back behind 12.

`examples_mcu/rp2` has a complete example, `ww_udp_ncm`: the board shows up on the host as a USB CDC-NCM Ethernet
adapter, runs `embassy-net` with a static address and a small DHCP server, so the host is configured automatically,
and serves the blinky API at `192.168.7.1:9000`. Run it with `just run rp2 ww_udp_ncm` from `examples_mcu/rp2`,
then blink from the host with `cargo run -p blinky --features udp --example blinky_udp`.

## Reliability

Nothing is retransmitted:

- a lost request (or its reply) times out, as a request to a busy device would;
- a lost stream event is gone;
- every datagram starts with a frame counter: a duplicated datagram, one that comes late (after a later one), or one
  from the previous session is dropped, a gap is counted as lost (the [link tap](tap.md)'s statistics); the counter has 4 bits,
  so a datagram held back by more than 8 later ones is taken as new, and up to 8 datagrams after it are dropped as
  late (TR-33);
- a message split across datagrams is dropped if any piece of it is lost, the framer detects it from the frame counter,
  the remaining length and the CRC, and continues with the next one;
- every message carries a CRC-32: a datagram corrupted on the way (the UDP checksum is a 16-bit sum, and optional over
  IPv4) is dropped instead of decoded into wrong values;
- link setup survives losses, the hello is retried;
- if the device goes away silently, the peer timeout ends the session.

Keep messages that must not be lost small enough to fit in one datagram, or use [WebSocket](websocket.md) if the
device can run TCP. Changes that add up (a pointer, a scroll wheel) go as [running totals](../api/totals.md): a lost write
loses nothing.

## Wire format

Each datagram is one [ww_framer](ww_framer.md) frame, with `ww_link::UdpFraming`: a one-byte
[frame prefix](ww_framer.md#frame-prefix) (`UdpPrefix`, `SeqPrefix`: frame counter mod 16 and session epoch mod 4),
then the messages with `UdpHead` (`U2Head`) heads and `UdpChecksum`, a CRC-32/ISO-HDLC (the CRC-32 of zip and
Ethernet, `ww_crc`'s `CRC32_ISO_HDLC`) after every message, covering the prefix, the head and the message, and `UdpTail`
(none). Same as over USB, small messages accumulated by the event loop share a datagram and big ones are split across
several. Byte examples are on [framer wire format](ww_framer_wire.md#with-a-crc-32-on-every-message-udp). The hello is
the one datagram outside the framer: no frame prefix, its own CRC, and it is not counted
([the frozen hello](ww_link.md#the-frozen-hello)).

The device starts a new session of its framer on every hello (the epoch goes up), and the host drops
everything before the device's hello with its [setup token](ww_link.md#setup-token). A datagram only starts link setup
(and takes the device over from another host) when its CRC is good.

The host sends datagrams of at most `ww_link::UDP_MAX_DATAGRAM_LEN` (1452 B: Ethernet MTU minus IPv6 and UDP headers,
so that nothing is fragmented over IPv4 or IPv6), the device must be able to receive datagrams of this size. The
host accepts datagrams of any size, the device picks its own frame size.
