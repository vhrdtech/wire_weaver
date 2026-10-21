---
level: nomicon
---

# Framer wire format

The exact bytes [ww_framer](ww_framer.md) puts into a frame: the head layout, how small messages share a frame,
how a big one is split, what the receiver does with lost frames, where the checksum and tail go, and the frame
prefix. For
debugging bytes on the wire, writing a framer in another language, or changing this one.

The examples use the types from the [framer page](ww_framer.md#model): `LinkTx` / `LinkRx` with `U2Head` and no
checksum or tail, unless said otherwise.

## Head layout (`U2Head`)

`U2Head` is the head implementation shipped for framed media. It is bit-packed with ShrinkWrap and takes
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
tx.write(0, &[0x01, 0x02, 0x03, 0x04]);
tx.write(1, &[0x11, 0x12, 0x13, 0x14, 0x15]);
tx.write(2, &[0x21, 0x22]);
let len = tx.flush(); // 14
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

A 20-byte message over 8-byte frames, written with the loop from
[writing and reading](ww_framer.md#writing-and-reading):

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

Frame B is 7 bytes — shorter than the maximum — see [sending a partial frame early](ww_framer.md#sending-a-partial-frame-early-latency).

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
- Checksum mismatch → **only that message** is dropped, the rest of the frame is decoded normally: a bad checksum
  on a split message means a frame of it was lost or reordered, or the bytes at hand were corrupted; the heads that
  follow still say where the next message starts (a corrupted one is a bad head or fails its own checksum).
- Bad tail → the frame cannot be trusted, the message is dropped and the rest of the frame is skipped.
- Head or payload of a `Full` message cut between two `stage` calls (stream media, byte-by-byte UART) → kept and
  completed by the next `stage`.

After any of these the framer is back in a clean state and the next valid message is decoded normally. With a
[frame prefix](#frame-prefix), duplicated, late and stale frames are dropped before any of this, and a gap in the
counter drops a split message in progress right away.

## With a checksum and a tail

The examples below use the `Crc8` checksum (CRC-8/SMBUS, on split messages only) and the `0x7E` `EndMarker` tail
from the [hand-rolled example](ww_framer.md#hand-rolled-example). After the payload of a `Full` or `End` message
come the checksum, then the tail; `Start` and `Continue` carry neither. A checksum covers the frame prefix (none
here), the head of its `Full` or `End` and the whole message.

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
 │ C2 │ 13 │ 14 │ A8 │ 7E │
 └────┴────┴────┴────┴────┘
  head [18..20]  crc8 tail       0xA8 = CRC-8 over the head C2 and all 20 bytes
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
 │ C1 │ 0C │ 61 │ 7E │
 └────┴────┴────┴────┘
  head [11] crc8 tail       0x61 = CRC-8 over the head C1 and all 12 bytes
```

Frame 1 is sent short instead of being filled with a byte that would force `End` to span two frames.

## Frame prefix

With a [prefix](ww_framer.md#frame-prefix), every frame starts with it and the first head comes right after.
`SeqPrefix` is one byte:

```
 bit:   7   6   5   4   3   2   1   0
      ┌───┬───┬───┬───┬───┬───┬───┬───┐
      │ 0   0 │ e   e │ s   s   s   s │
      └───┴───┴───┴───┴───┴───┴───┴───┘
        spare   epoch   frame counter
                mod 4   mod 16
```

The spare bits are sent as 0 and ignored on read. The counter starts at 0 and goes up by one per frame sent, the
epoch goes up by one per session (`Tx::new_session`, on link setup), and the counter starts at 0 again.

The three small messages from [above](#multiple-small-messages-in-one-frame) in the first frame of a session: the
same bytes after one more at the front.

```
 00 │ 04 01 02 03 04 │ 15 11 12 13 14 15 │ 22 21 22
 ▲
 └─ epoch 0, frame 0
```

The 20-byte message over 8-byte frames: the prefix takes a byte of every frame, so each piece carries one byte less
than [without it](#large-message-spanning-multiple-frames), and the message takes the same four frames.

```
 frame 0     00 │ 48 14 │ 01 02 03 04 05           Start, total = 20
 frame 1     01 │ 88 0F │ 06 07 08 09 0A           Continue, remaining = 15
 frame 2     02 │ 88 0A │ 0B 0C 0D 0E 0F           Continue, remaining = 10
 frame 3     03 │ C5    │ 10 11 12 13 14           End, remaining = 5
```

Losing frame 1 is now seen at frame 2 from the counter (`Gap { lost: 1 }`), before any head is read.

### With a CRC-32 on every message (UDP)

`ww_link`'s UDP configuration: `SeqPrefix` and `CrcChecksum<CRC-32/ISO-HDLC, FULL = true>`. A 3-byte message and a
12-byte one over 12-byte frames:

```
 frame 0
 ┌────┬────┬──────────┬─────────────┬────┬────┬────┐
 │ 00 │ 03 │ 01 02 03 │ FC 01 6A 7E │ 48 │ 0C │ 01 │
 └────┴────┴──────────┴─────────────┴────┴────┴────┘
  pfx  Full  payload   CRC-32 (LE)    Start     [0]
       len 3           over 00 03 01 02 03   total = 12

 frame 1
 ┌────┬────┬────┬────────────────────────────┐
 │ 01 │ 88 │ 0B │ 02 03 04 05 06 07 08 09 0A │
 └────┴────┴────┴────────────────────────────┘
  pfx  Continue, remaining = 11

 frame 2
 ┌────┬────┬───────┬─────────────┐
 │ 02 │ C2 │ 0B 0C │ A7 BC E4 A4 │
 └────┴────┴───────┴─────────────┘
  pfx  End  [10..12] CRC-32 over 02 C2 and all 12 bytes
```

The CRC of the `Full` covers the frame's prefix `00`, its head `03` and its payload; the one of the `End` covers the
prefix of the `End`'s frame `02`, the `End` head `C2` and the whole message. A flipped bit in a prefix, a user kind or
a length fails it, as one in the payload does.
