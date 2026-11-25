---
level: basic
---

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
check, introspection, pings, timeouts, streams. The client takes a URL: it doesn't browse for
[announced servers](#announcing-on-the-lan) yet, so device filters (serial, label, ...) are not checked before
connecting, and only plain `ws://` is supported, no `wss://`.

`websocket_urls([..])` gives several URLs, opened at once, the first to answer wins (CLT-80); `connect_keep_trying()` makes a client
that connects when the device comes up and again whenever the link goes ([keep-trying connections](keep_trying.md)).

## Device side

The `ws` feature of `ww_device` is the device end: `ww_device::ws::WsTx` / `WsRx` implement `MessageTx` /
`MessageRx` with the [wire format](#wire-format) below, on top of any TCP-like `ws::Socket` (accept one client at a time, read, write).
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

On a PC, the `futures-io` feature (needs `std`) adds `IoSocket`: a `ws::Socket` whose clients are the streams of any
futures-io source, such as a tokio TCP listener's connections wrapped with `tokio_util::compat`
(`examples/blinky/examples/ws_device.rs`):

```rust
let incoming = Box::pin(futures_util::stream::unfold(listener, |listener| async move {
    let (stream, _) = listener.accept().await.ok()?;
    Some((stream.compat(), listener))
}));
let conn = WsConnection::new(IoSocket::new(incoming));
```

A host server with many clients at once, and several APIs on one port, is `wire_weaver_server` with
`WsFraming` ([byte streams](byte_streams.md)).

`examples_mcu/rp2` has a complete example, `ww_ws_ncm`: the board shows up on the host as a USB CDC-NCM Ethernet
adapter, runs `embassy-net` with a static address and a small DHCP server (`edge-dhcp`), so the host is configured
automatically, and serves the blinky API at `ws://192.168.7.1:8080/ww`. Run it with `just run rp2 ww_ws_ncm` from
`examples_mcu/rp2`, then blink from the host with `cargo run -p blinky --example blinky_ws`.

## Announcing on the LAN

A WebSocket server on a LAN can announce itself over DNS-SD (multicast DNS), so that clients and people find it
without a typed address: `avahi-browse -r _wireweaver._tcp` on Linux, `dns-sd -B _wireweaver._tcp` on macOS. The
`ww_dns_sd` crate does it for any server, on any runtime or none: its calls block for a moment (a message to the
responder's thread or a D-Bus round trip) and return.

```rust
use ww_dns_sd::{Advertiser, InstanceId, InstanceName, ResponderChoice, ServiceRecord, Txt};

let txt = Txt::new(
    "/ww",
    &my_api::MY_API_FULL_GID,
    Some(my_api::MY_API_WIRE_VERSION),
    InstanceId::new("tv-living-room")?,
)?;
let record = ServiceRecord::new(InstanceName::new("Living room TV")?, port, txt)
    .with_addresses(ww_dns_sd::interface_addresses(Some("eth0"))?)?;
let mut advertiser = Advertiser::new(ww_dns_sd::start_responder(ResponderChoice::Auto)?, record)?;
// later, when the interface's addresses changed (a new DHCP lease):
advertiser.set_addresses(ww_dns_sd::interface_addresses(Some("eth0"))?)?;
// dropping the advertiser withdraws the record
```

What is announced, as service type `_wireweaver._tcp`:

| Part | Value |
|---|---|
| Instance name | the caller's, one DNS label (63 bytes), shown to people browsing |
| Host name | `ww-<id>.local`, its own, so it never collides with the machine's |
| Addresses | the caller's, each only on the interface whose subnet holds it |
| Port | the WebSocket server's |
| TXT `path` | the request path of the upgrade, `/ww` |
| TXT `api` | the API crate and version, `blinky_api@0.1.0` |
| TXT `wire` | the wire version of the API root, `w1.0`; left out when unknown |
| TXT `id` | a stable instance id the caller keeps, `a-z`, `0-9` and `-` |

`Advertiser::set_addresses` follows a change: the same set in another order sends nothing, a new set replaces the
old one (browsers may report the instance gone for a moment), no addresses withdraw the record until some come
back. Dropping the advertiser withdraws it with a goodbye, so browsers see it go at once. A process that dies
without dropping it leaves the record in caches until its TTL runs out (Avahi drops a dead client's records by
itself).

Two responders, each behind a feature of `ww_dns_sd`:

- `MdnsResponder` (feature `mdns`, on by default) answers in-process: the `mdns-sd` crate on its own thread, joined to
  the mDNS group on every interface, nothing to install. It shares port 5353 with Avahi or systemd-resolved on the
  same box.
- `AvahiResponder` (feature `avahi`) registers with the system's Avahi daemon over D-Bus (`zbus`), where the
  in-process one can't run: port 5353 held without `SO_REUSEPORT`, a sandbox without multicast. Avahi must be
  installed and running there.

`start_responder(ResponderChoice::Auto)` starts the in-process one and falls back to Avahi when it fails to start;
`InProcess` and `Avahi` pick one. Multicast DNS stays on its link: a server in a VM behind NAT, or on another subnet,
is not seen from the LAN.

`examples/blinky`'s `blinky_ws_device` serves the blinky API over WebSocket from a PC and announces it: `cargo run -p
blinky --example blinky_ws_device -- --id blinky-demo --interface eth0`.

!!! warning "Everything announced reaches the whole LAN"

    Every device on the link sees the record, unauthenticated: the server's addresses and port, the API it serves
    with its version, and the instance id. Announcing makes a server easy to find; it was reachable before by
    anyone who could guess or scan the port, and a WebSocket server takes any client that connects
    (peer authentication is planned as TR-24, `ws-auth`). Announce only a server that may be found by everyone on
    that network, never put a secret in the TXT keys or the id (hash a machine id with an application key rather
    than announcing it raw), and pass only the addresses of the interfaces meant for it.

`ww_dns_sd` itself reads nothing from the network except through its responder: `mdns-sd` parses incoming
queries (bounded by its packet size), Avahi does so in its own daemon. Every value the caller passes is checked
against DNS limits before anything is sent: the instance name, the id, the path, each `key=value` within 255
bytes, at most 16 addresses.

## Finding servers on the LAN

`ww_dns_sd::Discovery` (feature `mdns`) browses the LAN for `_wireweaver._tcp` and keeps the list of servers
current as they come, change address and go, on a thread of its own and without an async runtime (CLT-82). Each
`FoundServer` has its instance name, port, path, TXT `api`, `wire` and `id`, and its addresses, IPv4 first.
`wire_weaver_client` re-exports it under its `dns_sd` feature (off by default), and `discovery::urls` hands a
`ClientConfig` a source of WebSocket URLs that is asked again at every connect and every reconnect attempt, so a
keep-trying client follows a server to its new address after a DHCP change:

```rust
use wire_weaver_client::discovery::{self, Discovery, Select};

let discovery = Discovery::start()?;
discovery.on_change(move || ctx.request_repaint()); // called from the browsing thread
for server in discovery.servers().iter() {
    println!("{} {:?} {:?}", server.instance(), server.api(), server.addresses());
}
let client = MyClient::config(|c| {
    c.websocket_urls_from(discovery::urls(
        &discovery,
        Select::Name("Living room TV".into()),
        vec!["ws://100.64.0.5:7590/ww".into()], // tried after the found addresses: a saved or VPN address
    ))
})
.connect_keep_trying()?;
```

`Select` picks a server by instance name, by TXT `id` (stable across renames) or by API crate. While the server is
not found, the attempt has only the fallback URLs; with none, it finds no device and the next attempt asks again.
Browsing stops when the last clone of the `Discovery` is dropped. `Discovery::fed()` gives a discovery that the
caller fills by hand instead, for tests or servers known some other way. `ww_dns_sd::Browser` is the browser
underneath, for a caller that keeps its own list.

The browser runs `mdns-sd` in-process, on every interface. On Android the app must hold a
`WifiManager.MulticastLock` while it browses, or the Wi-Fi driver drops the multicast answers; browse only while
the app is on screen. The `dns_sd` feature builds without `usb`, so a WebSocket-only client (an Android app) builds
no USB code. A found server is what someone on the LAN announced: names are not authenticated, and link setup still
checks the API. Values read from the LAN are checked against the same limits as announced ones; a record that breaks
them is skipped, and at most `SERVERS_MAX` servers are kept.

## Simulated WebSocket in tests

With the client's `in_process` and `ws` features, `wire_weaver_client::in_process::ws_device(path)` registers a device
that speaks WebSocket over an in-memory pipe instead of TCP (TR-22). The host connects with
`ClientConfig::in_process_path(path)` and runs the `ws` transport's own client over the pipe: the same handshake, one
binary message per link message. The device serves the returned `WsPort` with `IoSocket`, `WsConnection`, `WsTx` and
`WsRx`, as it would a TCP listener.

Nothing waits in a kernel, so a test on tokio's paused clock (`start_paused`) drives both ends exactly, and the order
of the host's and the device's frames is the same on every run. ww_stdlib's `tests/dfu_scenarios` pins its WebSocket transcripts
this way (`1L-UPD-BULK`, `2R-APP-STEP`) and keeps one run over a real TCP socket, which checks that the update gets
through. The same holds for UDP, see [Simulated UDP in tests](udp.md#simulated-udp-in-tests).

## Wire format

Each [ww_link](ww_link.md) message is one binary WebSocket message:

```
| kind: u8 | payload ... |
```

`kind` is `ww_link::Kind`, the payload is what the same message carries over other media. The hello is frozen in this form, kind
`04` and its payload ([the frozen hello](ww_link.md#the-frozen-hello), `ww_link::hello::ws`). WebSocket has its own
framing over a reliable stream, so [ww_framer](ww_framer.md) is not used: messages are never split or packed
together, and there is nothing to re-synchronize on. Small messages still share TCP segments: the host buffers them
until the event loop flushes (after the accumulation time) and writes them out in one go. The host's socket has
`TCP_NODELAY` set, so Nagle's algorithm doesn't hold back a flush while an earlier segment is waiting for its ACK,
which would add up to a delayed-ACK timeout (tens of ms) to a request.

Text messages are not expected and drop the connection. Ping and Pong are answered by the WebSocket library and
are not needed to keep the link alive, `ww_link` pings do that.
