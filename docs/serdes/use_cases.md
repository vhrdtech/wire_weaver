# Use cases

Where `shrink_wrap` is used outside the RPC machinery, in two small programs of the vhrd.tech tool chain
(`tpm_mesh_dash` and `tpm_mesh`, which watch and connect a handful of PCs). Nothing here is `no_std`: the point is
that a dense, evolvable format with bounded values is useful on a desktop as well, for files, streams and
size-capped messages. Every size and byte sequence below is asserted by
[`shrink_wrap/shrink_wrap/tests/use_cases.rs`](https://github.com/vhrdtech/wire_weaver/blob/master/shrink_wrap/shrink_wrap/tests/use_cases.rs),
which holds the same types as the snippets (the shipped code names the same items, see each section).

Read [wire format](shrink_wrap.md) and [derive](derive.md) first, this page assumes `Unsized` types, the FIFO of
lengths and [`TailSize<N>`](shrink_wrap.md#size-of-the-rest-of-a-value-tailsizen) are familiar.

## Ring files of a load history

`tpm_mesh_dash` keeps the load history of a PC (CPU, memory, network, ... up to 64 series) in one file per
resolution: a 1 s tier for an hour, 1 min for a day, 15 min for 30 days, 1 h for a year. A tier is a header followed
by a fixed number of record slots, indexed by time (`(ts / period) % slots`), so one `write_at` per sample, nothing
grows, and there is no cursor to lose. This is the case `TailSize<N>` was made for: a value that lives at a fixed
file offset, in a region longer than itself.

```rust
#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct TierHeaderOwned {
    magic: u32,
    size: TailSize<2>,   // bounds the rest, the file region is 4096 bytes
    period: u32,
    slots: u32,
    names: Vec<String>,  // series in use, in slot order
}

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct FineRecordOwned {
    ts: u32,             // start of the slot, a record whose ts is not the expected one reads as empty
    size: TailSize<2>,
    values: Vec<f32>,    // a value per series in use, NaN for none
}

#[derive_shrink_wrap(borrowed, owned, sized, derive(Debug, PartialEq, Clone, Copy, Default))]
struct Stat { min: f32, avg: f32, max: f32 }

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct CoarseRecordOwned {
    ts: u32,
    size: TailSize<2>,
    values: Vec<Stat>,   // the coarser tiers keep min / avg / max of the samples in the slot
}
```

Why the fields are in this order: everything before a `TailSize` must be `Sized` or `SelfDescribing` (a `Vec` keeps
its length at the _back_ of the buffer, which is not where the value ends in a longer region), so `ts`, `magic` and
the like go first and the `Vec` after the slot.

A header with three series, written into the zero-padded 4096-byte region at the start of the file:

```rust
let header = TierHeaderOwned {
    magic: u32::from_le_bytes(*b"TMH2"),
    size: TailSize(0),                       // ignored on write, filled in
    period: 1,
    slots: 3600,
    names: vec!["cpu".into(), "mem".into(), "net".into()],
};
let bytes = header.to_ww_bytes_owned().unwrap();
assert_eq!(bytes, hex!("54 4D 48 32  80 13  01 00 00 00  10 0E 00 00  63 70 75 6D 65 6D 6E 65 74  33 33"));
//                     magic        slot   period       slots        "cpu" "mem" "net"             lengths
region[..bytes.len()].copy_from_slice(&bytes);
let back = TierHeaderOwned::from_ww_bytes_owned(&region).unwrap(); // all 4096 bytes, stops at the header's end
assert_eq!(back.names, header.names);
assert_eq!(back.size, TailSize(19));         // bytes after the slot: 4 + 4 + 9 + 2
```

The header takes 25 bytes of its 4096, and the reader needs neither to know the 25 nor to find the end of the names:
the slot says 19 bytes follow. The same holds for the records, which is what lets them be sized to the series in use
instead of to the worst case:

| Record               | 3 series in use | 64 series (slot capacity in the file) |
|----------------------|----------------:|--------------------------------------:|
| `FineRecordOwned`    |          19 B   |                                264 B  |
| `CoarseRecordOwned`  |          43 B   |                                776 B  |

A record is `ts` (4) + slot (2) + values (4 per series, 12 for a `Stat`) + the reversed length of the `Vec` (1 byte up
to 63 series, 2 at 64), and the capacity is computed from that at compile time in the shipped code and checked in its tests
(`4 + 2 + 64 * 4 + 2`). Be honest about what this buys: the file slot stays at the capacity, so the file does not
get smaller (the 1 s tier is `4096 + 3600 * 264` ≈ 0.95 MB), but a write is 19 bytes instead of 264, and what
matters more:

- a series added later just makes the next records longer, a record written before it reads the new series as
  "no value" (a `Vec` that ends early), no migration of the file;
- a field appended to a record or the header later with `#[default = ..]` keeps old files readable, and old readers
  skip it, because the slot bounds what they parse (see [evolution](../evolution/rules.md));
- a half-written or zeroed slot is an error (`MalformedUVlq32`, `OutOfBoundsSplit`), never garbage values;
- no hand-packed offsets anywhere: the file format is the three structs above.

The shipped code is `src/history.rs` of `tpm_mesh_dash`; a header that does not match (magic, period, slots) means
the file is recreated, as the data is telemetry and not worth a migration.

## Request and reply frames over streams

`tpm_mesh` lets one PC ask another PC's daemon a question next to its gossip (NET-8): a QUIC stream per question
(iroh), one request frame in, one reply frame out. The payload of both is JSON the daemon never looks into, so
`shrink_wrap` only does the envelope:

```rust
#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
pub struct AskRequestOwned {
    pub service: String,   // which local program answers
    pub body: Vec<u8>,     // its request, JSON bytes
}

#[derive_shrink_wrap(owned, ww_repr = u8, derive(Debug, PartialEq))]
pub enum AskReplyOwned {
    Ok(Vec<u8>),           // the provider's JSON bytes
    Err(String),           // or why there is none
}
```

Streams have no message boundaries, so a frame is a `UVlq32` length followed by the message, and the length is
checked against a cap _before_ a byte of the body is read or allocated: 16 KiB for a request, 1 MiB for a reply.

```rust
fn frame<T: SerializeShrinkWrapOwned>(msg: &T, cap: usize) -> Result<Vec<u8>, Error> {
    let body = msg.to_ww_bytes_owned()?;
    if body.len() > cap { /* refuse: over the cap */ }
    let mut wr = BufWriterOwned::with_capacity(body.len() + 5);
    wr.write_uvlq32(body.len() as u32)?;
    wr.write_raw_slice(&body)?;
    wr.finish_and_take()
}

fn unframe<T: DeserializeShrinkWrapOwned>(bytes: &[u8], cap: usize) -> Result<T, Error> {
    let mut rd = BufReader::new(bytes);
    let len = rd.read_uvlq32()? as usize;
    if len > cap { /* refuse */ }
    let body = rd.read_raw_slice(len)?;
    if rd.bytes_left() != 0 { /* refuse: bytes after the frame */ }
    T::from_ww_bytes_owned(body)
}
```

The request `{service: "history", body: {"range":"24h"}}` is 25 bytes on the wire, 24 of them the message:

```text
18                                     frame length, UVlq32 (24)
68 69 73 74 6F 72 79                   "history"
7B 22 72 61 6E 67 65 22 3A 22 32 34 68 22 7D   {"range":"24h"}
07 97                                  reversed lengths of the two fields (7 and 15)
```

and `AskReplyOwned::Err("no")` is 5: `04 | 01 | 6E 6F | 02`, the length, the discriminant (`ww_repr = u8`), the text and
its length. The rules a frame reader enforces, all tested in `tpm_mesh`: over the cap is refused (also on the way
out), a truncated frame is an error, bytes after the frame are an error, and a hostile length such as
`85 80 80 80 00` is refused by the cap before anything is read. The `owned` types allocate, which is fine on a
daemon.

!!! note
    The derive's generated code names `BufReader` and friends unqualified, so such types go into their own module
    with `use shrink_wrap::prelude::*;` if the program also imports another `BufReader` (tokio's here).

## Small messages under a size cap: gossip payloads

`tpm_mesh` announces each PC's state to the others over `iroh-gossip`, which caps a message at 4 KiB. The messages
were JSON, and 7 Oct 2026 was spent finding out what a more compact encoding gains, on 67 live events. Average bytes
per message:

| Message  | JSON | JSON + deflate | `shrink_wrap` | `shrink_wrap` + deflate |
|----------|-----:|---------------:|--------------:|------------------------:|
| bot      |  219 |            162 |           110 |                     112 |
| sessions |  759 |            367 |           364 |                     294 |
| stats    |  809 |            513 |           536 |                     436 |

Then the stats message with a schema designed for the wire instead of ported from the JSON one: percents as `u8`
instead of floats, counts as `UVlq32`, memory in MiB instead of bytes, times as offsets. It is **323 B**, and deflate
gains nothing on it. Two lessons:

- **Schema choice beats general compression on small messages.** `shrink_wrap` alone with a straight port of the JSON
  schema (536 B) is no better than JSON + deflate (513 B), a tight schema (323 B) beats everything including the straight port with deflate (436 B), and nothing is left for deflate to find in it. Deflate on 200 to 800 bytes has little
  history to learn from and pays its own header.
- **Compression pays for string-heavy messages.** `sessions` is mostly text (working directories, titles): there
  `shrink_wrap` + deflate (294 B) is a clear step below `shrink_wrap` alone (364 B) and JSON + deflate (367 B),
  whereas for `bot` it is 2 bytes worse than `shrink_wrap` alone.

A small tight schema looks like this (an illustration of the style, not the shipped type):

```rust
#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct SessionOwned {
    name: String,
    busy: bool,
    cpu_percent: u8,   // not an f32
    mem_mib: UVlq32,   // 512 MiB is 2 bytes, not a u64
}
// {"name":"omarchy-m1","busy":true,"cpu_percent":42,"mem_mib":512} is 64 bytes of JSON, and:
assert_eq!(bytes, hex!("6F 6D 61 72 63 68 79 2D 6D 31  80  2A  84 00  29"));  // 15
//                     "omarchy-m1"                    busy cpu mem    length of name
```

The switch of the gossip payloads to `shrink_wrap` is in progress in `tpm_mesh` as `gossip-sw`; this section
describes its design and the measurements behind it, it has not shipped. The JSON/deflate numbers are measured
on real events, the 323 B figure is the tight stats schema; the type above is only an example.

## Where compression wrappers would pay: time series

[SW-28 `compression-wrappers`](../features.md) (planned: `Compression<T>`, delta, delta-of-delta and float XOR
encodings) is the opposite case of the gossip messages. A gossip message is a snapshot: dozens of unrelated fields,
once, where the only redundancy is in text, so the schema is the lever. A time series is thousands of values of one
field where each is close to the previous, which is where delta-of-delta (timestamps) and float XOR (values, as in
Gorilla) shrink a stream severalfold:

- the `ts` of consecutive records of a tier is `previous + period`: the delta-of-delta is 0, a bit or a byte
  instead of the 4 bytes `u32` per record;
- `f32` load values change slowly, so their XOR with the previous one has long runs of zero bits;
- 15 min and 1 h tiers repeat the pattern at a smaller scale.

The ring files above would _not_ use it per slot, though: they depend on every record being independently readable at
a fixed offset (the `TailSize` slot and the time index), and delta chains break exactly that. Where it would fit is
the data around the rings: an answer to a range question (a chart's worth of up to 720 points per series, sent
through the ask protocol of the previous section), or an archive of old slots written once as a block. A snapshot
message of the gossip kind gains nothing from it.
