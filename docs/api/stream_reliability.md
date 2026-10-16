---
level: advanced
---

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

This page explains what counted and reliable streams guarantee. How to declare them is in [Streams](streams.md),
the device's handlers and producers and the host's `PositionedStream` in
[Counted and reliable streams](counted_streams.md), the bytes on the wire and the acknowledgement protocol in
[Stream protocol](stream_protocol.md).

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

## Acknowledgements

A reliable stream's host acknowledges what it has, cumulatively per run, and grants the device a window of bytes in
flight (4 MiB by default, what the host queues for the stream). A host that doesn't read stops granting: the device
stops sending and keeps the data in its store, so the host's memory stays bounded. A gap (UDP) is asked for again
from its first unit, and the device goes back and resends from there (go-back-N). Data counts as received when user
code takes it, or with `AckMode::Explicit` only when user code confirms it, for example after writing it to disk. On
USB the link doesn't lose, so an acknowledgement is a few bytes every half window. The messages and timeouts:
[Acknowledgements and resends](stream_protocol.md#acknowledgements-and-resends).

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

With several reliable streams, the device sends by priority class (`High`, `Normal`, `Low`, set by the firmware, not
the API), live data before backlog within a class, see
[Sending](stream_protocol.md#sending-producers-the-event-loop-and-priorities).

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

The host's receiver (`ww_stream::rx::Receiver`) keeps `RxStats` with the same causes: received, lost on the device, on the link and in the store,
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
