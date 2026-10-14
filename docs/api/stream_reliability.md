# Stream reliability (design)

!!! note "Design, approved by Roman on 14 Oct 2026, with his points worked in"
    Items: API-30 counted-reliable-streams, API-31 stream-functions (typed open, includes API-15), API-34
    sink-sessions, SRV-15 stream-stores, CLT-23 host-stream-positions, TEST-19 lossy-test-medium. Roman's review and
    the answers are in [Review](#review-14-oct-2026); the pieces are built on their own branches as in
    [Implementation plan](#implementation-plan). Piece 1, the protocol core (API-30: wire types and the `ww_stream`
    crate), is built; what it decided is in [As built in API-30](#as-built-in-api-30). Once all pieces are, this
    page becomes the reference for counted and reliable streams.

Roman, reviewing the IO trait family (D4 `seq` units, D5 `lost` counts, 14 Oct 2026): *"we miss reliable transfer
in wire_weaver, let's not bodge it in each trait from the side. On USB there shouldn't really be losses at all."*

This page puts positions, lost-data accounting, reliable delivery and backfill into the framework, for every
device and every stream, so that no item type has to carry `seq`, `lost` or `t` fields of its own.

## Summary

- **Every stream gets a delivery kind**, chosen in the API: plain (latest value wins, nothing is counted),
  **counted** (a position in each event, exactly how much was lost and where) or **reliable** (acknowledged and
  resent, nothing lost while the device has room to keep it).
- **Positions count units** the device names when it sends (bytes for a UART, transactions for a monitor, sample sets
  for an ADC), 64-bit: D4 and D5, done once in the framework. The host never computes units.
- **The head lives in the event, not in the item**: flags, `pos`, `lost` and `t` (device time) in a per-event head of
  1 to 6 bytes for any position up to 2 T units (plus 1-2 for events of more than one unit), plus 5 for a timestamp. Item types stay pure data.
- **Reliable streams and credit sinks are one protocol** in both directions: positions in units, grants in bytes (the
  receiver's memory), cumulative acknowledgements, go-back-N; sessions and cursors for both.
- **Backfill is built in from the start**: what the device keeps lives in a **store** (RAM ring, or an SD card / flash
  log) that keeps filling while the host is gone; a host that comes back **resumes** from its last position, sees the
  **latest values at once** and gets the backlog alongside, with the original timestamps.
- **Streams are opened like methods are called**: `#[stream(SampleBlock<'i>)] fn samples(config: StreamConfig<'i>) ->
  Result<StreamActual, AdcError>`, the old "functions returning streams" idea (API-15). A read on a stream returns
  its state.
- **Timestamps**: per event (a chunk) by default, per anchor for fixed-rate streams, per item as `DeltaOfDelta` inside
  monitor items.
- **One crate, `ww_stream`**, holds the state machines (device and host side), the stores and the send scheduler
  with priorities between streams; it counts what happens with typed `cnt` counters.

## Where data is lost today

Along the way from a producer on the device to user code on the host:

```
 producer ──► device buffer ──► event loop ──► framer ──► medium ──► host transport ──► event loop ──► stream queue ──► user code
 (DMA, ISR)   (user's ring)     (ww_device)               (USB..)    (nusb, socket)     (client)      (per stream)
     ①             ②                ③                         ④                                          ⑤
```

1. **Producer overflow** (② full): the producer can't wait, so data is dropped. Happens on **every medium**: when the
   stream's rate is above what the link moves (rePort's budget is ~800 KB/s on USB full speed for all streams
   together), while the device's event loop is busy (a flash erase, a long handler), and while no host is connected.
2. **Event loop blocked** (③): `<name>_send().await` waits for the framer, which waits for the medium. On USB a host
   that doesn't read makes the device's event loop wait, replies to requests included. Nothing is lost here, the
   loss moves to ①.
3. **Medium** (④), per transport below.
4. **Link down**: everything in device buffers, the framer, the medium and host queues at that moment is lost, and
   everything the producer makes until the host is back.
5. **Host queue** (⑤): today an unbounded channel per stream (`rx_dispatcher.rs`). A slow consumer (a Python notebook
   busy for a second) loses nothing, the host's memory grows instead, without limit. So on USB, contrary to the
   assumption in rePort's `api_design.md` §10.3, a slow notebook does not make the device drop: a slow USB bus or a
   too high total rate does.

| Medium | The medium itself | Where losses happen |
|---|---|---|
| USB bulk / interrupt | CRC and retries in hardware, flow control by NAK: **no loss** | ① when the rate is above the bus's share, ④ on unplug, device reset, USB suspend (SRV-9), host process exit |
| RTT | RAM rings, no corruption; a chunk that doesn't fit is retried for `write_timeout` (500 ms), then the link goes down | ① (a probe polls at its own pace, ~0.1-1 MB/s), ④ on detach |
| UDP | datagrams lost, duplicated, reordered; a message split across datagrams is dropped whole when a piece is lost | any datagram, ① , ④ (also a takeover by another host) |
| WebSocket | TCP: no loss while connected, backpressure blocks the device | ①, ④: what was in flight when the connection drops |
| CAN (TR-6, planned) | frames retried on bus errors; frames dropped when a receiver FIFO overflows (USB-CAN adapters, SocketCAN queues), bus-off | any frame (a split message is dropped whole), ①, ④ |
| UART (TR-7, planned) | bit errors, overruns: a message with a bad checksum is dropped | any message, ①, ④ |
| In-process | none | ① only |

So Roman is right that USB doesn't lose on the wire; but USB still loses at ① and ④, and the host has no way to
notice either: a stream item carries nothing that tells it something is missing.

### What WireWeaver guarantees today

| Part | Guarantee |
|---|---|
| Requests | `seq` and a timeout per request: a lost request or reply times out. A retry may execute twice (CLT-17 reliable-requests). |
| `ww_framer` | Detects a lost piece of a split message (remaining length, CRC on UDP), drops that message, continues with the next one. No retransmission. |
| `ww_link` | Link setup survives losses (retries), a peer timeout ends a silent session. |
| Plain sinks, host → device | Nothing: not acknowledged, not counted. |
| Credit sinks, host → device (API-28) | Reliable: byte positions, cumulative credit, go-back-N resend on loss; the host never outruns the device's buffer. |
| Streams, device → host | **Nothing**: no position, no loss signal, no acknowledgement, nothing kept for a host that comes back. |
| Sideband | In order with data on a link; host → device commands are answered. |

## Scenarios

The design is checked against these; [Scenarios walked through](#scenarios-walked-through) shows each one with the
mechanism below.

- **S1 UART monitor on USB, total rate above the budget.** rePort monitors two UART lines at 2 Mbaud while an ADC
  streams; for 300 ms the sum is above what USB full speed moves. The device drops 60 000 bytes of line A. The host
  must show "60 000 bytes lost at 12.345 s" at exactly the right place, and the protocol decoder must resynchronise
  after the gap instead of decoding garbage.
- **S2 UDP telemetry.** An ESP32 sensor node streams 100 Hz readings over Wi-Fi; some datagrams are lost, a few come
  twice or out of order. Live data: resending a reading from 2 s ago is pointless, but the plot must show gaps where
  they are, and duplicates must not show twice.
- **S3 Data logger, host gone.** A field logger samples 4 ADC channels at 1 kS/s and writes to its SD card. The laptop
  is connected once a day over USB (or a gateway over UDP); it must get every sample since the last time, with the
  original timestamps, while new samples keep coming. Nothing is lost unless the card is full, and then the host is
  told exactly what.
- **S4 Host stuck.** A Python script stops reading for 10 minutes (breakpoint). A plain or counted stream must not eat
  the host's memory; a reliable stream must stop at the device's store (RAM or SD) and continue without loss when the
  script reads again, or report exactly what the store couldn't hold.
- **S5 Device reboot.** The logger reboots (watchdog, power cut) while the host is away. Data on the SD card from before
  the reboot must still arrive, its timestamps marked as belonging to the previous clock epoch.
- **S6 Host process restart.** The host-side recorder crashes and is restarted; it resumes from the position it saved
  with the data, not from "now", and not from the beginning.
- **S7 ADC at 500 kS/s.** Fixed rate, 750 KB/s of packed 12-bit samples on USB full speed: the head and timestamps must
  cost around 1 %, not 10 %.
- **S8 I2C monitor at 1 MHz.** Up to ~20 000 short transactions per second, each with its own time; per-item cost of
  time must stay near one byte.
- **S9 A latest-value stream.** A motor controller streams its state at 1 kHz, the host shows it. Losing some is fine,
  resending is wrong (stale), counting is unnecessary: it must cost nothing extra.

- **S10 Latest values during backfill.** The S3 logger has 6 hours on its SD card; the laptop connects and a dashboard
  must show the current readings at once, not after the 4 minutes the backlog takes to come over, while a recorder
  gets all 6 hours and everything after.

## Design

### Three delivery kinds

| | plain | counted | reliable |
|---|---|---|---|
| Position in each event | – | yes | yes |
| Device-side drops reported exactly (`lost`) | – | yes | yes |
| Link losses, duplicates, reorder detected | – | yes | yes, and repaired |
| Device timestamp in the head | – | optional | optional |
| Acknowledged, resent | – | – | yes (go-back-N) |
| Kept while the host is slow or gone, sent when it is back (backfill) | – | – | while the device's store has room |
| Latest values first after a reconnect, backlog alongside | – | – | yes |
| Host → device messages | – | none (open / close only) | `Ack` every ~half window |
| Extra bytes per event | 0 | 1-8 (+5 with `t`) | 1-8 (+5 with `t`) |
| Device state | none | a few counters | counters and a store |
| Scenario | S9 | S1, S2, S7, S8 | S3-S6, S10 |

The kind is part of the API (a different event, like credit sinks are a different resource kind), so it is chosen in
the API crate and visible in introspection, and changing it later is a break. Plain stays the default, so existing
APIs are unchanged and S9 costs nothing. Which store a reliable stream uses and how big it is, is the device's choice,
not the API's: a rePort keeps 256 KB in RAM, a logger keeps 32 GB on SD, the same `ww_adc` API.

Counted is the advice for any stream of measured data: it costs a few bytes per event and makes every loss visible.
Reliable is for data that must not be lost: logs, monitors whose decoders can't recover from gaps, loggers.

### Declaring streams: functions that return streams (API-31)

`ww_adc` round 1 had `start_stream(config)` / `stop_stream()` methods next to the stream; round 2 starts and stops by
sideband `Open` / `Close` and configures with a method, because `Open` can't carry a configuration. Sideband is
lacking in four ways:

1. `Open` carries no data, so a configuration needs a separate method, and between the two calls the device runs with
   a configuration the host didn't choose, or the host has to know that the order matters.
2. A sideband handler can't answer with a typed result or error (`Option<StreamSideband>` only): the actual rate and
   "channel 9 doesn't exist" need a method.
3. A session (positions from 0, the units' meaning) has to start exactly when the configuration changes. With a method
   beside the stream, the framework doesn't know when that is; the method would reset the framework's counters from
   the side, the bodge Roman doesn't want.
4. Generic tools (a recorder, `ww stream`) can open any stream, but don't know which method to call first.

Roman's old idea (API-15) answers all four: **a stream is opened by calling a function, and the call returns the
stream**. In the trait it is a method with an attribute naming the item type:

```rust
#[ww_trait]
pub trait AdcStream {
    /// Raw samples, whole sample sets interleaved; unit: sample set
    #[stream(SampleBlock<'i>, reliable)]
    fn samples(config: StreamConfig<'i>) -> Result<StreamActual, AdcError>;

    fn stream_capabilities() -> StreamCapabilities<'i>;
}
```

- The method part is an ordinary method: arguments, a return type, `#[errors(..)]` lists and the flat `CallError`
  of the errors design (`docs/api/errors.md`, API-32, CLT-24), optional items (API-16), all apply unchanged.
- `stream!(x: T)` stays, as the short form of `#[stream(T)] fn x();` (no arguments, unit result): one mechanism for all
  streams. `#[stream(T, counted)]` / `#[stream(T, reliable)]` choose the kind; `#[counted] stream!(x: T)` is the short
  form there too.
- Device: the handler is the method's (`fn samples(&mut self, cx, config) -> Result<StreamActual, AdcError>`); an `Ok`
  starts a new session, an `Err` leaves the stream as it was. Closing calls `close_samples(cx)`. Scaffolded.
- Host: `let (actual, mut samples) = adc.samples(config).call().await?;` (a prepared call like any method, one `?`, CLT-24), `adc.samples_resume(cursor)` for
  a resume (the configuration of the session is kept on the device), dropping or `close()`-ing the stream closes it.
- Re-configuring a running stream is calling the function again: a new session. A setting that doesn't change what
  the units mean (a trigger level) can stay a method.
- **Several instances** of one stream (two subscribers of an I2C monitor with different address filters) are what
  "functions returning streams" make possible later. Not in v0.5, but the wire keeps room: an instance would be one more
  element of the event's `path`, which costs nothing now.
- **A read on a stream returns its state** (`RequestKind::Read` on a stream path, unused today): open or not, session,
  oldest / newest / next position, store use and capacity, persistent or not, and the arguments it was opened with,
  typed (`StreamStatus<StreamConfig>`). Methods like `ww_adc`'s `stream_state()` go away, and a generic tool shows the
  health of any stream.

Wire: `RequestKind::StreamOpen { resume: Option<StreamCursor>, args: TailBytes }` (appended), answered with a `Value`
holding `StreamOpened { session, oldest, live_from, resume_from: Option<pos>, result: TailBytes }`. Sideband `Close`
stays; sideband `Open` is no longer sent by new hosts.

### Positions and units (D4, D5)

Each counted or reliable stream counts **units** from the start of its session, as a `u64`. What a unit is, the
stream's doc says, and the device says how many units each item has when it sends it:

| Stream | Unit | An item of… | …has units |
|---|---|---|---|
| `ww_uart` `rx` | byte | 37 bytes | 37 |
| `ww_i2c` `I2cMonitor::records` | transaction | 12 transactions | 12 |
| `ww_adc` `AdcStream::samples` | sample set (one sample of every channel) | 1 536 bytes of 4 channels × 12 bit | 256 |
| `ww_logic_analyzer` `LogicStream` | sample | a block | its samples |
| a status stream | item | one value | 1 (the default) |

`pos` of an event is the position of its first unit, `units` (in the head too) how many it has. The next event's
`pos` is this one's `pos + units` plus its own `lost`, so:

- **`lost`** (units the device dropped right before this event: producer overflow, store full, no host) explains a
  jump of `pos` exactly, and says the gap is the device's.
- A jump of `pos` that `lost` does not explain is a **link loss** (UDP, CAN): the host knows exactly how many units are
  missing, though not their content. Reliable streams get them resent.
- `pos` lower than expected is a **duplicate** or a **late** event (UDP): the host drops it and counts it.

The host never needs to know how to count units of an item: it only compares positions. That is why units are given
at send time and never computed from the item (an ADC block's byte length doesn't tell the sets without the format),
and why they travel in the head: without them the host can't tell where an event ends, and a lost event between two
received ones would look like a bigger first one (found while building API-30).

Why units and not items: a protocol decoder needs "frame starts at byte 4711 of this capture", and a gap of "3 items"
says nothing when the device drops raw bytes before they ever became items (D5's scenario).

**Why 64 bits.** A 32 GB SD card holds 2³⁵ bytes, so a byte stream's positions pass 2³² within one card, and a
session lasts as long as the card's data does. 32-bit positions would wrap inside the store, and a resume cursor
would become ambiguous. Positions are `u64` everywhere: in the head, in cursors, in acknowledgements, in stores. On the
wire they cost only what they need (below): a position never wraps in practice (2⁵⁷ units in the head is 144 P), and
nothing is truncated, so the host never has to guess the high bits.

### Head on the wire

A counted or reliable stream sends a new event kind, `EventKind::StreamItem` (appended to `EventKind`, which has room
for 16 variants), instead of `StreamData`:

```rust
// ww_client_server
pub enum EventKind<'i> {
    // ... Value, Written, StreamData, StreamSideband as today
    StreamItem { path: RefVec<'i, UNib32>, head: StreamHead, data: TailBytes<'i> },
}

pub struct StreamHead {
    pub pos: u64,           // position of the event's first unit since the session started
    pub units: u64,         // units of the event's item (1 for one item, the bytes of a UART chunk, ...)
    pub lost: Option<u64>,  // units the device dropped right before this event; `pos` counts them
    pub t: Option<Second>,  // device time of the first unit (`ww_timebase` clock)
    pub ext: Option<HeadExt>, // rare markers, evolvable: clock epoch changed (reboot), ...
}
```

The derive would write each `Option`'s flag right before its value, and `pos` as a byte-aligned number after its own
flag: the first measurement put 3 flags into a byte of their own. Written by hand instead, the flags share the first
byte with the length of `pos` and its top bit:

```
 byte 0:  L T X U n n n p      L, T, X, U: lost, t, ext, units follow (units only when not 1)
                               nnn: bytes of pos after this one (0..7), p: the top bit of pos
 then:    n bytes of pos (1 + 8n bits in all, big endian)
          units: UVlq64             if U
          lost: UVlq64              if L
          t:    ww_si::Second       if T (starts on a byte, so its prefix and type tag share one byte)
          ext:  HeadExt             if X (Unsized: fields can be appended later)
```

| `pos` below | 2 | 512 | 128 Ki | 32 Mi | 8 Gi | 2 Ti | 512 Ti | 2⁵⁷ |
|---|---|---|---|---|---|---|---|---|
| Head bytes (one unit, no `lost`, `t`, `ext`) | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |

Plus `units` when not 1 and `lost`, each 1 byte below 128 (2 below 16 Ki, ...), `t` 5 bytes as `Second { Micro,
U32 }` (wraps every 71.6 min, D6; 9 bytes as `U64`). Events of one item (units = 1) pay nothing for `units`; a byte
stream pays 1-2 bytes. A 32 GB byte stream near its end, in 1 KiB chunks, has an 8-byte head, 13 with a timestamp.

All of it measured with real bytes (`ww_client_server::stream` tests): every size in the table at both ends of its
range; `units` = 127 is 1 byte, 128 and 16 383 are 2, 16 384 is 3; `lost` = 100 is 1 byte, 200 is 2; `t` as
`Second { Micro, U32 }` is 5 bytes and as `U64` 9; a whole `StreamItem` event with a one-element path, `pos` = 2³³,
`units` = 3, `lost` = 7 and `t` is 4 + 13 bytes before its data (`da 02 00 00 00 00 03 07 23 78 56 34 12` for the
head). A position of 2⁵⁷ or more is a serialization error. For comparison, measured before (derived `Option`s and a
32-bit `UVlq32` position, no `units`): 2 to 6 bytes for positions up to 4 Gi only.

Today's `StreamData` event costs 3 to 5 bytes of header and path before the data (measured), so a long-running stream
with a timestamp pays ~15 bytes per event: 3 % of a 512-byte chunk, 1 % of 1.5 KiB. Small items go into chunks
(a `MaxLen<RefVec<'i, Record>, N>` item, as the IO family does already), see [Timestamps](#timestamps-and-their-cost).

`Second` costs one byte more than raw ticks: not worth a new type (D6 holds). Considered and not taken: sending `pos`
only when the host may be out of step (the host would have to count units from items); truncating `pos` to its low
bits (ambiguous across a big store); putting `pos` into `Event::seq` (mixes request ids with positions).

### Sessions, cursors and resuming

A **session** is one run of a stream with one meaning of its positions, numbered by the device (`session: u32`, e.g.
a boot counter and an open counter, or random). A **cursor** names a point in it:

```rust
pub struct StreamCursor { pub session: UVlq32, pub pos: UVlq64 }
```

- A session starts when the host **opens the stream** (calls its function) with arguments, or when the device starts
  it by itself (a logger that samples from power-on with its stored configuration). Positions start at 0.
- It ends when the host opens it again (the units may mean something else now), or when the device's store is erased.
  **It does not end when the link goes down**, nor, with a persistent store, on a reboot.
- Opening carries an optional cursor (**resume**). The device answers with the session, the oldest position it still
  has, where the live part starts, and where the backlog starts:
    - same session, cursor still in the store: backlog from the cursor on, nothing lost (S3, S6);
    - same session, cursor older than what is kept: backlog from the oldest it has; the units in between are reported
      lost (`Gap { cause: Store }` on the host);
    - another session (the device restarted the stream): the host reports the old session as ended and starts with the
      new one.
- Counted streams resume too: there is no store, but the device keeps counting while the host is away (and counts
  what it drops as `lost`), so after a reconnect the first event's `lost` says exactly how much the host missed.

The host keeps the cursor of every open stream. On a reconnect with `disconnect_keep_streams` (EX-10) it re-opens each
one with its cursor by itself; user code sees a `Resumed` event, and gaps if any. A recorder that saves data
somewhere also saves the cursor next to it, and passes it to `<name>_resume()` after a restart (S6).

### Latest values while backfilling (S10)

A resumed reliable stream has **two runs** of positions, each delivered in order:

- the **live run** starts at the newest event the device has (`live_from`, so a slow stream shows its latest value at
  once instead of after its next event; when the device lost units right before that event, `live_from` is where
  they start, so the event reports them) and goes on with everything new;
- the **backfill run** goes from the resume cursor (or the oldest kept event) up to `live_from`.

The device sends live events first, and backfill events whenever the live run has nothing to send or no credit
(the [send scheduler](#send-scheduler-and-priorities) does it, with live before backfill in every priority class). The
link's spare capacity drains the backlog; when the live run alone fills the link, the backlog waits in the store,
nothing is lost, and live data is never delayed by old data. When the backfill run reaches `live_from`, it ends
(`BackfillDone` on the host), and the two runs are one again.

On the host, events carry which run they belong to; user code takes what it needs:

```rust
let (_, samples) = adc.samples_resume(cursor).call().await?;
let (mut live, mut backlog) = samples.split();    // two receivers, each in order
// dashboard: live.recv() -> latest values right away
// recorder:  both, written by position; or samples.recv_event() for everything interleaved, each tagged Live / Backfill
```

Alternatives considered: a rate-limited "peek" of the newest value beside an in-order stream (sends data twice, and
a dashboard gets a preview, not the data); sending newest-first (the backlog would arrive backwards, a recorder has to
sort, and acknowledgements stop being cumulative). Two in-order runs keep every event sent once and every
acknowledgement cumulative.

Acknowledgements carry one position per run (below); the store releases what is acknowledged from its start, so while
a backfill runs, acknowledged live events stay in the store until the backfill passes them. That costs store room only
for the time the backfill takes.

### One protocol for reliable streams and credit sinks (API-34)

Reliable streams are credit sinks (API-28) the other way round. Both directions use one set of messages, with
**positions in units** (identity, loss accounting, resume) and **grants in bytes** (what actually runs out is the
receiver's memory: a device's sink buffer, a host's queue):

| Sideband | Direction | Meaning |
|---|---|---|
| `Ack { have, backfill, grant }` | receiver → sender | received every unit before `have` (live run) and before `backfill` (backfill run, while there is one); the sender may have up to `grant` bytes of events in flight beyond that |
| `Resend { from, grant }` (`ResendFrom` until API-34 removes the credit sinks' `Resend`) | receiver → sender | units from `from` on are missing (in whichever run `from` falls): send again from there |
| `Query { sent, backfill }` | sender → receiver | "I have sent up to `sent` (and `backfill` in the backfill run), where are you?" (out of credit, nothing heard for a while); answered with `Ack` or `Resend`. Without `backfill` while the receiver has a backfill run: the sender ended it (the store dropped the rest of the backlog) |

All positions `UVlq64`, all messages cumulative, so a lost one is harmless (UDP). Positions are always the start of an
event, since events are accepted whole. These replace today's `Credit` / `CreditQuery` / `Resend` (byte positions, 32
bit); credit sinks are not on crates.io yet (`ww_client_server` 0.1.0 is), so nothing deployed breaks.

What credit sinks gain (Roman: "reliable sinks in units: yes"): positions in units (bytes for a byte sink, items for a
typed one), sessions and cursors, so a DFU plan resumes after a reconnect instead of starting over, and 64-bit
positions beyond 4 GiB. The device's buffer is still granted in bytes, so nothing changes in how a device sizes it
(`SinkCredit::sized_for`).

For a reliable stream:

- **Device**: keeps every event it sent in its store until an `Ack` passes it, sends while the bytes in flight stay
  within `grant`, goes back to `from` on `Resend`. Host gone: nothing is acknowledged, the store fills, then new data
  is dropped and counted in `lost` (or the oldest, by the store's policy).
- **Host**: accepts events of each run in order only, drops duplicates, asks for a resend on a gap (once per gap),
  acknowledges every half window or 100 ms. The **window** is what the host will queue for this stream (default 4 MiB,
  configurable): a host that doesn't read stops granting, the device stops sending and keeps filling its store. Host
  memory stays bounded, data waits on the device, where a big SD card may be (S4).
- **Acknowledge when**: by default when user code receives an event; with `AckMode::Explicit`, only when user code
  calls `ack(pos)`, e.g. after it wrote the data to disk, so a crash between receiving and saving loses nothing (S6).
- On USB the link never loses, so `Resend` never happens, and `Ack` is a few bytes every half window: reliable costs
  almost nothing over counted on USB, and gives backfill and bounded host memory.

Why per stream and not a reliable link layer (ARQ in `ww_link`, making UDP as good as USB): link-level resending would
hold back latest-value streams behind a lost datagram (S2, S9), can't keep data across a reconnect or a reboot, and
doesn't know about device-side drops; the end-to-end positions are needed anyway. A reliable link may still be right
for requests on UDP; that is CLT-17 reliable-requests, kept separate.

### Stores and backfill (SRV-15)

What a reliable stream keeps lives in a **store**:

```rust
pub trait StreamStore {
    type Error;
    /// Append an event: its head and item bytes, as they go on the wire. `Full`: nothing stored, the caller counts
    /// its units as lost.
    async fn push(&mut self, head: &StreamHead, item: &[u8]) -> Result<(), PushError<Self::Error>>;
    /// Copy the event at `pos` (or the first one after it) into `buf`.
    async fn read<'b>(&mut self, pos: u64, buf: &'b mut [u8]) -> Result<Option<StoredEvent<'b>>, Self::Error>;
    /// Events before `pos` are acknowledged and may go.
    async fn release(&mut self, pos: u64) -> Result<(), Self::Error>;
    /// Session, oldest and newest kept positions, next position, bytes used and capacity, persistent or not.
    fn state(&self) -> StoreState;
    /// A new session: drop everything, positions from 0.
    async fn reset(&mut self, session: u32) -> Result<(), Self::Error>;
}
```

Shipped stores, in `ww_stream`:

- **`RamRing<N>`**: a byte ring in RAM, the default (rePort: 256 KB from the capture pool, ~300 ms of the full USB
  budget). Survives host reconnects, not reboots.
- **`LogStore`** on a block device (SD card through `embedded-sdmmc`'s `BlockDevice`, NOR flash through
  `embedded-storage`), behind a feature: an append-only log of segments with a header per segment (session, first
  position, clock epoch), a tail pointer written every so often, a CRC per record. Survives reboots and power cuts:
  after a power cut it finds the end of the log by scanning the last segment, so at most the record being written is
  lost, and it is counted. Big enough for days of data.
- Policy when full: **drop newest** (default, keeps what is stored contiguous, as D5 wants) or **drop oldest** (a ring
  of the most recent data). Both counted exactly.

Events are stored as they are sent, head included, so a resent event is bit-identical: timestamps are the device's
time when the data was made, never when it was sent (S3). A reboot restarts the device's clock, so the first record
after one carries a new **clock epoch** in `HeadExt`; the host maps each epoch to real time with `ww_timebase`'s
`time_map` (or shows "before reboot N"), S5.

Producers write into the store from their own task (a DMA half-done interrupt wakes a task that builds the item and
pushes it), the event loop reads from it and sends: the store is shared between the two (a producer half and a sender
half, or a `critical-section` mutex). API-30 decided: one `Reliable` state, which the event loop holds only to read
the next event out of the store into its own buffer, and sends that buffer with the state released; shared with a
producer task behind the firmware's async mutex. Pushing never waits for the link, the link only ever waits for
the store, so a slow link can't stall a producer, and replies are not stuck behind stream data (rePort
`api_design.md` §10.3).

### Send scheduler and priorities

Today every `<name>_send().await` writes straight into the framer, in whatever order the producers call it. With
stores in between, sending is decided in one place, the **send scheduler** in `ww_stream`, which the event loop
calls between requests (generated `poll_streams(&mut server, out)`):

- each counted or reliable stream has a **priority** set by the device (`Reliable::new(store).priority(High)`), not
  the API: a rePort puts its log stream above a bulk ADC stream, a logger the other way round;
- per turn the scheduler sends events by priority class, the live run before the backfill run within a class,
  round-robin between streams of one class, up to a byte budget per turn, so requests are read between turns;
- a stream out of credit or with nothing to send is skipped.

This is not a crutch: it is the one place where the order of outgoing data is decided, every stream goes through it,
and nothing else has to know. What it can't do: reorder what is already in the framer or the USB endpoint (bounded
by their few KB), or schedule plain streams, which still write directly; data that needs a priority is counted or
reliable. Live before backfill (S10) is the same mechanism.

### Counting with `cnt`

Each counted or reliable stream counts what happens to its data with typed `cnt` instance counters, not one counter
for everything, so `cnt tui` on a firmware shows per stream what went where:

```rust
// ww_stream
#[derive(cnt::Count)]
pub enum StreamCnt {
    #[count(u64, unit = "unit")] Sent,
    #[count(u64, unit = "unit")] Resent,
    #[count(u64, unit = "unit", warn)] LostProducer,   // producer's own ring overflowed (`lost()`)
    #[count(u64, unit = "unit", warn)] LostStoreFull,
    #[count(u64, unit = "unit", warn)] LostNoHost,     // counted stream, link down
    #[count(u64, unit = "B")] Acked,
    #[count(warn)] ResendRequests,
    #[count(warn)] Overruns,                           // more in flight than granted (a host bug)
}

// firmware: one instance per stream, named by the firmware
static ADC_SAMPLES_CNT: cnt::Counters<StreamCnt> = cnt::counters!(StreamCnt, adc_samples);
let samples = Reliable::new(RamRing::new()).counters(&ADC_SAMPLES_CNT);
```

`cnt`'s `disabled` feature compiles them out. The host side keeps its own `StreamStats` (received, lost by cause,
duplicates, late), the same names, in `stream.stats()`. Store fill could also go into SRV-11 link statistics.

### Device side (codegen)

Like credit sinks, the server struct owns the state, and generated code is handed it:

```rust
struct Server {
    rx: Counted,                                    // counted: counters only
    samples: Reliable<RamRing<{ 256 * 1024 }>>,     // reliable: counters, credit, runs, store
}

impl Server {
    // generated code calls this for open / close / Ack / Resend / Query and for the scheduler
    fn stream_samples(&mut self) -> &mut Reliable<RamRing<{ 256 * 1024 }>> { &mut self.samples }
    // the stream function's handler, see above
    fn samples(&mut self, cx: &mut Context<'_, impl EventOut>, config: StreamConfig<'_>)
        -> Result<StreamActual, AdcError> { .. }
    fn close_samples(&mut self, cx: &mut Context<'_, impl EventOut>) { .. }
}

// producer task: units and time given here, nothing in the item type
stream_data_ser().samples_push(&mut server.samples, &block, units, Some(t)).await; // full: counted as lost
server.rx.lost(dropped_bytes); // the producer's own ring overflowed: goes into the next event's `lost`
// event loop, between requests: what the scheduler picks, as credit allows
stream_data_ser().poll_streams(&mut server, &mut server.sink()).await;
```

Exact names and how state is shared with producer tasks are for API-30 / API-31; `ww api scaffold` emits the handlers.

### Host side (CLT-23)

```rust
let (actual, mut samples) = adc.samples(StreamConfig { .. }).call().await?;
while let Some(ev) = samples.recv_event().await {
    match ev {
        StreamEvent::Item { pos, t, value, run } => { .. }       // run: Live or Backfill
        StreamEvent::Gap { at, units, cause } => { .. }          // cause: Device, Link, Store, Host
        StreamEvent::Resumed { cursor } | StreamEvent::BackfillDone | StreamEvent::Disconnected => { .. }
    }
}
save(samples.cursor()); // with the data, for a restart
```

- `recv()` keeps returning plain values; gaps are counted in `samples.stats()` and logged.
- Per-stream queues become bounded for every stream (`ClientConfig`, default a few MiB): plain and counted streams
  drop the newest and report a `Gap { cause: Host }`, reliable ones stop granting. Fixes the unbounded growth of S4.
- `samples.status().await` is the read on the stream (state, store fill, typed open arguments).
- The dynamic client and Python get the same: positions, gaps, runs, cursors, `ack`.

### Timestamps and their cost

`t` in the head is the device time of the event's first unit, a `ww_si::Second` in the storage the device picks
(D6): `Second { Micro, U32 }` (5 bytes, wraps every 71.6 min, the host unwraps) on most MCUs. One clock per device
(`ww_timebase`), so items of different streams line up. Three granularities, the device picks per stream:

| Granularity | How | Cost | For |
|---|---|---|---|
| Per anchor | `t` in every N-th event (e.g. every 50 ms); the host interpolates by `pos` and the configured rate | ~5 B per anchor | fixed-rate streams: ADC, logic analyzer |
| Per event (chunk) | `t` in every event | 5 B per event | UART chunks (a chunk ends on line idle, so `t` is its first byte), sensor readings |
| Per item | `t` per event, plus each item's offset from it inside the item, in the head's unit, as `DeltaOfDelta` (SW-31) | measured: 1.2 B/item for jittered ~100 µs gaps, 1.9 B/item for random 20-2020 µs gaps, 0.23 B/item for a regular rate | monitors: I2C transactions, GPIO edges |

Per anchor relies on the sample clock and the timer coming from the same oscillator; anchors bound the drift if not.

- **ADC, 500 kS/s, 12-bit packed, one channel (750 KB/s), S7**: an hour in, positions are ~2³¹ sample sets, a 7-byte
  head with `units`. Blocks of 256 samples (384 B) are 1 953 events/s: header and head 11 B = 2.9 %, `t` in every
  event 1.3 % more. Blocks of 1 024 samples (1.5 KiB): 0.7 % and 0.3 %. Per-sample timestamps would be 15 % for no information: the rate
  is fixed. Recommendation: blocks of ~1 KiB and a `t` anchor every 50 ms: under 1 % in all.
- **I2C monitor at 1 MHz, ~20 000 transactions/s, S8**: chunks of 32 transactions every ~1.6 ms: 625 events/s × 15 B
  (header, head with `units`, `t`) = 9.4 KB/s, plus per-transaction time ~1.5 B × 20 000 = 30 KB/s, against ~10 B of data per
  transaction (200 KB/s). Timing costs ~20 %, needed for a monitor; well inside the budget.

### Scenarios walked through

- **S1**: counted (or reliable) `line_a`. The consumer task can't push 60 000 bytes, calls `lost(60000)`; the next
  event has `lost = 60000`, `pos` jumped by as much plus its own bytes. The host emits `Gap { at, 60000, Device }`, the
  decoder resyncs. Exact, on any medium.
- **S2**: counted. Lost datagrams show as `pos` jumps without `lost` (`Gap { Link }`), duplicates and late ones are
  dropped and counted. Nothing is resent, nothing is stale.
- **S3**: reliable with a `LogStore` on SD. Without a host the store fills; the laptop resumes with its saved cursor,
  gets the live run at once and the backlog with the link's spare capacity. Timestamps are from the records.
- **S4**: plain/counted: the host's bounded queue drops and reports `Gap { Host }`; memory stays bounded. Reliable: the
  host stops granting, the device's store absorbs 10 minutes (SD) or reports what didn't fit (RAM).
- **S5**: `LogStore` keeps session and positions across the reboot; the first record after it carries the new clock
  epoch. The host gets one session with an epoch change in it.
- **S6**: `AckMode::Explicit` and a cursor saved with the data; after the restart, `samples_resume(cursor)`.
- **S7, S8**: [Timestamps](#timestamps-and-their-cost).
- **S9**: plain `stream!`: unchanged, no cost.
- **S10**: [Latest values while backfilling](#latest-values-while-backfilling-s10): the dashboard reads the live run,
  the recorder both.

### Per transport, with this design

| Medium | Counted | Reliable |
|---|---|---|
| USB, RTT, WebSocket, in-process | device drops and link-down gaps exact | nothing lost while the store has room; backfill after reconnect, live first; host memory bounded |
| UDP | also link losses, duplicates, reordering detected | go-back-N repairs them; acknowledgements are cumulative, so their loss is harmless |
| CAN, UART (when they exist) | as UDP | as UDP; keep events small (a split message is lost whole) |

## The `ww_stream` crate

One new `no_std` crate (Roman: one crate, if it doesn't mix too much), framework layer, version with the workspace:

| Module | What | Used by |
|---|---|---|
| `head` | `StreamHead` codec (the hand-written layout above) | both sides |
| `tx` | `Counted`, `Reliable<S>`: sender state machines (runs, credit, go-back-N, sessions) | device (and host, for sinks) |
| `rx` | receiver state machine: in order per run, gaps by cause, duplicates, `Ack` timing | host (and device, for sinks) |
| `store` | `StreamStore`, `RamRing`; `LogStore` behind the `log-store` feature | device |
| `sched` | send scheduler with priorities | device |
| `counters` | `StreamCnt` (`cnt` instance counters with the `cnt` feature), `TxStats` | device |

It is one subject, delivery of positioned data, and each module is small; the block-device log is the only part with
outside dependencies, so it sits behind a feature. Wire types (`StreamHead` as a type, `StreamCursor`, `StreamOpened`,
the sideband messages, `EventKind::StreamItem`, `RequestKind::StreamOpen`) stay in `ww_client_server`, which is the
wire model. `ww_client_server::credit::SinkCredit` moves into `ww_stream::tx` (API-34), since sinks and streams share
the protocol.

### As built in API-30

What building the protocol core added to the design, or decided where the design left it open:

- **`units` in the head** (byte 0 `L T X U nnn p`): without them the host can't see where an event ends. Positions up
  to 2⁵⁷. See [Head on the wire](#head-on-the-wire).
- **Stores** take the head only (`push(head, item)`), it carries the units. `StoreState::dropped_units` says what a
  drop-oldest store forgot, so the sender counts it exactly.
- **Grants**: an event goes out while the bytes in flight before it are below the grant (so a grant smaller than an
  item can't deadlock). The sender keeps up to `K` in-flight records per run (`Reliable<S, K = 16>`); beyond that,
  neighbours merge: acknowledgements and resends then act on the merged range, never wrong. It never goes back
  before what is acknowledged.
- **The live run's start** includes the units lost right before the newest event (above).
- **Resuming in the middle of a backfill**: a cursor is one position, so the live events after it come again in the
  new backlog. The receiver remembers up to 8 such ranges (`Seen`) and drops them as `Verdict::Had`, reporting
  only the part of their gaps it didn't have; beyond 8, events are delivered twice, never lost.
- **When one side's backfill run ended without the other knowing** (a lost `Query`, a stale `ResendFrom`): the
  device answers a `ResendFrom` or `Ack` for what it no longer has with a `Query` right away, a host whose backfill
  stalls sends its `Ack` again. Found by the seeded simulation (`ww_stream/src/tests/sim.rs`, `SIM_SEEDS=20000`).
- **Timing** is the caller's: `poll(now_ms)` / `poll_query(now_ms)`; defaults: ack every half window or 100 ms,
  resend asked again after 500 ms, query after 200 ms of silence, window 4 MiB, initial grant 4 KiB.
- **Introspection**: `ApiItemKind::PositionedStream { ty, delivery }` (appended, like `FlowSink`); plain streams keep
  `Stream` and their bytes. API-31 adds the stream function's arguments and result.

## Compatibility and what breaks

- `ww_client_server`: appended `EventKind::StreamItem`, `RequestKind::StreamOpen`, new types; `StreamSideband`'s
  `Credit` / `CreditQuery` / `Resend` replaced by `Ack` / `Query` / `Resend` with 64-bit unit positions, and
  `CreditWrite` by a write with a 64-bit unit position (API-34). Existing messages of plain streams, methods and
  properties keep their bytes. Credit sinks are not on crates.io (`ww_client_server` 0.1.0 is), so no deployed device
  breaks; `ww_dfu` and its tests move along.
- `ww_self`: `ApiItemKind::Stream` gets the delivery kind and the stream function's arguments and result;
  introspection data changes, snapshots re-saved.
- Codegen: plain streams generate what they do today, plus the open / close handlers; new hosts open every stream with
  `StreamOpen`. An old host's sideband `Open` keeps working on plain streams.
- Turning a plain stream into a counted or reliable one, or giving it arguments, is an API break (bump the API crate),
  like plain sink → credit sink. To be added to `docs/evolution/rules.md`.
- IO family (STD-13, not published): `seq`, `lost`, `t` stay out of item types (already done there); `configure_stream`
  becomes the stream function's arguments, `stream_state()` the read on the stream. No deployed device is affected.

## Review (14 Oct 2026)

Roman: "the design looks great; build it", with nine points. Each with what changed:

1. **Latest values right away while backfilling.** Two in-order runs, live first: [Latest values while
   backfilling](#latest-values-while-backfilling-s10), scenario S10.
2. **32 bits cover 4 GB, a 32 GB SD card more.** Positions are `u64` everywhere; on the wire a length-coded position
   up to 2⁵⁷ that costs 1 to 6 bytes for positions up to 2 Ti, nothing truncated: [Why 64 bits](#positions-and-units-d4-d5),
   [Head on the wire](#head-on-the-wire).
3. **Smaller head with flag repositioning.** The flags share the first byte with the position's length; `Second`
   then starts on a byte with its prefix and tag in one byte: [Head on the wire](#head-on-the-wire).
4. **Typed open, close to functions returning streams.** That's it: `#[stream(T)] fn name(args) -> Result<A, E>`,
   `stream!` its short form, room for several instances later (API-15 folds into API-31):
   [Declaring streams](#declaring-streams-functions-that-return-streams-api-31).
5. **One `ww_stream` crate.** Yes, with the wire types staying in `ww_client_server`: [The `ww_stream`
   crate](#the-ww_stream-crate).
6. **A read on a stream returning its state.** Kept, part of API-31.
7. **`cnt`, typed counters.** `StreamCnt` instance counters per stream: [Counting with `cnt`](#counting-with-cnt).
8. **Priorities between streams, if not a crutch.** The send scheduler, which is where the order is decided anyway:
   [Send scheduler and priorities](#send-scheduler-and-priorities).
9. **Reliable sinks in units.** One protocol for both directions, sinks get sessions and resume (API-34):
   [One protocol](#one-protocol-for-reliable-streams-and-credit-sinks-api-34).

The round 1 decisions, as built: R1 three kinds, plain the default; R2 units given at send time, `u64` positions in a
hand-laid head; R3 `t` in the framework head, per-item time in the item as `DeltaOfDelta`; R4 streams as functions,
one open mechanism for all streams; R5 one `ww_stream` crate; R6 drop newest by default, drop oldest as an option;
R7 acknowledge on receive by default, explicit as an option.

## Implementation plan

Each piece on its own branch, in this order (claimed ids in brackets):

1. **Protocol core** (API-30, `top`): wire types in `ww_client_server`, the `ww_stream` crate (head codec, `Counted`,
   `Reliable`, receiver, runs, `RamRing`, scheduler, `StreamCnt`), `ww_self` delivery kind. Exhaustive unit tests:
   loss, duplicates, reorder, positions past 2³² and 2⁵⁰, resume inside / before / after the store, both full policies,
   live and backfill runs, scheduler order. Real bytes of the head written into this page. Its first commit pins the
   crate skeleton and the `StreamStore` trait, so piece 4 can start from it.
2. **Stream functions and codegen** (API-31, `opus`, after 1): `#[stream(T, kind)]`, `#[counted]` / `#[reliable]` on
   `stream!`, grammar and AST, server codegen (handlers, `_push`, `_send`, `poll_streams`, dispatch of `StreamOpen`,
   the read and the sideband), client codegen (`name(args)`, `name_resume(cursor)`, `status()`), scaffold,
   introspection, `tests/streams_reliable` (+ `_api`). FEATURES: API-15 to superseded.
3. **Host runtime** (CLT-23, `opus`, after 1, beside 2): `StreamEvent`, runs and `split()`, stats, bounded queues for
   every stream, ack modes, cursors, auto-resume on reconnect, dynamic client and Python.
4. **Persistent store** (SRV-15, `top`, after the first commit of 1): `LogStore`, power-cut safe, clock epochs, both
   policies; simulated block device with a power cut at every write.
5. **Sinks on the shared protocol** (API-34, `opus`, after 1): unit positions, `Ack` / `Query` / `Resend`, sessions and
   resume for credit sinks, `SinkCredit` into `ww_stream`, `ww_dfu` and its tests along.
6. **Lossy test medium and scenarios** (TEST-19, `sonnet`, after 2 and 3): an in-process medium that drops,
   duplicates, reorders, stalls and disconnects on a seeded schedule; S1-S6 and S10 end to end; docs: `streams.md`,
   each transport page's reliability section, evolution rules.
7. **IO family on top** (`sonnet`, after 2, with the STD-13 session): `ww_uart` rx and monitor, `ww_i2c` monitor,
   `ww_adc` samples as counted or reliable stream functions, units in their docs, per-item `DeltaOfDelta` time in
   monitor items.

## Ideas not asked for

- **`ww record`**: every stream of one or more devices into a file (or rerun), with cursors, so `ww record --resume`
  continues after a restart; gaps, runs and clock epochs on the timeline.
- **Several instances of a stream function** (subscribers with their own filters), on the room left in the path.
- **Store fill in link statistics** (SRV-11), so a dashboard warns before data is dropped.
