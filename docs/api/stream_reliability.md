# Stream delivery: counted and reliable streams

A plain `stream!` sends each item once and forgets it: nothing tells the host that an item is missing, and nothing is
kept for a host that is slow or gone. That is right for a latest-value stream (a motor controller's state at 1 kHz,
where an old value is worthless), and wrong for measured data, logs and monitors, where the host must know exactly
what it didn't get, or must get everything.

WireWeaver has three delivery kinds for device → host streams:

| | plain | counted | reliable |
|---|---|---|---|
| Position in each event | – | yes | yes |
| Data the device dropped reported exactly | – | yes | yes |
| Link losses, duplicates, reordering detected | – | yes | yes, and repaired |
| Device timestamp per event | – | optional | optional |
| Acknowledged, resent on loss | – | – | yes |
| Kept while the host is slow or gone, sent when it is back | – | – | while the device's store has room |
| Latest values first after a reconnect, the backlog beside them | – | – | yes |
| Extra bytes per event | 0 | 1-8, plus `units`, `lost`, `t` when present | the same |
| Device state | none | a few counters | counters and a store |

Item types carry none of this: positions, loss counts and the time of the first unit are in a small head the framework
puts in front of every event, so a `SampleBlock` stays pure data (an item may add a time of its own, such as when a
UART chunk's last byte arrived).

This page explains what counted and reliable streams guarantee and how the protocol works. How to declare them, write
the device's handlers and producers, and open and resume them on the host is in [Streams](streams.md).

## Where stream data gets lost

From a producer on the device to user code on the host:

```
 producer ──► device buffer ──► event loop ──► framer ──► medium ──► host transport ──► event loop ──► stream queue ──► user code
 (DMA, ISR)   (user's ring)     (ww_device)               (USB..)    (nusb, socket)     (client)      (per stream)
```

- **The producer's buffer overflows**: a producer can't wait, so it drops data. That happens on every medium, when the
  stream's rate is above what the link moves, while the device's event loop is busy (a flash erase, a long handler),
  and while no host is connected.
- **The medium loses it**, depending on the transport (below).
- **The link goes down**: whatever was in buffers on the way is gone, and whatever the producer makes until the host is
  back.
- **The host's queue is full**, when user code doesn't read.

| Medium | The medium itself | Where losses happen |
|---|---|---|
| USB bulk / interrupt | CRC and retries in hardware, flow control by NAK: no loss | the producer's buffer, unplug, device reset, USB suspend, host process exit |
| RTT | RAM rings, no corruption; a chunk that doesn't fit is retried, then the link goes down | the producer's buffer (a probe polls at its own pace), detach |
| UDP | datagrams lost, duplicated, reordered; a message split across datagrams is dropped whole when a piece is lost | any datagram, the producer's buffer, link down |
| WebSocket | TCP: no loss while connected, backpressure blocks the device | the producer's buffer, what is in flight when the connection drops |
| In-process | none | the producer's buffer |

Counted streams make every one of these losses visible, with its size and place. Reliable streams also repair link
losses and keep data on the device while it has room.

Choose by what a missing piece costs:

- **plain**: the latest value is all that matters, and counting costs more than it tells;
- **counted**: measured data, telemetry over UDP: gaps are acceptable, but must be shown where they are;
- **reliable**: logs, monitors whose protocol decoders can't recover from a gap, data loggers that collect while no
  host is connected.

The kind is part of the API, visible in introspection (`ApiItemKind::StreamFn { delivery, .. }`), so it is
chosen in the API crate. How much a reliable stream keeps, and where, is the device's choice: one device keeps 256 KB
in RAM, another days of data on an SD card, behind the same API.

## Positions and units

A counted or reliable stream counts **units** from the start of its session, as a `u64`. What a unit is, the stream's
documentation says, and the device says how many units each item has when it sends it:

| Stream | Unit | An item of… | …has units |
|---|---|---|---|
| UART receive | byte | 37 bytes | 37 |
| I2C monitor | transaction | 12 transactions | 12 |
| ADC samples | sample set (one sample of every channel) | 1 536 bytes, 4 channels × 12 bit | 256 |
| a status stream | item | one value | 1 |

Each event carries the position of its first unit (`pos`) and its number of units (`units`). The next event starts at
`pos + units + lost`:

- **`lost`**: units the device dropped right before this event (producer overflow, store full, no host). It explains
  a jump of `pos` exactly and says the gap is the device's.
- A jump of `pos` that `lost` doesn't explain is a **link loss** (UDP): the host knows exactly how many units are
  missing, though not their content. A reliable stream gets them resent.
- An event that starts before what the host expects is a **duplicate** or a **late** one: the host drops it and
  counts it.

The host never counts units of an item: it only compares positions, so units are whatever the device says (an ADC
block's bytes don't tell its sample sets without the sample format). Counting units rather than items keeps every
position exact: a protocol decoder can say "the frame starts at byte 4711 of this capture", and a gap of 60 000 bytes
says how much is missing even when the device dropped raw bytes before they ever became items.

Positions are 64-bit because a store can be big: a 32 GB SD card holds 2³⁵ bytes of a byte stream within one session.
On the wire a position costs only the bytes it needs (below), and it is never truncated, so the host never has to guess
its high bits.

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

## Sessions, cursors and resuming

A **session** is one run of a stream with one meaning of its positions, numbered by the device (`session: u32`, for
example a boot counter combined with an open counter). A **cursor** names a point in it:

```rust
pub struct StreamCursor { pub session: UVlq32, pub pos: UVlq64 }
```

- A session starts when the host opens the stream, or when the device starts it by itself (a logger that samples from
  power-on). Positions start at 0.
- It ends when the stream is opened again (the units may mean something else now), or when the device's store is
  erased. It does not end when the link goes down, nor, with a store that survives reboots, on a reboot.
- Opening can carry a cursor: a **resume**. The device answers with `StreamOpened { session, oldest, live_from,
  resume_from }`:
    - same session, cursor still in the store: the backlog starts at the cursor, nothing is lost;
    - same session, cursor older than the store keeps: the backlog starts at the oldest event kept, the units before
      it are reported lost (cause `Store`);
    - another session: the old one has ended, the host starts with the new one.
- Counted streams resume too. They keep nothing, but the device keeps counting while the host is away, so after a
  reconnect the first event's `lost` says exactly what the host missed.

A host that saves data also saves the cursor with it, and resumes from it after a restart: with explicit
acknowledgements (below), nothing is lost even when the host crashes between receiving data and saving it.

## Latest values while backfilling

A resumed reliable stream has two **runs** of positions, each delivered in order:

- the **live run** starts at the newest event the device has (`live_from`), so a slow stream shows its latest value at
  once instead of after its next event, and goes on with everything new;
- the **backfill run** goes from the resume point up to `live_from`.

The device sends live events first, and backfill events with whatever capacity the link has left. A data logger with
six hours on its SD card shows its current readings on a dashboard as soon as the laptop connects, while the backlog
arrives beside them; when the live data alone fills the link, the backlog waits in the store and nothing is lost. When
the backfill run reaches `live_from` it ends, and the runs are one again.

Every event is sent once, and acknowledgements stay cumulative: one position per run. On the host each event says
which run it belongs to, so a dashboard reads the live run, a recorder takes both and writes by position. While a
backfill runs, acknowledged live events stay in the store until the backfill passes them.

## Acknowledgements and resends

A reliable stream's receiver (the host) tells the sender (the device) what it has. Positions are in units, grants in
bytes, since what runs out on the receiving side is memory:

| Sideband | Direction | Meaning |
|---|---|---|
| `Ack { have, backfill, grant }` | receiver → sender | received every unit before `have` (live run) and before `backfill` (backfill run, while there is one); the sender may have up to `grant` bytes of events in flight beyond them |
| `ResendFrom { from, grant }` | receiver → sender | units from `from` on are missing, in whichever run `from` falls: send again from there |
| `Query { sent, backfill }` | sender → receiver | "I have sent up to here, where are you?", when out of credit or nothing was heard for a while; answered with `Ack` or `ResendFrom` |

All three carry the whole state, so losing one is harmless: the next one replaces it.

- **The device** keeps every event in its store until an `Ack` passes it, sends while the bytes in flight before the
  next event are below the grant (so a grant smaller than an item can't stall the stream), and goes back on
  `ResendFrom` (go-back-N). With no host, nothing is acknowledged: the store fills, then data is dropped and counted.
- **The host** accepts events of each run in order only, drops duplicates, asks for a resend once per gap, and
  acknowledges every half window or 100 ms. The window is what the host queues for the stream (4 MiB by default): a
  host that doesn't read stops granting, the device stops sending and keeps the data in its store, so the host's
  memory stays bounded.
- **When data counts as received** (`AckMode`): by default when user code takes an event; with `Explicit`, only when
  user code confirms it, for example after writing it to disk.
- Defaults: a resend is asked again after 500 ms, the device queries after 200 ms of silence, the first grant is 4 KiB.

On USB the link doesn't lose, so `ResendFrom` never happens and an `Ack` is a few bytes every half window: a reliable
stream costs almost nothing more than a counted one there, and adds backfill and bounded host memory.

## Stores

What a reliable stream keeps lives in a store, a `ww_stream::store::StreamStore` the device picks per stream: `RamRing`
in RAM, `PersistentRamRing` in RAM that survives a soft reset, `LogStore` on an SD card or NOR flash that survives
reboots and power cuts. When a store is full it drops the newest events (`FullPolicy::DropNewest`, the default) or the
oldest (`DropOldest`); either way the units are counted and reported to the host as lost. See
[Stream stores](stream_stores.md).

Events are stored as they are sent, head included, so a resent event is bit-identical to the first send: its
timestamp is the device's time when the data was made, never when it was sent. A reboot restarts the device's clock;
the first event after one carries the new clock epoch in `HeadExt::clock_epoch`, so the host can tell the two
timelines apart.

## Sending: producers, the event loop and priorities

A `ww_stream::tx::Reliable` is one state: the producer pushes events into it, whether a host is there or not, and the
event loop reads the next event out of it and sends it. Shared between tasks, it sits behind the firmware's async
mutex, held for a push or a store read only, never while the link is busy, so a slow link never stalls a producer.

Generated code drives it: `stream_data_ser().<name>_push(..)` pushes, the server's `poll_streams` sends and the
dispatcher feeds it the host's sideband ([Reliable streams](streams.md#reliable-streams)). Underneath, and for a
device without generated code, it is:

```rust
use ww_stream::{store::RamRing, sched::Priority, tx::{Reliable, Pushed}, Run};

let mut samples: Reliable<RamRing<{ 256 * 1024 }>> = Reliable::new(RamRing::new()).priority(Priority::Low);

// producer: units and time given here, nothing in the item type
if samples.push(&block_bytes, sets, Some(t)).await? == Pushed::Lost { /* counted, reported to the host */ }
samples.lost(dropped_sets); // the producer's own ring overflowed: goes into the next event's `lost`

// event loop: what may go out now, the host's Ack / ResendFrom, a Query when the host went silent
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
had, late or out of order; `poll(now_ms)` returns the next `Ack` or `ResendFrom` to send. It runs in the client's
event loop behind every `PositionedStream`, see [Counted and reliable streams on the
host](streams.md#counted-and-reliable-streams-positionedstreamt).

## Counting

Every sending stream keeps `TxStats` (`stats()`): units sent and resent, lost by cause (producer, store full, no
host), bytes acknowledged, resend requests, overruns. With the `cnt` feature it also counts into typed
[`cnt`](https://crates.io/crates/cnt) instance counters, one set per stream, named by the firmware, so `cnt tui` shows
per stream where data went:

```rust
use ww_stream::counters::StreamCnt;

static ADC_SAMPLES_CNT: cnt::Counters<StreamCnt> = cnt::counters!(StreamCnt, adc_samples);
let samples = Reliable::new(RamRing::<{ 256 * 1024 }>::new()).counters(&ADC_SAMPLES_CNT);
```

The host's `Receiver` keeps `RxStats` with the same causes: received, lost on the device, on the link and in the store,
duplicates, late, out of order.

## Timestamps

`t` in the head is the device time of the event's first unit, a `ww_si::Second` in the storage the device's timer gives
cheaply: `Second { Micro, U32 }` (5 bytes, wraps every 71.6 minutes, the host unwraps it) on most MCUs. A device uses
one clock for every stream, so items of different streams line up. Three ways to timestamp, chosen per stream:

| Granularity | How | Cost | For |
|---|---|---|---|
| Per anchor | `t` in every N-th event (e.g. every 50 ms); the host interpolates by position and the configured rate | ~5 B per anchor | fixed-rate streams: ADC, logic analyzer |
| Per event | `t` in every event | 5 B per event | UART chunks (a chunk ends when the line goes idle, so `t` is its first byte), sensor readings |
| Per item | `t` per event, and each item's offset from it inside the item, in the head's unit, as a `DeltaOfDelta` | about 1.2 B per item for jittered ~100 µs gaps, 1.9 B for random 20-2020 µs gaps, 0.23 B for a regular rate | monitors: I2C transactions, GPIO edges |

Two examples:

- An ADC at 500 kS/s, 12-bit packed, one channel (750 KB/s): in blocks of 1 024 samples (1.5 KiB), event header and head
  take 0.7 %, a `t` in every event 0.3 % more. Per-sample timestamps would take 15 % and tell nothing, the rate is
  fixed: use blocks of about 1 KiB and an anchor every 50 ms.
- An I2C monitor at 1 MHz with about 20 000 transactions per second, in chunks of 32: 625 events per second of about
  15 bytes of header, head and `t` (9.4 KB/s), plus per-transaction time at ~1.5 B each (30 KB/s), against ~10 bytes of
  data per transaction (200 KB/s).

## With each transport

| Medium | Counted | Reliable |
|---|---|---|
| USB, RTT, WebSocket, in-process | device drops and link-down gaps exact | nothing lost while the store has room; backfill after a reconnect, live first; bounded host memory |
| UDP | link losses, duplicates and reordering detected too | go-back-N repairs them; acknowledgements carry the whole state, so losing one is harmless |
| CAN, UART (planned transports) | as UDP | as UDP; keep events small, a message split across frames is lost whole |

Resending is per stream, not in the link layer: a link that resends would hold latest-value streams back behind a lost
datagram, couldn't keep data across a reconnect or a reboot, and wouldn't know about data the device dropped.

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
