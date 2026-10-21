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
control traffic is rare.

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
    GetDeviceInfo { token: u64 },
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
| `GetDeviceInfo`      | host → device | Start of link setup, with the host's [setup token](#setup-token) (`u64` LE). The device starts a new session in its transmit framer.         |
| `DeviceInfo`         | device → host | Link/API/user versions, API hash, max message length the device accepts, requested frame accumulation window, the token.                      |
| `LinkSetup`          | host → device | Host user API version, max message length the host accepts, the token.                                                                        |
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
| `Malformed(Kind)` | payload didn't deserialize — a leftover from a previous session or version mismatch |
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
 GetDeviceInfo { token } ───────────►       (new session in the transmit framer)
                         ◄───────────       DeviceInfo { versions, hash, max_len, acc_time, token }
   (check link and user API compatibility)
 LinkSetup { version, max_len, token } ──►
                         ◄───────────       LinkReady { token }

 ── link up ──────────────────────────────────────

 Data / Ping             ◄──────────►       Data / Ping

 ── either side ──────────────────────────────────
 Disconnect(reason)      ───────────►
                         ◄─────────── OR: Disconnect(reason)
```

Rules the reference event loop follows (and a new transport should too):

- `GetDeviceInfo` is retried every 50 ms, up to 5 times; then link setup is considered failed.
- The host refuses the connection if `DeviceInfo.user_api_version` is not protocol-compatible with the
  generated client API (see [ww_version](../std_library/ww_version.md)). A device may likewise reply with
  `Disconnect(IncompatibleVersion)` after `LinkSetup`.
- `Data` accumulates into the current frame for `packet_accumulation_time_us` (as requested by the device) before
  being flushed; control messages are flushed immediately.
- The host drops every message until the `DeviceInfo` carrying its token, see [Setup token](#setup-token).
- `Data` received before `LinkReady` and `Disconnect` received while not connected are ignored — they are
  stragglers from a previous session.
- Nothing received for `PEER_TIMEOUT_MS` → the host disconnects with an error.
- Each side sends `Disconnect` on the way out when it can, but must not rely on the peer doing so.

### Setup token

A new session can start while the previous one still has bytes on the way: an answer already in the device's USB IN
buffer for a host that went away, the rest of a split message, a reply to a `GetDeviceInfo` the host already
re-sent. Without anything to tell them apart, an old `DeviceInfo` would be taken for the new one. So the host puts a
random, non-zero 64-bit **token** into `GetDeviceInfo` and `LinkSetup` (one per connection, the same on retries),
the device echoes it in `DeviceInfo` and `LinkReady`, and:

- the host drops everything that arrives before the `DeviceInfo` with its token, and a `LinkReady` with another
  one; a wrong match needs a stale reply to carry the same 64-bit value (1 in 1.8·10¹⁹);
- the device takes `LinkSetup` only with the token of the last `GetDeviceInfo`;
- on `GetDeviceInfo` the device starts a new session in its transmit framer (`Transmit::NewSession`,
  `MessageTx::new_session`, `Tx::new_session` in `ww_framer`): a frame being filled and a half-sent message are
  dropped, the reply starts in a fresh frame, and with a [frame prefix](ww_framer.md#frame-prefix) the epoch goes
  up, so that frames of the old session still on the way are dropped by the host's framer;
- there is no USB reset on connect (slow, re-enumerates, kicks other tools off the device).

A peer built before the token sends none (the field reads as 0): a host answers such a `DeviceInfo` with
`Error::IncompatibleLinkVersion` ("without setup token"), a device answers such a `GetDeviceInfo` or `LinkSetup`
with `Disconnect(IncompatibleVersion)`, so a mismatch is a clear error, not a setup timeout. The host also checks
`DeviceInfo::dev_link_version` against its own `LINK_VERSION`.

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

**Tx** handles every command, drives link setup (Nop → GetDeviceInfo with retries → LinkSetup), decides when
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

**`DeviceLink`** answers `GetDeviceInfo` (a new framer session, then Nop flushed alone, then `DeviceInfo` with
the host's token), checks the token and the host version on `LinkSetup` and
replies `LinkReady` or `Disconnect(IncompatibleVersion)`, runs the frame accumulation window and pings, and
declares the host gone after the peer timeout. `GetDeviceInfo` while the link is up means the host application
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
