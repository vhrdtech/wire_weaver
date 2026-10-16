---
level: advanced
---

# Framer

`ww_framer` packs variable-length **messages** into fixed-size **frames** and unpacks them back on the other side.
It is `no_std`, allocation-free and is meant to sit between the serialized bytes produced by
[shrink_wrap](../serdes/shrink_wrap.md) and a physical link (USB, CAN, UART, UDP, etc.).

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

The three pieces are generic parameters:

```rust
use ww_framer::{Tx, FramedRx};
use ww_framer::framed::U2Head;
use ww_framer::traits::{NopChecksum, NopTail};

type LinkTx<'a> = Tx<'a, U2Head, NopChecksum, NopTail>;
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

The checksum length is configured separately for `Full` and split messages. On USB or CAN a `Full` message is
already protected by the frame CRC, while a split one spans several frames and a lost frame in between would not
be detected by the medium alone — so a typical setup is _no_ checksum for `Full` and a small one for split messages.
Byte-level examples with a checksum and a tail are on [framer wire format](ww_framer_wire.md#with-a-checksum-and-a-tail).

### Ready-made CRCs: `ww_framer::crc`

Instead of hand-rolling a checksum, use `CrcChecksum<A, FULL>` built on the [crc](https://docs.rs/crc) crate.
`A` selects both the **width** (u8 / u16 / u32 / u64, written little endian) and the **algorithm**; the lookup table
is computed at compile time. `FULL` (default `false`) controls whether `Full` messages are checksummed too:

```rust
use ww_framer::crc::{CrcChecksum, Crc16Usb, Crc32IsoHdlc};

// 2-byte CRC-16/USB on split messages only — the usual choice for USB / CAN
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

Any other algorithm from the `crc` catalog is a three-line impl:

```rust
use ww_framer::crc::CrcAlgorithm;

struct Crc8Autosar;
impl CrcAlgorithm for Crc8Autosar {
    type Width = u8;
    const CRC: crc::Crc<u8> = crc::Crc::<u8>::new(&crc::CRC_8_AUTOSAR);
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

    fn write(message: &[u8], is_split: bool, wr: &mut BufWriter<'_>) -> Result<(), WrError> {
        if is_split {
            wr.write_u8(crc8(message))?;
        }
        Ok(())
    }

    fn read(message: &[u8], is_split: bool, rd: &mut BufReader<'_>) -> Result<(), RdError> {
        if is_split && rd.read_u8()? != crc8(message) {
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
not need to be incremental.

## API summary

### `Tx<H, C, T>`

| Method                                      | Description                                                                                                                                                                                                                                |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `new(buf)`                                  | Wrap an assembly buffer. Its length **is** the maximum frame size (e.g. the DMA / endpoint size).                                                                                                                                          |
| `write(user_kind, msg) -> Result<bool, ()>` | Append a message. `Ok(true)` — fully written. `Ok(false)` — frame is full, `flush()` and call again with the **same** message. `Err(())` — the message can never be sent (too large for the head encoding, or head alone fills the frame). |
| `write_full(user_kind, msg) -> Result<bool, ()>` | Same, but never splits: `Ok(false)` if the message does not fit into what is left of the frame, `Err(())` if it does not fit into an empty one. For stream media, where frame boundaries are not preserved. |
| `flush() -> usize`                          | Length of the frame accumulated so far; resets the buffer. Returns 0 if nothing was written.                                                                                                                                               |
| `buf() -> &[u8]`                            | The assembly buffer. Frame bytes are `&buf()[..len]` right after `flush()`.                                                                                                                                                                |

### `FramedRx<H, C, T>`

| Method                                   | Description                                                                                                             |
| ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `new(buf)`                               | Wrap a receive buffer. Must hold at least one maximum re-assembled message **plus** one maximum frame.                  |
| `stage(frame) -> Result<(), ()>`         | Copy a received frame in. `Err(())` if `free()` is too small.                                                           |
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

Everything above is generic over three traits in `ww_framer::traits`, so a different medium can plug in its own
encoding without touching the packing/splitting logic:

| Trait      | Purpose                                                                                                  | Provided implementations          |
| ---------- | -------------------------------------------------------------------------------------------------------- | --------------------------------- |
| `Head`     | Serialize / parse `(MessageKind, user_kind, len)`; declares `MAX_HEAD_SIZE`                              | `framed::U2Head`                  |
| `Checksum` | Optional fixed-length checksum after each message, separately configurable for `Full` and split messages | `NopChecksum`, `crc::CrcChecksum` |
| `Tail`     | Optional fixed-length bytes after each message (e.g. an end marker for stream media)                     | `NopTail`, `ByteTail`             |

See [Checksum and tail](#checksum-and-tail) for an example implementation of the last two.

### Cargo features

| Feature      | Effect                                                                 |
| ------------ | ---------------------------------------------------------------------- |
| `std`        | `TxOwned` / `FramedRxOwned`, backed by a `Vec`                         |
| `large`      | Enables the 3-byte head form: messages up to 128 KiB                   |
| `very_large` | Enables the 4-byte head form: messages up to 16 MiB (requires `large`) |

Without either feature, the maximum message size is 1023 bytes and the head is at most 2 bytes (3 with an extended
user kind), which is what most microcontroller links need.

## Testing and Fuzzing

The framer carries every message over USB, RTT and UDP (WebSocket has framing of its own), so it has to be right.
Unit tests cover the head forms, splitting and the receiver failure modes listed on the
[wire format page](ww_framer_wire.md#receiver-robustness), and a fuzz target round-trips random messages over random
frame sizes: `cargo fuzz run framer-tx-rx` from `fuzz/`.
