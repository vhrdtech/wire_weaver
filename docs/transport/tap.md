---
level: advanced
---

# The link tap

The link tap records what crosses a WireWeaver link: every frame as it crossed the medium (a USB packet, a UDP
datagram, an RTT chunk), every `ww_link` message as it went down to the framer or came back up, and the link events
the bytes don't show (connected, link ready, disconnected, errors). A decoder links frames and messages back
together, so you see which packet a request went out in, which packets a long reply was split over, and what a lost
packet took with it. On top of it sit `ww monitor`, `ww stats`, `ww capture` and `ww explain FILE`
([CLI reference](../cli_reference.md#ww-monitor)).

```
$ ww monitor --layer frame
   0.049465 L0 → frame #3 64 B: Full Data0 12 B, Full Data0 12 B, Full Data0 12 B, Full Data0 12 B, Start Data0 6/12 B
   0.049311 L0 → Data0 12 B msg #3 in #3@2+12 Request Call add seq 1
   ..
   0.053181 L0 ← frame #5 64 B: Start Data0 62/304 B
   0.053186 L0 ← frame #6 64 B: Continue Data0 62/242 B
   ..
   0.053194 L0 ← Data0 304 B msg #11 in #5..#9 (5 frames) Event Value blob seq 9 reply to #11 in 2.18 ms
```

## Where it records

The tap is the `ww_tap` crate (`no_std`). A transport calls its `Tap` at the two layers it has: `frame(dir, bytes)`
and `message(dir, kind, bytes)`; the event loop adds `link_event`. The tap only copies bytes and stamps a time:
linking, statistics and decoding happen in the consumer, so the link pays for a copy and nothing more.

- **Host.** Every `wire_weaver_client` event loop records into a `ww_tap::Recorder`: the process-wide
  `tap::default_recorder()`, or the one named with `ClientConfig::tap(recorder)`. Nothing is copied while nobody
  subscribes. `Command::RegisterTracer` and its `TraceEvent`s are an adapter over the same records.
- **Device.** `ww_device`'s `FramedTx::with_tap` / `FramedRx::with_tap` take a tap; the default is `NoTap`, whose
  calls are empty and inline, so firmware without a tap is the same size as before (checked on an STM32G0 build).
  `ww_framer` itself has no callbacks: `ww_framer::pieces(frame)` walks a frame into its pieces with the same head
  and checksum code `FramedRx` uses, and the decoder re-runs the framer on the recorded frames.

## Batched into blocks

The `Recorder` batches records into pooled blocks of 64 KiB (`RecorderConfig::block_size`) and hands a block over
when it is full or `RecorderConfig::flush_time` (10 ms by default) after its first record, so a live monitor still
feels live while a fast link costs one hand-over per block, not per message. Blocks go back to the pool once every
consumer dropped them: the steady state allocates nothing. When a consumer falls so far behind that every block is
held, records are dropped and counted, and the next block on the link opens with a `LinkEvent::Lost { records }`: a
gap is always visible, and the link itself is never held up.

Subscribe with `Recorder::subscribe(layers, queue)` (a bounded queue of blocks; a consumer that falls further behind
misses blocks, seen as a gap in their numbers) or `Recorder::subscribe_fn`. `Layers::MESSAGES` leaves frames out,
`Layers::ALL` has them.

## Records

The records are ordinary WireWeaver types: streams of the `ww_tap::LinkTap` trait, one entry per link
(`link[id]`) with the streams `session` (medium, framer, frame size, peer, the device's `DeviceInfo` once the link is
set up), `events`, `tx_frames`, `rx_frames`, `tx_messages`, `rx_messages` and, on a server, `client_map` (which
client's request a device-side request was). A capture is described by the API bundle of `ww_tap` like any device
log is by its device's, and every frame and message has a number, so the decoder sees a missing one.

## Capture files

A capture is [logbook](#logbook-blocks) blocks appended back to back: `ww capture -o FILE` appends, and a file cut
short or damaged in the middle still reads up to and after the damage. The describe blocks carry the device's API
bundle, so `ww explain FILE` explains the requests and replies of a capture on any PC, without the device.

### Logbook blocks

`ww_logbook` is the generic block format the tap writes; nothing in it knows the tap. A block is a 32 byte header
(magic `WWLB`, version, kind, length, record count, the producer's block number, a time base), records (track,
time delta, length, item bytes as LEB128 varints) and a CRC-32C, as long as its records and never padded. A
**describe** block carries the producer (wall clock at its time 0), a `TrackDef` per track (number, stable key, API,
resource path, label, whether it is sticky) and the API bundles the items need, then the newest record of every
sticky track. It is written at the start, when a track is added, when a consumer joins, after a loss and every
second, so a consumer that joins late decodes from its first block.

## Decoding

`wire_weaver_client::tap::Decoder` takes blocks, live or from a file (`ww_logbook::FileBlocks`), and gives out
`Decoded` frames, messages and link events:

- **Linkage.** Every frame is walked into its pieces and reassembled with the framer's rules: a message knows the
  frame bytes it came in (`MessageInfo::frames`, `#5@2+62`: frame 5, offset 2, 62 bytes), a frame its pieces and
  what became of them (`FrameInfo::outcome`), a lost frame shows as a gap in the frame numbers and as the split
  message it broke.
- **Cross-check.** Each message the framer gives back is compared with the one the transport recorded: a
  `Check::Mismatch` is a framer bug.
- **The API.** Data messages are read as requests and events and explained with the device's API bundle (from the
  capture, or the local cache); a reply is paired with its request by `seq`, so its return value is explained by
  its method (no `--for` as with `ww explain event HEX`).
- **Statistics.** `Decoder::stats` per link and direction: frames, fill, overhead, messages per frame, split
  messages, dropped pieces, frame gaps, mismatches, message-to-frame latency, link message kinds, and per method its
  calls and reply times; `Stats::report()` is what `ww stats` prints.

Live and saved data go through the same code, so a capture prints exactly as the live session did.
