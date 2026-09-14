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
    use it internally. Read on if you are implementing your own link, debugging bytes on the wire or just curious.

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
See [Checksum and tail](#checksum-and-tail) for an example with both enabled.

The three pieces are generic parameters:

```rust
use ww_framer::{Tx, FramedRx};
use ww_framer::framed::U2Head;
use ww_framer::traits::{NopChecksum, NopTail};

type LinkTx<'a> = Tx<'a, U2Head, NopChecksum, NopTail>;
type LinkRx<'a> = FramedRx<'a, U2Head, NopChecksum, NopTail>;
```

## Head layout (`U2Head`)

`U2Head` is the head implementation shipped for framed media. It is bit-packed with `shrink_wrap` and takes
**one byte** in the most common case.

### Common form: 1 byte

```
 bit:   7   6   5   4   3   2   1   0
      ┌───┬───┬───┬───┬───┬───┬───┬───┐
      │ m   m │ u   u │ 0 │ l   l   l │
      └───┴───┴───┴───┴───┴───┴───┴───┘
        kind    user    ▲   3-bit length
                kind    │
                        └─ 0 = short length follows
```

- `mm` — `MessageKind`: `00` Full, `01` Start, `10` Continue, `11` End.
- `uu` — `user_kind` `0..=2` stored as-is. `11` means "extended user kind follows" (see below).
- `lll` — length **0..=7** as is.

### Longer lengths

If the length does not fit into the smallest form, the next one is used. Each form starts with a unary prefix
that tells the reader how many length bits to expect:

```
 length < 1 KiB (2 bytes)
 ┌────────┬────────┐
 │mmuu10ll│llllllll│      10 bits of length
 └────────┴────────┘

 length < 128 KiB (3 bytes, feature "large")
 ┌────────┬────────┬────────┐
 │mmuu110l│llllllll│llllllll│      17 bits of length
 └────────┴────────┴────────┘

 length < 16 MiB (4 bytes, feature "very_large")
 ┌────────┬────────┬────────┬────────┐
 │mmuu1110│llllllll│llllllll│llllllll│      24 bits of length
 └────────┴────────┴────────┴────────┘
```

### Extended user kind

`user_kind >= 3` sets `uu = 11` and appends a full 8-bit user kind before the length. This is meant for rare
messages (link setup, diagnostics), where an extra byte does not matter:

```
 ┌────────┬────────┐
 │mm11uuuu│uuuu0lll│                  user_kind = 8 bits, 3-bit length
 └────────┴────────┘
 ┌────────┬────────┬────────┐
 │mm11uuuu│uuuu10ll│llllllll│         user_kind = 8 bits, 10-bit length
 └────────┴────────┴────────┴─ ...    and so on for 17 / 24 bit lengths
```

Example — `Full`, `user_kind = 7`, 4-byte payload:

```
 0x30       0x74       01 02 03 04
 0011 0000  0111 0100  ─────────────
 ││││ └┴┴┴──┴┴┴┘ ││││  user_kind = 0000_0111 = 7
 ││└┘            ││││  11 = extended user kind follows
 └┘              ││││  00 = Full
                 │└┴┘  100 => len = 4
                 └───  0 = short length
```

### Head summary

| `kind` | `user_kind` | `len`     | Head size              |
| ------ | ----------- | --------- | ---------------------- |
| any    | 0..=2       | 0..=7     | 1 byte                 |
| any    | 0..=2       | < 1 KiB   | 2 bytes                |
| any    | 0..=2       | < 128 KiB | 3 bytes (`large`)      |
| any    | 0..=2       | < 16 MiB  | 4 bytes (`very_large`) |
| any    | 3..=255     | as above  | +1 byte                |

## Multiple small messages in one frame

Messages are appended back-to-back, each with its own head. Nothing is padded between them and no separators are
needed — the receiver reads a head, knows the length, skips the payload, reads the next head.

Three messages written into a 16-byte frame:

```rust
let mut buf = [0u8; 16];
let mut tx = LinkTx::new(&mut buf);

tx.write(0, &[0x01, 0x02, 0x03, 0x04]);        // Ok(true)  — fits
tx.write(1, &[0x11, 0x12, 0x13, 0x14, 0x15]);  // Ok(true)  — fits
tx.write(2, &[0x21, 0x22]);                    // Ok(true)  — fits

let len = tx.flush();                          // 14
let frame = &tx.buf()[..len];
```

Resulting frame (14 of 16 bytes used):

```
 byte:  0   1  2  3  4   5   6  7  8  9  10  11  12 13
      ┌────┬───────────┬────┬──────────────┬────┬───────┐
      │ 04 │01 02 03 04│ 15 │11 12 13 14 15│ 22 │ 21 22 │
      └────┴───────────┴────┴──────────────┴────┴───────┘
       head  payload    head   payload      head payload
       ─────────┬──────  ────────┬─────────  ─────┬─────
            message A         message B      message C

 0x04 = 00 00 0 100   Full, uk=0, len=4
 0x15 = 00 01 0 101   Full, uk=1, len=5
 0x22 = 00 10 0 010   Full, uk=2, len=2
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

## Large message spanning multiple frames

When a message does not fit into what is left of the current frame, the framer writes as much as it can, changes
the head kind from `Full` to `Start`, and returns `Ok(false)`. The caller flushes the frame, sends it and calls
`write` again **with the same message**. Continuation frames carry a `Continue` head, the last one an `End` head.

The `Start` head carries the **total** length. `Continue` and `End` heads carry the **remaining** length — how many
bytes of the message are still to come, counting from this head. `Start` and `Continue` payloads always extend to
the end of the frame, so the receiver knows how many bytes each of them brings without any extra field; the
remaining length is there to detect lost frames and to skip an `End` whose `Start` was never seen
(see [Receiver robustness](#receiver-robustness)).

A 20-byte message over 8-byte frames:

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

```
 frame 0 (Start, total = 20)
 ┌────┬────┬────┬────┬────┬────┬────┬────┐
 │ 48 │ 14 │ 01 │ 02 │ 03 │ 04 │ 05 │ 06 │
 └────┴────┴────┴────┴────┴────┴────┴────┘
  ──head─── ────── payload[0..6] ────────
  0x48 0x14 = 01 00 10 00 | 00010100    Start, uk=0, 10-bit len = 20

 frame 1 (Continue, remaining = 14)
 ┌────┬────┬────┬────┬────┬────┬────┬────┐
 │ 88 │ 0E │ 07 │ 08 │ 09 │ 0A │ 0B │ 0C │
 └────┴────┴────┴────┴────┴────┴────┴────┘
  ──head─── ───────── payload[6..12] ────
  0x88 0x0E = 10 00 10 00 | 00001110    Continue, uk=0, 10-bit len = 14

 frame 2 (Continue, remaining = 8)
 ┌────┬────┬────┬────┬────┬────┬────┬────┐
 │ 88 │ 08 │ 0D │ 0E │ 0F │ 10 │ 11 │ 12 │
 └────┴────┴────┴────┴────┴────┴────┴────┘
  ──head─── ───────── payload[12..18] ───
  0x88 0x08 = 10 00 10 00 | 00001000    Continue, uk=0, 10-bit len = 8

 frame 3 (End, remaining = 2)
 ┌────┬────┬────┐
 │ C2 │ 13 │ 14 │
 └────┴────┴────┘
  head  [18..20]
  0xC2 = 11 00 0 010    End, uk=0, len = 2
```

Timeline of `write` return values: `Ok(false)`, `Ok(false)`, `Ok(false)`, `Ok(true)`.

Note that frames 1 and 2 use a 2-byte head because the remaining length (14, 8) exceeds 7, leaving only 6 payload
bytes per frame; the final `End` fits in one byte. With a realistic 64-byte frame this overhead is negligible.

!!! note "`End` always fits in one frame"

    `Start` and `Continue` extend to the end of a frame, but `End` (together with the checksum and tail, if any)
    is written only if it fits **completely** into the current frame. Otherwise the framer writes a `Continue`
    instead and the rest goes into the next frame. `Start` and `Continue` also always leave at least one byte
    for the `End`. This keeps the receiver simple: it never has to hold a half-received `End` across frames.

On the receiving side `stage` + `reassemble` must be called **per frame**, because frame boundaries carry meaning
(`Start`/`Continue` extend to the end of the frame). The message is copied into the front of the receive buffer as
pieces arrive and delivered once `End` is processed:

```
 after frame 0:   [01 02 03 04 05 06 ........................]  assembling  6/20
 after frame 1:   [01 02 03 04 05 06 07 08 09 0A 0B 0C ......]  assembling 12/20
 after frame 2:   [01 .. 0C 0D 0E 0F 10 11 12 ...............]  assembling 18/20
 after frame 3:   [01 .. 14]  ─► message() = Some((0, &[1..=20]))
```

### Split message followed by small ones

An `End` does not have to be alone in a frame. Whatever is left after it is used for the next messages, exactly as in
the [small-messages case](#multiple-small-messages-in-one-frame). 10-byte message over 8-byte frames, then a 1-byte
message on `user_kind = 1`:

```
 frame A                                    frame B
 ┌────┬────┬────┬────┬────┬────┬────┬────┐  ┌────┬────┬────┬────┬────┬────┬────┐
 │ 48 │ 0A │ 01 │ 02 │ 03 │ 04 │ 05 │ 06 │  │ C4 │ 07 │ 08 │ 09 │ 0A │ 11 │ 55 │
 └────┴────┴────┴────┴────┴────┴────┴────┘  └────┴────┴────┴────┴────┴────┴────┘
  Start ──── payload[0..6] ──────────────    End  ─ [6..10] ───────── head payload
  total=10                                   rem=4                    Full uk=1 len=1
```

Frame B is 7 bytes — shorter than the maximum — see the [next section](#sending-a-partial-frame-early-latency).

### Receiver robustness

`FramedRx` tolerates the failure modes of a lossy or restarted link:

- `Continue` with no `Start` in progress → rest of frame skipped (it spans to the frame end anyway).
- `End` with no `Start` in progress → its remaining length says how many bytes to skip; **only that message** is
  dropped and the rest of the frame is decoded normally.
- `Continue`/`End` whose remaining length does not match what the receiver expects → a frame in between was lost;
  the partial message is dropped.
- `Full`/`Start` while assembling → the partial message is dropped, the new one is processed (an `End` was lost).
- `user_kind` changes mid-message → dropped.
- A `Start` or `Continue` that does _not_ extend to the frame end, an `End` that does not fit into the frame
  together with checksum and tail, or a message larger than the receive buffer → skipped.
- Checksum mismatch → **only that message** is dropped, the rest of the frame is decoded normally. Frames are
  already protected by the medium, so a bad checksum on a split message means a frame of it was lost or reordered,
  not that the bytes at hand are corrupt.
- Bad tail → the frame cannot be trusted, the message is dropped and the rest of the frame is skipped.
- Head or payload of a `Full` message cut between two `stage` calls (stream media, byte-by-byte UART) → kept and
  completed by the next `stage`.

After any of these the framer is back in a clean state and the next valid message is decoded normally.

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

### Small messages

Two `Full` messages in a 16-byte frame — no checksum (`LEN_BYTES_FULL = 0`), tail after each:

```
 byte:  0   1  2  3  4   5    6   7  8   9
      ┌────┬───────────┬────┬────┬─────┬────┐
      │ 04 │01 02 03 04│ 7E │ 12 │11 12│ 7E │
      └────┴───────────┴────┴────┴─────┴────┘
       head  payload   tail head  payl. tail
       ────────┬───────────  ────────┬───────
           message A             message B

 0x04 = 00 00 0 100   Full, uk=0, len=4
 0x12 = 00 01 0 010   Full, uk=1, len=2
```

### Split message

The 20-byte message from [above](#large-message-spanning-multiple-frames) over 8-byte frames. Frames 0–2 are
identical to the no-checksum case; only the `End` frame grows by two bytes:

```
 frame 0     48 14 01 02 03 04 05 06        Start, total = 20
 frame 1     88 0E 07 08 09 0A 0B 0C        Continue, remaining = 14
 frame 2     88 08 0D 0E 0F 10 11 12        Continue, remaining = 8

 frame 3 (End, remaining = 2)
 ┌────┬────┬────┬────┬────┐
 │ C2 │ 13 │ 14 │ 99 │ 7E │
 └────┴────┴────┴────┴────┘
  head [18..20]  crc8 tail       0x99 = CRC-8 over all 20 bytes
```

### `End` that would not fit stays `Continue`

A 12-byte message over 8-byte frames. After frame 0 there are 6 bytes left, which _would_ fit into a frame with a
1-byte head — but not together with CRC and tail (1 + 6 + 1 + 1 = 9 > 8). So frame 1 is a `Continue` with 5 bytes,
leaving one for the `End`:

```
 frame 0 (Start, total = 12)
 ┌────┬────┬────┬────┬────┬────┬────┬────┐
 │ 48 │ 0C │ 01 │ 02 │ 03 │ 04 │ 05 │ 06 │
 └────┴────┴────┴────┴────┴────┴────┴────┘

 frame 1 (Continue, remaining = 6) — 6 of 8 bytes used
 ┌────┬────┬────┬────┬────┬────┐
 │ 86 │ 07 │ 08 │ 09 │ 0A │ 0B │
 └────┴────┴────┴────┴────┴────┘
  0x86 = 10 00 0 110   Continue, uk=0, remaining = 6

 frame 2 (End, remaining = 1)
 ┌────┬────┬────┬────┐
 │ C1 │ 0C │ FF │ 7E │
 └────┴────┴────┴────┘
  head [11] crc8 tail       0xFF = CRC-8 over all 12 bytes
```

Frame 1 is sent short instead of being filled with a byte that would force `End` to span two frames.

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
    guarantee this. For stream media (UART, TCP) the framer is not yet ready.

!!! note "Frames can also be short for another reason"

    If the remaining space in a frame is too small to fit even a head plus one payload byte, `write` returns
    `Ok(false)` without touching the frame, and the message starts at the beginning of the next one.
    Such a frame is a byte or two shorter than the maximum.

## API summary

### `Tx<H, C, T>`

| Method                                      | Description                                                                                                                                                                                                                                |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `new(buf)`                                  | Wrap an assembly buffer. Its length **is** the maximum frame size (e.g. the DMA / endpoint size).                                                                                                                                          |
| `write(user_kind, msg) -> Result<bool, ()>` | Append a message. `Ok(true)` — fully written. `Ok(false)` — frame is full, `flush()` and call again with the **same** message. `Err(())` — the message can never be sent (too large for the head encoding, or head alone fills the frame). |
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

### Traits

Everything above is generic over three traits in `ww_framer::traits`, so a different medium can plug in its own
encoding without touching the packing/splitting logic:

| Trait      | Purpose                                                                                                  | Provided implementations          |
| ---------- | -------------------------------------------------------------------------------------------------------- | --------------------------------- |
| `Head`     | Serialize / parse `(MessageKind, user_kind, len)`; declares `MIN_FRAME_SIZE`                             | `framed::U2Head`                  |
| `Checksum` | Optional fixed-length checksum after each message, separately configurable for `Full` and split messages | `NopChecksum`, `crc::CrcChecksum` |
| `Tail`     | Optional fixed-length bytes after each message (e.g. an end marker for stream media)                     | `NopTail`, `ByteTail`             |

See [Checksum and tail](#checksum-and-tail) for an example implementation of the last two.

### Cargo features

| Feature      | Effect                                                                 |
| ------------ | ---------------------------------------------------------------------- |
| `large`      | Enables the 3-byte head form: messages up to 128 KiB                   |
| `very_large` | Enables the 4-byte head form: messages up to 16 MiB (requires `large`) |

Without either feature, the maximum message size is 1023 bytes and the head is at most 2 bytes (3 with an extended
user kind), which is what most microcontroller links need.

## Testing and Fuzzing

This framer implementation is used for all transport media supported by `wire_weaver`.
It is thus very important that it is correct.
This is ensured by complete unit tests coverage and fuzzing.
