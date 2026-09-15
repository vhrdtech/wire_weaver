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
(`wire_weaver_client` on the host, TBD on the device).

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
`(kind, payload)` pairs, and each transport picks the [ww_framer](ww_framer.md) head
/ checksum / tail that suits its medium. For convenience a default configuration is provided:

```rust
use ww_link::{UsbHead, UsbChecksum, UsbTail};  // U2Head, CRC-16/IBM-SDLC per message, no tail
type UsbTx<'a> = ww_framer::Tx<'a, UsbHead, UsbChecksum, UsbTail>;
type UsbRx<'a> = ww_framer::FramedRx<'a, UsbHead, UsbChecksum, UsbTail>;
```

This is what USB uses (`TxOwned`/`FramedRxOwned` variants with the `std` feature of `ww_framer`). A medium with its own integrity check might drop the checksum,
a stream medium (UART) would add a tail for synchronization, and so on — the link messages stay the same.
Frame size is whatever the medium dictates and is passed in when creating the framer (e.g., USB max packet
size). Both ends of a link must of course agree on the configuration.

### Messages

```rust
pub enum Message<'i> {
    Data { channel: u8, bytes: &'i [u8] },   // channel 0..=2
    Nop,
    GetDeviceInfo,
    DeviceInfo(DeviceInfo<'i>),
    LinkSetup(LinkSetup<'i>),
    LinkReady,
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
| `GetDeviceInfo`      | host → device | Start of link setup.                                                                                                                          |
| `DeviceInfo`         | device → host | Link/API/user versions, API hash, max message length the device accepts, requested frame accumulation window.                                 |
| `LinkSetup`          | host → device | Host user API version and max message length the host accepts.                                                                                |
| `LinkReady`          | device → host | Setup accepted, `Data` may flow.                                                                                                              |
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
`LinkSetup` are `shrink_wrap` structs and are deserialized on demand; with the `std` feature `DeviceInfoOwned` /
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
 GetDeviceInfo           ───────────►
                         ◄───────────       DeviceInfo { versions, hash, max_len, acc_time }
   (check user API compatibility)
 LinkSetup { version, max_len }  ──►
                         ◄───────────       LinkReady

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
- Introspection and dynamic clients are also supported, both from Python and GUI.
  Great for debugging and quick tests.
- `Data` accumulates into the current frame for `packet_accumulation_time_us` (as requested by the device) before
  being flushed; control messages are flushed immediately.
- `Data` received before `LinkReady` and `Disconnect` received while not connected are ignored — they are
  stragglers from a previous session.
- Nothing received for `PEER_TIMEOUT_MS` → the host disconnects with an error.
- Each side sends `Disconnect` on the way out when it can, but must not rely on the peer doing so.

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
- **Framing is the wrapper's choice.** Cores speak `(kind, payload)`; USB uses `U2Head` + CRC-16, another medium
  can pick differently or skip framing if it already delivers whole messages.
- **Mirrors the device side.** Embedded implementations already run separate rx/tx tasks, so porting the device
  onto `ww_link` follows the same shape.

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
- `wire_weaver_usb_link` — current device-side implementation; being migrated onto `ww_framer` + `ww_link`.
- [USB](usb.md), [WebSocket](ws.md), [UDP](udp.md) — transports that carry frames.
