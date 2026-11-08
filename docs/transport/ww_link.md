---
level: advanced
---

# Link layer

`ww_link` is the thin protocol that sits between [ww_framer](ww_framer.md) and the application messages
([ww_client_server](../std_library/ww_client_server.md) requests/events). It answers the questions that the framer
deliberately leaves open:

- **who is on the other end** — versions, API hash, buffer sizes are exchanged before any data flows;
- **is the peer still alive** — periodic pings and a peer timeout;
- **how does a session end** — an explicit `Disconnect` with a reason, instead of silence;
- **what goes on which sub-channel** — user data vs. link control messages.

It is `no_std`, allocation-free and does **no IO of its own**: it only defines messages and how they are encoded.
Driving the framer, timers and the actual medium is left to the event loop on each side
(`wire_weaver_client` on the host, [`ww_device`](#how-the-device-runs-it) on the device).

!!! note

    As with the framer, you don't need this to *use* WireWeaver: connect with `wire_weaver_client` and things work.
    Read on if you are implementing a new transport, porting the device side or looking at raw frames.

## Where it sits

```
 ┌───────────────────────────────────────────┐
 │ user API (generated code)                 │
 ├───────────────────────────────────────────┤
 │ ww_client_server   Request / Event bytes  │
 ├───────────────────────────────────────────┤
 │ ww_link            Data / Ping / Setup …  │  ◄── this page
 ├───────────────────────────────────────────┤
 │ ww_framer          messages ⇄ frames      │
 ├───────────────────────────────────────────┤
 │ medium             USB, UART, CAN, UDP …  │
 └───────────────────────────────────────────┘
```

`ww_link` uses the framer's `user_kind` field to tag each message. Values `0..=2` are **data channels** and take the
compact 1-byte head; everything above is a **control message** and costs one extra head byte, which is fine because
control traffic is rare. Only `Data0` is used today; `Data1` and `Data2` are reserved for a later use, not free for
applications, and a device drops them.

## What you get

### Framer configuration

`ww_link` messages are independent of how they are framed — the protocol logic only deals with
`(kind, payload)` pairs, and each medium picks the [ww_framer](ww_framer.md) head / checksum / tail /
[frame prefix](ww_framer.md#frame-prefix) that suits it. `ww_link` names them per medium, as a `Framing` (one type
with all four) and as separate aliases:

| Medium | `Framing` | Prefix | Checksum | Why |
| --- | --- | --- | --- | --- |
| USB | `UsbFraming` | none | none | USB checks every packet, retries, keeps them in order; frames of a previous session are told apart by the [setup token](#setup-token) |
| USB, diagnostic | `UsbSeqFraming` | `SeqPrefix` | none | counts lost, duplicated or reordered packets (a working USB stack has none); the device turns it on, the host follows its interface protocol |
| UDP | `UdpFraming` | `SeqPrefix` | CRC-32 on every message, prefix and head folded in | datagrams get lost, duplicated and reordered, nothing is retried; the UDP checksum is a 16-bit sum, optional over IPv4, and Ethernet's CRC only covers each hop; ShrinkWrap is dense, so a flipped bit often still parses |
| RTT | (`RttHead` / `RttChecksum` / `RttTail`) | none | none | RAM rings read by the probe, nothing corrupted or lost; a byte stream, so only `Full` messages |
| WebSocket | (no framer) | | | TCP below keeps order and retries, TLS (when used) authenticates every record |

```rust
use ww_link::{FramingRx, FramingTx, UdpFraming, UsbFraming};
type UsbTx<'a> = FramingTx<'a, UsbFraming>; // ww_framer::Tx<'a, UsbHead, UsbChecksum, UsbTail, UsbPrefix>
type UdpRx<'a> = FramingRx<'a, UdpFraming>; // ww_framer::FramedRx<'a, UdpHead, UdpChecksum, UdpTail, UdpPrefix>
```

`FramingTxOwned` / `FramingRxOwned` are the `std` variants (`TxOwned` / `FramedRxOwned` of `ww_framer`). On the
device, a `PacketSink` / `PacketSource` of `ww_device` names its medium's `Framing`, and `FramedTx` / `FramedRx` frame
in it. The CRC-32 of UDP is `ww_link::LinkCrc32`, computed by `ww_block::crc32_update`: one implementation for links
and [block devices](../std_library/ww_block.md), with the table a firmware picks once through `ww_block`'s
`crc-table-*` features (bitwise without one, hosts get `crc-table-slice16`). Frame size is whatever the medium
dictates and is passed in when creating the framer (e.g., USB max packet size). Both ends of a link must agree on the
framing: USB tells it in the interface protocol (`USB_INTERFACE_PROTOCOL`, 0x37, or `USB_INTERFACE_PROTOCOL_SEQ`,
0x38, for `UsbSeqFraming`), every other medium has one.

### Messages

```rust
pub enum Message<'i> {
    Data { channel: u8, bytes: &'i [u8] },   // channel 0..=2
    Nop,
    Hello(Hello),                           // the frozen hello, both ways
    DeviceInfo(DeviceInfo<'i>),             // DeviceInfo::token echoes it
    LinkSetup(LinkSetup<'i>),               // LinkSetup::token
    LinkReady { token: u64 },
    Ping,
    GetStats,
    Stats(&'i [u8]),
    Loopback { repeat: u32, seq: u32, data: &'i [u8] },
    Disconnect(DisconnectReason),
}
```

| Message              | Direction     | Purpose                                                                                                                                       |
| -------------------- | ------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `Data`               | both          | Application bytes, opaque to the link. Channel `0` carries `ww_client_server`; `1` and `2` are free for future use.                           |
| `Nop`                | both          | Ignored by the receiver. Sent first after connecting so that a lost first packet (e.g., USB toggle mismatch) doesn't eat something important. |
| `Hello`              | both          | The [frozen hello](#the-frozen-hello): generations and the [setup token](#setup-token). From the host it starts link setup and a new session in the device's transmit framer; the device answers with its own. |
| `DeviceInfo`         | device → host | After the device's hello, when every generation matches: link/API/user versions, API hash, max message length the device accepts, requested frame accumulation window, the token, the user API's wire version. |
| `LinkSetup`          | host → device | Host user API version, max message length the host accepts, the token, client meta, the wire version of the host's client.                   |
| `LinkReady`          | device → host | Setup accepted, `Data` may flow; the token (`u64` LE).                                                                                        |
| `Ping`               | both          | Keep-alive, sent when nothing else was sent for `PING_INTERVAL_MS` (3 s).                                                                     |
| `GetStats` / `Stats` | host ↔ device | Reserved for link statistics, payload not defined yet.                                                                                        |
| `Loopback`           | both          | Test helper: the peer echoes `data` back `repeat` times with increasing `seq`.                                                                |
| `Disconnect`         | both          | Session over; carries a `DisconnectReason` (`RequestByUser`, `CommanderDropped`, `IncompatibleVersion`, `ApplicationCrash`, …).               |

Every message has a `Kind` (`#[repr(u8)]`, `Message::kind()`, `Kind::from_repr()`) which is exactly the framer
`user_kind` it travels under.

### Encoding and decoding

```rust
// sending: Message → (user_kind, payload) → framer
let mut scratch = [0u8; 128];
let (user_kind, payload) = Message::Ping.encode(&mut scratch)?;
tx.write(user_kind, payload)?;

// receiving: framer → (user_kind, payload) → Message
if let Some((user_kind, payload)) = rx.message() {
    match Message::decode(user_kind, payload)? {
        Message::Data { channel: 0, bytes } => { /* hand to ww_client_server */ }
        Message::Ping => {}
        Message::Disconnect(reason) => { /* tear down */ }
        _ => {}
    }
}
```

`Data`, `Stats` and `Loopback` payloads are borrowed straight from the framer buffer — no copies. `DeviceInfo` and
`LinkSetup` are ShrinkWrap structs and are deserialized on demand; with the `std` feature `DeviceInfoOwned` /
`LinkSetupOwned` are available too.

Errors are deliberately small:

| `Error`           | When                                                                                |
| ----------------- | ----------------------------------------------------------------------------------- |
| `UnknownKind(u8)` | `user_kind` is not a `Kind` — ignore, likely a newer peer                           |
| `Malformed(Kind)` | payload didn't deserialize — a leftover from a previous session or version mismatch; a malformed `Hello` or `DeviceInfo` during setup is a peer of an unknown format |
| `ScratchTooSmall` | `encode` was given a buffer too small for `DeviceInfo` / `LinkSetup`                |

### Constants

| Constant           | Value  | Meaning                                                      |
| ------------------ | ------ | ------------------------------------------------------------ |
| `PING_INTERVAL_MS` | 3000   | Send `Ping` if nothing else went out for this long           |
| `PEER_TIMEOUT_MS`  | 10 000 | Consider the peer gone if nothing was received for this long |

Also re-exported for convenience: `DisconnectReason`, `CompactVersion`, `FullVersion`, `ApiHashPair`
(and `*Owned` variants under `std`).

## Session lifecycle

```
 host                                       device
 ────                                       ──────
 Nop                     ───────────►
 Hello { gens, token }   ───────────►       (new session in the transmit framer)
                         ◄───────────       Hello { gens, token }
   (compare generations: a mismatch ends setup here, named)
                         ◄───────────       DeviceInfo { versions, hash, max_len, acc_time, token }
   (check user API compatibility)
 LinkSetup { version, max_len, token } ──►
                         ◄───────────       LinkReady { token }

 ── link up ──────────────────────────────────────

 Data / Ping             ◄──────────►       Data / Ping

 ── either side ──────────────────────────────────
 Disconnect(reason)      ───────────►
                         ◄─────────── OR: Disconnect(reason)
```

Rules the reference event loop follows (and a new transport should too):

- The hello is retried every 50 ms, up to 5 times; then link setup is considered failed.
- Both sides decide on the user API's [wire line](../evolution/rules.md#the-wire-version) (EVO-25), never on its
  crate version. The host refuses the connection when `DeviceInfo.user_api_wire` is on another line than its
  generated client, or `user_api_version` names another API crate. The device likewise replies
  `Disconnect(IncompatibleVersion)` to a `LinkSetup` whose `host_user_wire` is on another line or missing; a dynamic
  host (an empty crate id, no wire version) is always taken.
- `Data` accumulates into the current frame for `packet_accumulation_time_us` (as requested by the device) before
  being flushed, never longer; an urgent request is flushed at once, see
  [latency and priority](ww_framer.md#latency-and-priority). Control messages are flushed immediately.
- The host drops every message until the device's hello carrying its token, see [Setup token](#setup-token).
- `Data` received before `LinkReady` and `Disconnect` received while not connected are ignored — they are
  stragglers from a previous session.
- Nothing received for `PEER_TIMEOUT_MS` → the host disconnects with an error.
- Each side sends `Disconnect` on the way out when it can, but must not rely on the peer doing so.

### Setup token

A new session can start while the previous one still has bytes on the way: an answer already in the device's USB IN
buffer for a host that went away, the rest of a split message, a reply to a hello the host already re-sent.
Without anything to tell them apart, an old `DeviceInfo` would be taken for the new one. So the host puts a random,
non-zero 64-bit **token** into its hello and `LinkSetup` (one per connection, the same on retries), the device echoes
it in its hello, `DeviceInfo` and `LinkReady`, and:

- the host drops everything that arrives before the device's hello with its token, and a `DeviceInfo` or
  `LinkReady` with another one; a wrong match needs a stale reply to carry the same 64-bit value (1 in 1.8·10¹⁹);
- the device takes `LinkSetup` only with the token of the last hello, and only when its generations matched;
- on a hello the device starts a new session in its transmit framer (`Transmit::NewSession`,
  `MessageTx::new_session`, `Tx::new_session` in `ww_framer`): a frame being filled and a half-sent message are
  dropped, the reply starts in a fresh frame, and with a [frame prefix](ww_framer.md#frame-prefix) the epoch goes
  up, so that frames of the old session still on the way are dropped by the host's framer;
- there is no USB reset on connect (slow, re-enumerates, kicks other tools off the device).

A peer built before the hello is still told apart, so a mismatch is a clear error, not a setup timeout:

- a device answers a host's bare token (or a `LinkSetup` without a matching hello) with
  `Disconnect(IncompatibleVersion)`;
- a device from before the hello reads the first 8 bytes of the host's hello as its token and echoes them in
  `DeviceInfo`; the host recognises them and fails with `Error::IncompatibleLinkVersion` ("before the hello").

### The frozen hello

Everything else on a link is evolvable: shrink_wrap inside framer frames. A break in one of those layers would make
the version exchange itself unreadable, an obscure decode error or a setup timeout. So link setup opens with a
**hello** of plain bytes at fixed positions, read before anything evolvable (EVO-23,
`ww_link::hello`). Its payload is the same on every medium and never changes:

| Offset | Size | Field                                                                     |
| ------ | ---- | ------------------------------------------------------------------------- |
| 0      | 2    | magic `WW` (`57 57`)                                                      |
| 2      | 1    | role: `H` (`48`) host, `D` (`44`) device                                  |
| 3      | 1    | `n`, the number of generation bytes that follow (4)                       |
| 4      | n    | generations: framer, link, shrink_wrap, API model (`ww_client_server`)    |
| 4 + n  | 8    | setup token, `u64` little endian                                          |
| 12 + n | ..   | nothing yet; a reader ignores what follows                                |

A **generation** is one byte per foundation layer (`ww_framer::GENERATION`, `ww_link::hello::GENERATION`,
`shrink_wrap::GENERATION`, `ww_client_server::GENERATION`, all 1). It counts only breaks of that layer's bytes:
additions never change it, they are negotiated in `DeviceInfo` and `LinkSetup` after the hello. A later layer gets
its byte appended (`n` grows), and a reader takes the four it knows.

The frame around the payload is frozen per medium too, and both sides write and read it **outside the framer**
(`Framing::hello_frame`, `Framing::find_hello`): every received frame is checked for a hello before the framer sees
it, so a peer whose framer is of another generation still reads the hello and names the framer as what differs.

| Medium    | What is frozen                                                                                          |
| --------- | ------------------------------------------------------------------------------------------------------- |
| USB       | The packet boundary: a hello is one packet of its own, never with a frame prefix (also with the frame counter of `UsbSeqFraming`): `30`, `48 \| (len >> 8)`, `len & 0xff`, then the payload; 19 bytes today (`ww_link::hello::usb`). |
| UDP       | The datagram boundary: a hello is one datagram of its own, without the frame counter every other datagram starts with: the same three head bytes, the payload, then a CRC-32/ISO-HDLC of head and payload, little endian; 23 bytes today (`ww_link::hello::udp`). A corrupted datagram fails the CRC and is not a hello. |
| RTT       | The bytes of the USB frame in the stream, wherever they are: every read is scanned for the peer's hello (`ww_link::hello::rtt::Scanner`), also one split across two reads, and what follows a hello is read anew (`ww_link::hello::rtt`). |
| WebSocket | One binary WebSocket message, kind byte `04` and the payload, the form every link message has there: WebSocket frames the stream, there is no framer to go around (`ww_link::hello::ws`). |

Devices on `embassy-net` send the UDP and WebSocket frames, as their media are.

On USB, `hello::usb::find` takes a packet for a hello only when all of these hold: `30` first, the length bits of the
second byte, the length within the packet, the magic, a role byte and at least four generation bytes. Anything else
is staged into the framer as before, so the tail of an old split message or a frame counter that happens to be `30`
is never taken for a hello. The `30 48 10` head is what framer generation 1 writes for a whole 16 byte `Hello`, so a
generation 1 framer would read the packet too, but nothing relies on that. Endpoints with packets shorter than the
frozen frame (8 or 16 bytes) take the hello framed; WireWeaver's own USB class uses 64 bytes and more. On UDP the CRC
adds to the same checks, and a datagram that starts with a frame counter is never a hello.

On RTT a hello can't be told by where it starts, as the stream has no packets: the scanner looks for the head, the
magic, the peer's role, the generation count and a whole token anywhere in what was read. Bytes before a hello of
another generation may be cut into junk messages by this side's framer, which link setup ignores. The stream has no
checksum, so a message payload that itself carries a whole hello frame of the peer's role would be taken for one; only
a payload made of WireWeaver hello frames does that.

A hello found outside the framer starts a new session on the side that receives it as well: the framer drops what it
held (half a split message of the previous session). It then goes through the same rules as any setup message:
the [setup token](#setup-token) decides whether it belongs to this session.

What each side does with it:

- The device always answers with its own hello. `DeviceInfo` follows only when every generation matches; otherwise
  setup stops there, and a `LinkSetup` that comes anyway is refused.
- The host compares the device's generations with its own and fails with `Error::IncompatibleGeneration`, naming the
  layer: "The device speaks shrink_wrap generation 2, this host shrink_wrap generation 1: update the host". A device
  in the field is stuck and a host is not, so the advice is always about the host (or, for an older device, a host
  that still speaks its generation).
- During setup, a `Hello` or `DeviceInfo` the host can't decode is reported as `Error::UnknownPeerFormat` with its
  bytes, never as a bare decode error.
- The hello costs bytes only in link setup, once each way per connection: nothing is added to any message after
  `LinkReady`.

## How the host runs it

`ww_link` itself is just messages. The host side (`wire_weaver_client`) drives them with two small
**sans-IO state machines** — one per direction — that contain all the protocol logic but do no IO, own no
timers and hold no channels. A thin per-transport wrapper supplies those.

```
                 commands                              frames
 Commander ───────────────► TxCore ──► Send / Flush ──► framer ──► medium
                              ▲  │
                         ToTx │  │ ToRx          (plain enums, ferried by the wrapper)
                              │  ▼
 responses, streams ◄──── RxCore ◄── (kind, payload) ◄── framer ◄── medium
```

**Tx** handles every command, drives link setup (Nop → hello with retries → LinkSetup), decides when
to flush the accumulated frame, sends pings and allocates request IDs.
**Rx** decodes messages, does the version check, routes responses and stream data to waiting callers, times out
requests and detects a silent peer.

Both are fed inputs (`Command`, `Message`, `Timer`, ...) with an explicit `now`, and hand back outputs
(`Send`, `Flush`, `Connect`, `Exit`) plus a few messages for the other half:

- rx → tx: `DeviceInfo` accepted, `LinkReady`, request ID `Freed`, `PeerGone`
- tx → rx: `Expect` this response, subscribe to a stream, `TransportUp`, `Stop`

### Why two halves

A device is usually **half-duplex**: it reads a packet, answers, and does not read again until the answer is
written out. If the host behaves the same way — awaiting a write while not reading — both sides eventually block
on each other as soon as a burst of requests produces more replies than the host has receive buffers for. It
shows up as a stall followed by a write timeout, and is hard to reproduce on real hardware.

Running rx and tx as **independent tasks** removes the cycle: the host always drains what the device writes, so
the device always gets back to reading, so host writes always complete. Backpressure falls out for free — a slow
device just means the tx task is parked in a write, so the command channel fills up and
`Commander::send().await` waits. No watermarks, no prioritised selects.

### What the split buys

- **Testable without a runtime or hardware.** Both cores are exercised in unit tests by hand-advancing time and
  ferrying `ToTx`/`ToRx` between them; the half-duplex deadlock has a regression test against a mock device.
- **One protocol implementation, any wrapper.** The async USB wrapper is ~200 lines of plumbing. A blocking
  transport is two `std::thread`s: tx does `blocking_recv` on commands plus a timer, rx does a read with timeout.
  No `select!` at all.
- **Framing is the wrapper's choice.** Cores speak `(kind, payload)`; USB uses `U2Head` without a checksum, UDP adds
  a frame counter and a CRC-32, another medium can pick differently or skip framing if it already delivers whole
  messages.
- **Same shape as the device side.** The device uses the same sans-IO core + message transport + thin wrapper
  layering, see below.

## How the device runs it

The device side lives in `ww_device` (`no_std`, no alloc) and follows the same layering as the host, but keeps the
event loop in **user code**, so that any other async source (UART data, sensors, timers) can be awaited right
next to the link:

| Host (`wire_weaver_client`)                           | Device (`ww_device`)                                               |
| ----------------------------------------------------- | ------------------------------------------------------------------ |
| `TxCore` / `RxCore` sans-IO state machines            | `DeviceLink` sans-IO state machine                                 |
| `MessageTx` / `MessageRx` + nusb + framer             | `MessageTx` / `MessageRx`, `FramedTx` / `FramedRx` over packets, `StreamTx` / `StreamRx` over bytes |
| `worker` with `tokio::select!`, two tasks             | user loop with `Server::wait()` / `Server::handle()`, or `run()`   |
| —                                                     | `blocking::Server` for devices without async                       |

**`DeviceLink`** answers a hello (a new framer session, then Nop flushed alone, then its own hello, then `DeviceInfo`
with the host's token when the generations match), checks the token and the host's API and wire line on `LinkSetup` and
replies `LinkReady` or `Disconnect(IncompatibleVersion)`, runs the frame accumulation window and pings, and
declares the host gone after the peer timeout. A hello while the link is up means the host application
restarted without disconnecting: the old session is dropped and a new one starts. Control replies are queued as
a few flags (no queue, no allocations) and handed out by `poll_transmit()`; link state changes come out of
`poll_event()` as `LinkEvent::Up` / `Down(reason)`.

**`Server`** (async) splits each step in two:

- `wait()` is cancel-safe and only waits — for a received message, the core's next deadline, or the medium coming
  up. A received message stays in the framer's buffer, so the returned value borrows nothing.
- `handle(ready, &mut backend)` runs to completion: link setup, `process_bytes()` on the backend, writing replies,
  pings and flushes.

```rust
loop {
    match select3(server.wait(), uart_rx.wait_read(), ticker.next()).await {
        Either3::First(ready) => {
            if let Some(event) = server.handle(ready, &mut state).await {
                // LinkEvent::Up / Down, e.g., to drive an LED
            }
        }
        Either3::Second(chunk) => {
            _ = stream_data_ser().uart_rx_send(&chunk, &mut server.sink()).await;
        }
        Either3::Third(_) => { /* periodic updates through server.sink() */ }
    }
}
```

`server.run(&mut backend)` is that loop with nothing else in it. Writes are awaited inline: that is safe since
the host reads independently of writing (see [Why two halves](#why-two-halves)), and avoids mutexes. Backend
handlers should not block for long though, as nothing is received meanwhile — use deferred replies instead.

If the backend fails to process a request entirely (it could not even serialize an error), a generic error
event (`ResponseSerFailed`, `err_seq = u32::MAX`) is sent back, so that the host does not wait for a timeout.

**`blocking::Server`** is the same for devices without an async runtime: received packets are pushed in with
`on_packet()` (e.g., from a USB interrupt), time is advanced with `poll(now)`, frames go out through a blocking
`PacketSink`. Backends generated with `use_async = false` implement `WireWeaverApiBackend`.

Time is a plain `ww_device::Instant` (µs) provided by the caller; the async server takes a small `Clock` trait
(`EmbassyClock` with the `embassy-time` feature). `wire_weaver_usb_embassy` only adds the USB class, packet IO
for the endpoints and `usb_init()`, which returns the `UsbDevice` to run and the `Server`.

## Features

| Feature  | Effect                                                                                                         |
| -------- | -------------------------------------------------------------------------------------------------------------- |
| _(none)_ | `no_std`, borrowed `Tx`/`Rx`, borrowed `DeviceInfo`/`LinkSetup`.                                               |
| `std`    | Adds `DeviceInfoOwned`/`LinkSetupOwned` (+ `make_owned()`), owned version types. Used by `wire_weaver_client`. |
| `defmt`  | `defmt::Format` on `Kind`, `Error`, `DeviceInfo`, `LinkSetup` for embedded logging.                            |

Framer features `large` and `very_large` are enabled unconditionally, so messages up to 16 MiB can be described by
the head; actual limits are negotiated via `dev_max_message_len` / `host_max_message_len`.

## Relation to other crates

- [ww_framer](ww_framer.md) — packs `ww_link` messages into frames; `ww_link` only picks its parameters.
- `wire_weaver_client` — host event loop: two sans-IO state machines (tx: commands, setup retries, accumulation,
  ping, seq allocation; rx: decoding, version check, dispatcher, peer timeout) run as independent tasks so that
  receiving is never blocked by a write.
- `ww_device` — device side: sans-IO `DeviceLink`, async and blocking servers, see [above](#how-the-device-runs-it).
- `wire_weaver_usb_embassy` — USB class and packet IO for embassy-usb on top of `ww_device`.
- [USB](usb.md), [RTT](rtt.md), [UDP](udp.md) — transports that carry frames; [WebSocket](websocket.md) carries
  `ww_link` messages without the framer.
