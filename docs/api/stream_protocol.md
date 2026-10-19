---
level: nomicon
---

# Stream protocol

How streams and sinks work on the wire and inside: the head of counted and reliable stream events, their
acknowledgements, the credit messages of credit sinks, and the `ww_stream` state machines generated code drives. For
using streams, read [Streams](streams.md), [Counted and reliable streams](counted_streams.md),
[Stream delivery](stream_reliability.md) and [Credit sinks and sideband](credit_sinks.md) first. The wire types live
in `ww_client_server` ([its page](../std_library/ww_client_server.md)).

## Opening and closing

On the wire, opening is a `RequestKind::StreamOpen { resume, args }` request, answered with a
`ww_client_server::stream::StreamOpened` (the session and positions, empty for a plain stream) followed by the
function's result. Closing is a `StreamSideband::Close` request from the host, or a `Close` event from the device
when the stream ends on its own.

## Head on the wire

A counted or reliable stream sends `EventKind::StreamItem` instead of `StreamData`:

```rust
// ww_client_server
pub enum EventKind<'i> {
    // ... Value, Written, StreamData, StreamSideband
    StreamItem { path: RefVec<'i, UNib32>, head: StreamHead, data: TailBytes<'i> },
}

// ww_client_server::stream
pub struct StreamHead {
    pub pos: u64,             // position of the event's first unit since the session started
    pub units: u64,           // units of the event's item
    pub lost: Option<u64>,    // units the device dropped right before this event; `pos` counts them
    pub t: Option<Second>,    // device time of the first unit
    pub ext: Option<HeadExt>, // rare markers: the clock epoch changed (a reboot)
}
```

The head is laid out by hand, so that its four flags share the first byte with the length of `pos`:

```
 byte 0:  L T X U n n n p      L, T, X, U: lost, t, ext, units follow (units only when not 1)
                               nnn: bytes of pos after this one (0..7), p: the top bit of pos
 then:    n bytes of pos (1 + 8n bits in all, big endian)
          units: UVlq64             if U
          lost:  UVlq64             if L
          t:     ww_si::Second      if T (starts on a byte, so its prefix and type tag share one byte)
          ext:   HeadExt            if X (Unsized: fields can be appended later)
```

| `pos` below | 2 | 512 | 128 Ki | 32 Mi | 8 Gi | 2 Ti | 512 Ti | 2⁵⁷ |
|---|---|---|---|---|---|---|---|---|
| Head bytes (one unit, no `lost`, `t`, `ext`) | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |

`units` (when not 1) and `lost` add 1 byte each below 128, 2 below 16 Ki, and so on; `t` as `Second { Micro, U32 }`
adds 5 bytes, as `U64` 9. A stream of single items pays nothing for `units`, a byte stream 1-2 bytes. A position of
2⁵⁷ or more is a serialization error.

Example, from the tests of `ww_client_server::stream`: `pos` = 2³³, `units` = 3, `lost` = 7 and a `t` of
`Second { Micro, U32(0x12345678) }` is the 13-byte head `da 02 00 00 00 00 03 07 23 78 56 34 12`; with the event header
and a one-element path, 17 bytes come before the item's data.

A `StreamData` event costs 3 to 5 bytes of header and path before its data, so a long-running counted stream with a
timestamp pays about 15 bytes per event: 3 % of a 512-byte item, 1 % of 1.5 KiB. Send small items in chunks (an item
of type `MaxLen<RefVec<'i, Record>, N>`, counted in records), not one event per record.

## Acknowledgements and resends

A reliable stream's receiver (the host) tells the sender (the device) what it has. Positions are in units, grants in
bytes, since what runs out on the receiving side is memory:

| Sideband | Direction | Meaning |
|---|---|---|
| `Ack { have, backfill, grant }` | receiver → sender | received every unit before `have` (live run) and before `backfill` (backfill run, while there is one); the sender may have up to `grant` bytes of events in flight beyond them |
| `Resend { from, grant }` | receiver → sender | units from `from` on are missing, in whichever run `from` falls: send again from there |
| `Query { sent, backfill }` | sender → receiver | "I have sent up to here, where are you?", when out of credit or nothing was heard for a while; answered with `Ack` or `Resend` |

All three carry the whole state, so losing one is harmless: the next one replaces it.

- **The device** keeps every event in its store until an `Ack` passes it, sends while the bytes in flight before the
  next event are below the grant (so a grant smaller than an item can't stall the stream), and goes back on
  `Resend` (go-back-N). A `Resend` acknowledges nothing (with `Explicit` the host may have received more
  than its user code confirmed); the host sends an `Ack` right before it when its position moved. With no host,
  nothing is acknowledged: the store fills, then data is dropped and counted.
- **The host** accepts events of each run in order only, drops duplicates, asks for a resend once per gap, and
  acknowledges every half window or 100 ms. The window is what the host queues for the stream (4 MiB by default): a
  host that doesn't read stops granting, the device stops sending and keeps the data in its store, so the host's
  memory stays bounded.
- **When data counts as received** (`AckMode`): by default when user code takes an event; with `Explicit`, only when
  user code confirms it, for example after writing it to disk.
- Defaults: a resend is asked again after 500 ms, the device queries after 200 ms of silence, the first grant is 4 KiB.

### Positions inside an event

An event of several units (a block of samples, a batch) has positions inside it, and the host can stand at one of
them: user code confirmed part of a batch (`ack(pos)` with `Explicit`, items saved one by one) and resumes from that
cursor, or a `Resend` comes from such a position. The device never splits an event: it sends the event that holds
the position, whole and with its own `pos` (a store reads "the event that holds `pos`, or the first one after it").
The host takes it without the units it has (`Verdict::Accept { skip }` of `ww_stream::rx::Receiver`): a typed
`PositionedStream` gives it with `pos` and `units` of the new part, the value is the whole item. The same holds for
ranges the host had before a resume (they may end inside an event). An event that is all had is a duplicate.

On USB the link doesn't lose, so `Resend` never happens and an `Ack` is a few bytes every half window: a reliable
stream costs almost nothing more than a counted one there, and adds backfill and bounded host memory.

## Credit sinks on the wire

A credit sink is its own resource kind (`ApiItemKind::FlowSink { ty, flow: FlowControl::Credit }` in `ww_self`), so
turning a plain sink into a credit sink is an API break: its writes change. Each write is a
`ww_client_server::CreditWrite { pos: UVlq32, data: TailBytes }` inside `RequestKind::Write`, `pos` being the byte
position of the item's first byte since the session's `Open`, modulo 2^28 (see [below](#positions-wrap-at-256-mib)):
1 byte for the first 128 bytes of a session, 2 up to 16 KiB, 3 up to 2 MiB, never more than 4. Three sideband
messages carry the rest:

| Sideband | Direction | Meaning |
|---|---|---|
| `Credit { have, grant }` | device → host | `have` bytes of items accepted in order; the host may send up to byte `have + grant` |
| `CreditQuery { sent }` | host → device | "I have sent up to byte `sent`, where are you?", answered with `Credit` or `Resend` |
| `Resend { have, grant }` | device → host | writes after `have` were lost: send again from byte `have` (an item boundary), up to `have + grant` |

All three carry the whole state, so losing one is harmless: the next one, or the answer to the next query, replaces
it. Their positions are `UNib32`, modulo 2^28 as well.

1. `Open` starts a session: the device resets its counters and answers with `Credit { have: 0, grant: capacity }`.
   Credit events left over from an earlier session are ignored by the host until this one arrives.
2. The host sends an item while `pos + its length <= have + grant`, keeping a copy of everything after `have` for a
   resend (at most the device's capacity).
3. The device counts accepted bytes (`have`) and bytes it **freed** (consumed, e.g. written to flash); its limit is
   `freed + capacity`. When the limit has moved by at least `threshold` since the last grant, it sends a new
   `Credit`: a large threshold means few acknowledgements over a slow link, a small one keeps the link full.
4. When the host is out of credit and nothing arrives for `query_interval`, it sends `CreditQuery`. This recovers from a
   lost `Credit`, from a device that freed less than `threshold`, and finds lost data at the end of a transfer.
5. `flush()` sends `CreditQuery` and waits until `have == sent`: on a reliable link with a buffer that holds the whole
   transfer, the one acknowledgement it needs.

On a lossy medium (UDP, LoRa, LTE-M) the device accepts a write only at `pos == have`: one before it is a duplicate
from a resend and is dropped; one after it means something was lost, so it is dropped too and the device sends
`Resend { have, grant }`, once per gap. A `CreditQuery { sent }` with `sent > have` means writes were lost after the
last one that arrived; the device answers with `Resend` instead of `Credit`. On `Resend` the host goes back to `have`
and sends everything again from its copy (go-back-N). Requests are processed in order, so when the device sees a
`CreditQuery`, everything sent before it has arrived or is lost; a plain `Credit` never rewinds the host, the rest may
still be in flight. An empty item takes one position, so its loss is noticed like any other.

### Positions wrap at 256 MiB

Both sides count positions in `u64`, so a session has no size limit. On the wire a position is sent modulo 2^28
(`credit::POS_MODULUS`, `credit::wire_pos()`), the most a `UVlq32` holds in 4 bytes, and the receiver takes the full
position nearest to the one it expects (`credit::extend_pos()`: from 2^27 behind it to 2^27 ahead, serial number
arithmetic as in TCP). A value of 2^28 or more is no position: the device drops such a write (`CreditVerdict::Invalid`),
the host ignores such a credit event.

That reads every position right as long as it is less than 2^27 (128 MiB) away from what the receiver expects. Credit
keeps it so: what the host has in flight, sent but not acknowledged, never exceeds the device's buffer, and the buffer
is at most `credit::MAX_WINDOW`, 64 MiB, a quarter of the modulus. `SinkCredit::new` and `sized_for` refuse a larger
one (a compile error in a `const`), `try_sized_for` and `set_capacity` return `SinkCreditError::BufferTooLarge`, and
the host never puts more than `MAX_WINDOW` in flight whatever the device grants. One window on either side of the
reference takes half the modulus; the other half is the margin for a late packet, a duplicate held back on UDP. Such a
packet looks like a write from ahead (a gap, answered with a `Resend`) only when it arrives more than 64 MiB of traffic
late, and like the expected write only when exactly a multiple of 256 MiB late: half a minute at 10 MB/s, far beyond
how long a datagram lives on a network. Ordered links (USB, RTT, WebSocket) never deliver a late packet at all.

The modulus is the same on every link and fixed by the protocol: nothing is announced at `Open`, there is nothing to
configure.

Credit sinks are planned to move onto the positions, acknowledgements and resume of counted and reliable streams
(`API-34` in the [Feature tracker](../features.md)).

## Sending: producers, the event loop and priorities

A `ww_stream::tx::Reliable` is one state: the producer pushes events into it, whether a host is there or not, and the
event loop reads the next event out of it and sends it. Shared between tasks, it sits behind the firmware's async
mutex, held for a push or a store read only, never while the link is busy, so a slow link never stalls a producer.

Generated code drives it: `stream_data_ser().<name>_push(..)` pushes, the server's `poll_streams` sends and the
dispatcher feeds it the host's sideband ([Reliable streams](counted_streams.md#reliable-streams)). Underneath, and for a
device without generated code, it is:

```rust
use ww_stream::{store::RamRing, sched::Priority, tx::{Reliable, Pushed}, Run};

let mut samples: Reliable<RamRing<{ 256 * 1024 }>> = Reliable::new(RamRing::new()).priority(Priority::Low);

// producer: units and time given here, nothing in the item type
if samples.push(&block_bytes, sets, Some(t)).await? == Pushed::Lost { /* counted, reported to the host */ }
samples.lost(dropped_sets); // the producer's own ring overflowed: goes into the next event's `lost`

// event loop: what may go out now, the host's Ack / Resend, a Query when the host went silent
if let Some(ev) = samples.next_event(Run::Live, &mut buf).await? { /* send ev.head and ev.item */ }
samples.on_host_sideband(&sideband).await?;
if let Some(query) = samples.poll_query(now_ms) { /* send it */ }
```

`ww_stream::tx::Counted` is the same for counted streams, without a store: `head(units, t)` returns the head of the
next event (`None` while no host has the stream open: the units count as lost), `lost(units)` counts drops,
`unsent(&head)` turns an event that couldn't be sent into lost units.

With several streams, `ww_stream::sched::Scheduler` decides which one sends next: by priority class (`High`, `Normal`,
`Low`, set by the device, not the API), the live run before the backfill run within a class, round-robin between
streams of one class, up to a byte budget per turn so that requests are read in between. A device puts its log stream
above a bulk ADC stream, a logger the other way round. Plain streams write directly and are not scheduled: data that
needs a priority is counted or reliable.

On the host, `ww_stream::rx::Receiver` takes each event's head (`on_item`) and returns a `Verdict`: accept (with the
gaps before it, each with its run, position, size and cause: `Device`, `Link`, `Store` or `Host`), duplicate, already
had, late or out of order; `poll(now_ms)` returns the next `Ack` or `Resend` to send. It runs in the client's
event loop behind every `PositionedStream`, see [Counted and reliable streams on the
host](counted_streams.md#host-side-positionedstreamt).

## The `ww_stream` crate

`no_std`, sans-IO: generated code (or a test) feeds the state machines what arrived and sends what they return, and
passes the time in milliseconds where timing matters. The wire types live in `ww_client_server::stream`.

| Module | What | Used by |
|---|---|---|
| `tx` | `Counted`, `Reliable`: sender state machines (runs, credit, go-back-N, sessions) | device |
| `rx` | `Receiver`: in order per run, gaps by cause, duplicates, `Ack` timing | host |
| `store` | `StreamStore`, `RamRing` | device |
| `sched` | `Scheduler`: which stream sends next, by priority | device |
| `counters` | `StreamCnt` (`cnt` instance counters with the `cnt` feature), `TxStats` | device |

The protocol is tested with a seeded simulation of a lossy, duplicating and reordering link with reconnects and
resumes (`ww_stream/src/tests/sim.rs`, more seeds with `SIM_SEEDS=20000`).
