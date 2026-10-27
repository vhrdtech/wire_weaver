---
level: advanced
---

# Framer

`ww_framer` packs variable-length **messages** into fixed-size **frames** and unpacks them back on the other side.
It is `no_std`, allocation-free and is meant to sit between the serialized bytes produced by
[ShrinkWrap](../serdes/shrink_wrap.md) and a physical link (USB, CAN, UART, UDP, etc.).

A _frame_ is whatever the underlying medium can carry as one unit: a 512B USB bulk packet, a 8B CAN frame,
a DMA buffer for UART, and so on. A _message_ is what the application cares about: a request, a reply, a stream item.
The two rarely have the same size, which is exactly the problem this crate solves:

- several **small messages** are packed into one frame, so that the link is not wasted on padding;
- a **large message** is split across as many frames as needed and re-assembled on the receiver;
- a frame **does not have to be full** before it is sent — flush early to trade bandwidth for latency.

!!! note

    Understanding the framer is optional. Client (`wire_weaver_client`) and server
    use it internally. Read on if you are implementing your own link or medium, or tuning one. The exact bytes are
    on [framer wire format](ww_framer_wire.md).

## Model

```
 application               ww_framer                   medium
 ──────────────            ──────────────────          ─────────────────
 message A ─┐                                          ┌─ frame 1 ─┐
 message B ─┼── Tx::write ─► [ hA A hB B hC C… ] ── ►  │ hA A hB B │
 message C ─┘                                          └───────────┘
                                                       ┌─ frame 2 ─┐
                                                       │ hC C  …   │
                                                       └───────────┘

 medium                    ww_framer                   application
 frame 1 ──► FramedRx::stage ─► reassemble ─► message() ──► A, B
 frame 2 ──► FramedRx::stage ─► reassemble ─► message() ──► C
```

Each message inside a frame is preceded by a small **head** that carries three things:

| Field       | Meaning                                                                                              |
| ----------- | ---------------------------------------------------------------------------------------------------- |
| `kind`      | `Full`, `Start`, `Continue` or `End` — whether the message fits in this frame or is split            |
| `user_kind` | Small integer chosen by the caller (sub-channel: e.g. `0` = data, `1` = control, `255` = link setup) |
| `len`       | Total message length for `Full` / `Start`, **remaining** length for `Continue` / `End`               |

Optionally a **checksum** over the message and a **tail** may follow the payload. Both are traits and default to
no-ops (`NopChecksum`, `NopTail`), because frame-based media like USB and CAN already check frame integrity.
See [Checksum and tail](#checksum-and-tail) for the ready-made CRCs and an example with both enabled.

Once per frame, before the first head, a **prefix** may be written: a frame counter and a session epoch for media
that lose, duplicate or reorder frames (UDP). It is a fourth generic parameter, `NopPrefix` (nothing) by default,
see [Frame prefix](#frame-prefix).

The pieces are generic parameters:

```rust
use ww_framer::{Tx, FramedRx};
use ww_framer::framed::U2Head;
use ww_framer::traits::{NopChecksum, NopTail};

type LinkTx<'a> = Tx<'a, U2Head, NopChecksum, NopTail>; // prefix: NopPrefix
type LinkRx<'a> = FramedRx<'a, U2Head, NopChecksum, NopTail>;
```

## Writing and reading

`Tx::write` appends a message to the frame being assembled, `flush` hands the frame out. Three messages written into
a 16-byte frame:

```rust
let mut buf = [0u8; 16];
let mut tx = LinkTx::new(&mut buf);

tx.write(0, &[0x01, 0x02, 0x03, 0x04]);        // Ok(true)  — fits
tx.write(1, &[0x11, 0x12, 0x13, 0x14, 0x15]);  // Ok(true)  — fits
tx.write(2, &[0x21, 0x22]);                    // Ok(true)  — fits

let len = tx.flush();                          // 14
let frame = &tx.buf()[..len];
```

`write` returns `Ok(true)` when the whole message has been consumed, so a producer loop can simply keep
writing until it gets `Ok(false)` (frame full, flush it) or runs out of messages (flush what is there).

The receiver yields them one at a time:

```rust
let mut rx_buf = [0u8; 64];
let mut rx = LinkRx::new(&mut rx_buf);

rx.stage(frame)?;
loop {
    rx.reassemble();
    let Some((user_kind, message)) = rx.message() else { break };
    // (0, &[01 02 03 04]) then (1, &[11 12 13 14 15]) then (2, &[21 22])
}
```

`Full` messages are returned **in place** from the staged frame — no copy is made.

A message that does not fit into what is left of the frame is split: `write` fills the frame, returns `Ok(false)`,
and is called again **with the same message** after the frame is sent. A 20-byte message over 8-byte frames:

```rust
let mut buf = [0u8; 8];
let mut tx = LinkTx::new(&mut buf);
let msg: [u8; 20] = core::array::from_fn(|i| i as u8 + 1);

loop {
    let done = tx.write(0, &msg).unwrap();
    let len = tx.flush();
    send(&tx.buf()[..len]);
    if done { break; }
}
```

On the receiving side `stage` + `reassemble` must be called **per frame**, because frame boundaries carry meaning;
the message is delivered once its last piece has arrived. How the pieces look in each frame, and what the receiver
does with lost or reordered frames, is on [framer wire format](ww_framer_wire.md#large-message-spanning-multiple-frames).

## Sending a partial frame early (latency)

`Tx` never sends anything by itself: it only fills the assembly buffer. `flush()` returns how many bytes are
currently in it and rewinds to the start. That means the caller decides _when_ a frame goes out:

- **Throughput-oriented**: keep calling `write` until it returns `Ok(false)`, then flush. Frames are as full as
  possible, per-frame overhead is minimal.
- **Latency-oriented**: flush as soon as there is nothing more to send _right now_, even if the frame is mostly
  empty. The message is sent out immediately instead of waiting for enough traffic to fill the frame.

```rust
let mut buf = [0u8; 64];                      // USB full-speed bulk packet
let mut tx = LinkTx::new(&mut buf);

tx.write(0, &[0xAA, 0xBB, 0xCC, 0xDD]);       // Ok(true)
let len = tx.flush();                         // 5, not 64
usb.write(&tx.buf()[..len]).await;            // short packet goes out now
```

```
 assembly buffer (64 B)
 ┌────┬────┬────┬────┬────┬─────────────────────────────────────── ─ ─ ─ ┐
 │ 04 │ AA │ BB │ CC │ DD │             59 bytes unused                  │
 └────┴────┴────┴────┴────┴─────────────────────────────────────── ─ ─ ─ ┘
  ◄──── sent: 5 bytes ────►◄───────────────── not sent ──────────────────►
```

A typical embedded loop is a hybrid: drain the outgoing queue into the frame, and flush when the queue is empty
_or_ the frame is full — whichever comes first. This gives full frames under load and minimal latency when idle.

```rust
loop {
    let mut pending: Option<(u8, &[u8])> = queue.peek();
    while let Some((uk, msg)) = pending {
        match tx.write(uk, msg) {
            Ok(true)  => { queue.pop(); pending = queue.peek(); } // fully written, next
            Ok(false) => break,                                   // frame full, ship it
            Err(())   => { queue.pop(); pending = queue.peek(); } // too big, drop
        }
    }
    let len = tx.flush();
    if len > 0 {
        link.send(&tx.buf()[..len]).await;
    } else {
        queue.wait_non_empty().await;
    }
}
```

!!! warning "Frame length must be preserved"

    A frame shorter than the maximum is fine, but the receiver must get exactly the bytes that were flushed,
    as one unit, because `Start`/`Continue` payloads are defined as "till the end of the frame". USB and CAN
    guarantee this. Stream media (RTT, UART, TCP) do not: use `write_full` there, which never splits a message,
    so that only `Full` messages are on the wire and the receiver cuts them out by length, wherever a read ends.

!!! note "Frames can also be short for another reason"

    If the remaining space in a frame is too small to fit even a head plus one payload byte, `write` returns
    `Ok(false)` without touching the frame, and the message starts at the beginning of the next one.
    Such a frame is a byte or two shorter than the maximum.
## Latency and priority

The framer only fills frames; when one goes out is decided above it, by the WireWeaver host event loop
(`wire_weaver_client`) and the device server (`ww_device`), with one rule on both ends:

- **Default traffic** (requests at the default priority, their replies, stream and property updates): a frame goes
  out when it is full or when the link's accumulation window is over, whichever comes first. The window is the
  device's `LinkConfig::accumulation_time` (sent to the host in `DeviceInfo`): 1 ms with
  `UsbTimings::fs_higher_speed()`, 250 µs on USB HS. A request or reply alone waits for the window too: it is
  what lets requests made close together share one packet, and it keeps packetization the same from run to run.
- **Never later than the window**: the host waits for it on a timer accurate to tens of microseconds (tokio's own
  counts whole milliseconds and made a 1 ms window cost about 2), so a window of `W` costs at most `W` (CLT-53).
- **Urgent requests** go out at once. Every request carries a 4-bit priority (`wire_weaver::Priority`, 0..=15,
  4 by default); 8 and up (`Priority::URGENT`) is urgent: the host flushes the frame as soon as the request is in
  it, taking what was already waiting along, and the device flushes its reply the same way, also a deferred reply
  sent later through `Server::sink()` (SRV-42).

```rust
let v = client.read_temperature().urgent().read().await?;           // sent and answered at once
let r = client.i2c_read(0x48, 2).priority(Priority::HIGHEST).call().await?;
let client = Board::config(|c| c.default_priority(Priority::URGENT)).connect().await?; // every request
```

A multi request goes at the highest priority of its items. Through the [hub](hub.md) the priority stays as the
client sent it, and the hub's link to the device flushes an urgent request at once as well.

Handlers see the priority as `cx.priority()`, and a deferred method's `cx.reply_to()` keeps it, so
`<method>_send_return(out, reply_to, ..)` answers urgent calls at once without anything else to do. Firmware that
wants to send what waits right now calls `server.sink().sink.flush()` (`ww_device` `Sink::flush`).

The level inside each half orders requests against each other; serving a queue by priority (the host's tx queue,
the device's, the hub's across clients) is planned (CLT-52, SRV-44, SRV-45).

## Checksum and tail

After the payload of a `Full` or `End` message, the framer writes the `Checksum` and then the `Tail`, both
byte-aligned. `Start` and `Continue` carry neither — the checksum covers the **whole** re-assembled message and is
verified once, when the `End` arrives.

```
 ┌──────┬─────────────────┬──────────┬──────┐
 │ head │     payload     │ checksum │ tail │      Full / End
 └──────┴─────────────────┴──────────┴──────┘
 ┌──────┬───────────────────────────────────┐
 │ head │  payload … till the end of frame  │      Start / Continue
 └──────┴───────────────────────────────────┘
```

Both must be **fixed length**: `read` has to consume exactly `LEN_BYTES_*` bytes. The receiver uses these constants
to decide upfront whether a whole message is present in a frame, and `Tx` uses them to decide whether an `End` fits.

The checksum length is configured separately for `Full` and split messages. A medium that checks every frame and
keeps them in order within a session (USB) needs none at all; one that loses frames but checks them (CAN) can take a
small one on split messages only, as a lost frame in between would not be detected by the medium alone; one whose
frames may arrive corrupted (UDP: its 16-bit checksum is weak and optional over IPv4) takes a CRC on every message.
Byte-level examples with a checksum and a tail are on [framer wire format](ww_framer_wire.md#with-a-checksum-and-a-tail).

A checksum covers more than the payload: `Checksum::write` / `read` get a `header` too, the frame's prefix followed by
the head of the piece the checksum belongs to (the `Full`, or the `End` of a split message), so that a flipped bit in
a user kind, a length or the prefix fails it as well.

### Ready-made CRCs: `ww_framer::crc`

Instead of hand-rolling a checksum, use `CrcChecksum<A, FULL>`. `A` selects both the **width** (u8 / u16 / u32 /
u64, written little endian) and the **algorithm**; the provided ones use the [crc](https://docs.rs/crc) crate with a
lookup table computed at compile time. `FULL` (default `false`) controls whether `Full` messages are checksummed too:

```rust
use ww_framer::crc::{CrcChecksum, Crc16Usb, Crc32IsoHdlc};

// 2-byte CRC-16/USB on split messages only — e.g. for CAN, which may lose a frame between pieces
type LinkTx<'a> = Tx<'a, U2Head, CrcChecksum<Crc16Usb>, NopTail>;

// 4-byte CRC-32 on every message — for media without its own frame CRC
type StreamTx<'a> = Tx<'a, U2Head, CrcChecksum<Crc32IsoHdlc, true>, EndMarker>;
```

| Type           | Width   | Algorithm                                  |
| -------------- | ------- | ------------------------------------------ |
| `Crc8Smbus`    | 1 byte  | CRC-8/SMBUS, poly `0x07`                   |
| `Crc16Usb`     | 2 bytes | CRC-16/USB, poly `0x8005` reflected        |
| `Crc16IbmSdlc` | 2 bytes | CRC-16/X-25 (HDLC, PPP), poly `0x1021`     |
| `Crc32IsoHdlc` | 4 bytes | CRC-32 (zlib, Ethernet), poly `0x04C11DB7` |

`CrcAlgorithm::checksum(header, message)` is a plain function, so any other algorithm from the `crc` catalog is a
few lines, and the CRC can also come from elsewhere: a hardware CRC unit, or code shared with the rest of the firmware
(`ww_link`'s UDP configuration takes its CRC-32 from `ww_block::crc32_update`, whose table size the firmware picks
once for both):

```rust
use ww_framer::crc::{CrcAlgorithm, crc_of};

struct Crc8Autosar;
impl CrcAlgorithm for Crc8Autosar {
    type Width = u8;
    fn checksum(header: &[u8], message: &[u8]) -> u8 {
        const CRC: crc::Crc<u8> = crc::Crc::<u8>::new(&crc::CRC_8_AUTOSAR);
        crc_of(&CRC, header, message)
    }
}
type LinkChecksum = CrcChecksum<Crc8Autosar>;
```

### Hand-rolled example

The traits are small enough to implement directly. CRC-8 (poly `0x07`) for split messages only, and a `0x7E` end
marker after every message:

```rust
use shrink_wrap::{BufReader, BufWriter};
use ww_framer::traits::{Checksum, RdError, Tail, WrError};

struct Crc8;

impl Checksum for Crc8 {
    const LEN_BYTES_FULL: usize = 0;   // frame CRC is enough for Full messages
    const LEN_BYTES_SPLIT: usize = 1;

    fn write(header: &[u8], message: &[u8], is_split: bool, wr: &mut BufWriter<'_>) -> Result<(), WrError> {
        if is_split {
            wr.write_u8(crc8(header, message))?;
        }
        Ok(())
    }

    fn read(header: &[u8], message: &[u8], is_split: bool, rd: &mut BufReader<'_>) -> Result<(), RdError> {
        if is_split && rd.read_u8()? != crc8(header, message) {
            return Err(RdError::ChecksumMismatch);
        }
        Ok(())
    }
}

struct EndMarker;

impl Tail for EndMarker {
    const LEN_BYTES: usize = 1;

    fn write(wr: &mut BufWriter<'_>) -> Result<(), WrError> {
        wr.write_u8(0x7E)?;
        Ok(())
    }

    fn read(rd: &mut BufReader<'_>) -> Result<(), RdError> {
        if rd.read_u8()? == 0x7E { Ok(()) } else { Err(RdError::BadTail) }
    }
}

type LinkTx<'a> = Tx<'a, U2Head, Crc8, EndMarker>;
type LinkRx<'a> = FramedRx<'a, U2Head, Crc8, EndMarker>;
```

`message` passed to `Checksum::write` / `read` is always the **complete** message, even for `End`, so the CRC does
not need to be incremental; `crc8(header, message)` is the CRC-8 of `header` followed by `message`.

## Frame prefix

A frame prefix is written by `Tx` once at the start of every frame and read by `FramedRx` once, before the first
head; heads, checksums and tails stay as they are (a checksum covers the prefix, see above). `Tx` keeps a `u32` frame
counter (from 0, up by one per non-empty `flush()`) and a session epoch (up by one per `new_session()`) and hands
them to the prefix, which decides what to write. The trait is `ww_framer::prefix::FramePrefix`:

| Prefix | Bytes | What it carries | For |
| --- | --- | --- | --- |
| `NopPrefix` (default) | 0 | nothing | USB, RTT, byte streams |
| `SeqPrefix` | 1 | `00ee_ssss`: frame counter mod 16, epoch mod 4, 2 spare bits (sent as 0, ignored) | UDP, and USB as an optional diagnostic |

```rust
use ww_framer::prefix::{FrameReport, SeqPrefix};

type UdpTx<'a> = Tx<'a, U2Head, CrcChecksum<Crc32IsoHdlc, true>, NopTail, SeqPrefix>;
type UdpRx<'a> = FramedRx<'a, U2Head, CrcChecksum<Crc32IsoHdlc, true>, NopTail, SeqPrefix>;

match rx.stage(frame)? {
    FrameReport::Gap { lost } => warn!("{lost} frames lost"),
    report if report.is_dropped() => return Ok(()), // duplicate, late, stale, too short: not staged
    _ => {}
}
```

`stage` (and `commit_staged`) check the prefix and say what they found in a `FrameReport`. `SeqPrefix`'s receiver
rebuilds the full counter from its low bits, as the nearest value to the expected one:

| Report | Meaning | The frame |
| --- | --- | --- |
| `Synced` | first frame after `new`, `reset` or `resync`: taken as the reference | decoded |
| `InOrder` | the expected frame | decoded |
| `Gap { lost }` | up to 7 frames missing before it | decoded; a split message in progress is dropped |
| `NewEpoch` | the sender started a new session (epoch one or two on) | decoded; a split message in progress is dropped |
| `Duplicate` | received before (a bitmap of the last 16 frames) | dropped |
| `Late` | from before the expected one, not received before: reordered | dropped (already counted lost) |
| `StaleEpoch` | from the sender's previous session | dropped |
| `Resynced` | the second of two frames in a row that look received and follow each other: 8 or more frames were lost | dropped, taken as the new reference |
| `TooShort` | shorter than the prefix | dropped |

So with a prefix, messages never come out twice or out of order, and a split message never gets a piece from
another session. `prefix_stats()` counts each case (`PrefixStats`), `resync()` takes the next frame as it is (a new
peer), and `Tx::new_session()` drops a half-written message and starts the next frame in a new epoch, so that the
receiver drops whatever of the old session is still on the way. Process every frame (reassemble until `message()`
is `None`) before staging the next one, as with split messages. The bytes are on
[framer wire format](ww_framer_wire.md#frame-prefix).

Limits of 4 + 2 bits: losing 8 or more frames in a row costs two more (the receiver needs two frames to see it), or
up to 8 more when the frames before were lost too; a frame delayed across two session changes is taken as new; a
corrupted prefix can move the receiver to a wrong position before the CRC rejects its frame, after which order is
not guaranteed for a few frames (integrity still is: every message passes its CRC).

## API summary

### `Tx<H, C, T, P = NopPrefix>`

| Method                                      | Description                                                                                                                                                                                                                                |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `new(buf)`                                  | Wrap an assembly buffer. Its length **is** the maximum frame size (e.g. the DMA / endpoint size).                                                                                                                                          |
| `write(user_kind, msg) -> Result<bool, ()>` | Append a message. `Ok(true)` — fully written. `Ok(false)` — frame is full, `flush()` and call again with the **same** message. `Err(())` — the message can never be sent (too large for the head encoding, or head alone fills the frame). |
| `write_full(user_kind, msg) -> Result<bool, ()>` | Same, but never splits: `Ok(false)` if the message does not fit into what is left of the frame, `Err(())` if it does not fit into an empty one. For stream media, where frame boundaries are not preserved. |
| `flush() -> usize`                          | Length of the frame accumulated so far; resets the buffer. Returns 0 if nothing was written. A non-empty frame moves the frame counter on.                                                                                               |
| `new_session()`                             | `reset()`, the epoch goes up by one and frames are counted from 0, see [Frame prefix](#frame-prefix). |
| `frame_ctx() -> FrameCtx`                   | Counter and epoch of the frame being assembled. |
| `buf() -> &[u8]`                            | The assembly buffer. Frame bytes are `&buf()[..len]` right after `flush()`.                                                                                                                                                                |

### `FramedRx<H, C, T, P = NopPrefix>`

| Method                                   | Description                                                                                                             |
| ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `new(buf)`                               | Wrap a receive buffer. Must hold at least one maximum re-assembled message **plus** one maximum frame.                  |
| `stage(frame) -> Result<FrameReport, ()>` | Copy a received frame in, checking its prefix (`FrameReport::Unchecked` without one; a dropped frame is not staged). `Err(())` if `free()` is too small. |
| `staging_buf()` / `commit_staged(len)`   | Receive a frame straight into the free part of the buffer, then stage it (same report). |
| `resync()` / `prefix_stats()`            | Take the next frame's prefix as it is; counters of what the prefixes showed. |
| `free() -> usize`                        | Bytes available for staging.                                                                                            |
| `reassemble()`                           | Process staged bytes until a message is ready or the frame is exhausted. Also discards the previously returned message. |
| `message() -> Option<(UserKind, &[u8])>` | The ready message, if any. Valid until the next `reassemble()`.                                                         |

Intended receive loop:

```rust
rx.stage(frame)?;
loop {
    rx.reassemble();
    let Some((user_kind, message)) = rx.message() else { break };
    handle(user_kind, message);
}
```

Both also have `reset()` (drop whatever is half-written or half-assembled, e.g. on a reconnect) and
`into_parts()` / `from_parts()` to keep the state apart from the buffer. With the `std` feature, `TxOwned` and
`FramedRxOwned` own a `Vec` buffer and have the same methods (`TxOwned::new(frame_size)`,
`FramedRxOwned::new(assembly_size)`); the host transports use them.

### Traits

Everything above is generic over these traits (in `ww_framer::traits` and `ww_framer::prefix`), so a different medium can plug in its own
encoding without touching the packing/splitting logic:

| Trait      | Purpose                                                                                                  | Provided implementations          |
| ---------- | -------------------------------------------------------------------------------------------------------- | --------------------------------- |
| `Head`     | Serialize / parse `(MessageKind, user_kind, len)`; declares `MAX_HEAD_SIZE`                              | `framed::U2Head`                  |
| `Checksum` | Optional fixed-length checksum after each message, separately configurable for `Full` and split messages | `NopChecksum`, `crc::CrcChecksum` |
| `Tail`     | Optional fixed-length bytes after each message (e.g. an end marker for stream media)                     | `NopTail`, `ByteTail`             |
| `prefix::FramePrefix` | Optional fixed-length bytes at the start of every frame, with its own receiver state | `NopPrefix`, `SeqPrefix` |

See [Checksum and tail](#checksum-and-tail) for an example implementation of the last two.

### Cargo features

| Feature      | Effect                                                                 |
| ------------ | ---------------------------------------------------------------------- |
| `std`        | `TxOwned` / `FramedRxOwned`, backed by a `Vec`                         |
| `defmt`      | `defmt::Format` for the prefix types (`FrameReport`, `PrefixStats`)    |
| `large`      | Enables the 3-byte head form: messages up to 128 KiB                   |
| `very_large` | Enables the 4-byte head form: messages up to 16 MiB (requires `large`) |

Without either feature, the maximum message size is 1023 bytes and the head is at most 2 bytes (3 with an extended
user kind), which is what most microcontroller links need.

## Testing and Fuzzing

The framer carries every message over USB, RTT and UDP (WebSocket has framing of its own), so it has to be right.
Unit tests cover the head forms, splitting, the prefix cases and the receiver failure modes listed on the
[wire format page](ww_framer_wire.md#receiver-robustness). Two fuzz targets (from `fuzz/`): `framer-tx-rx` round-trips
random messages over random frame sizes (`just fuzz-framer`), `framer-lossy` runs `SeqPrefix` with a CRC-32 on every
message over a channel that loses, duplicates, delays and corrupts frames, with new sessions on the sender: the
receiver must deliver only sent messages, and in order unless a frame was corrupted (`just fuzz-framer-lossy`).
