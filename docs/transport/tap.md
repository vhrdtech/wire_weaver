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

A capture is [tape](#tape-blocks) blocks appended back to back. `ww_tape::TapeWriter` writes it the way databases
write their logs: a new file as `NAME.partial`, renamed when finished, the file and its directory synced after the
create and after the rename; the data synced by a `SyncPolicy` (none, every N blocks, or an interval, 1 s by default:
`fdatasync` on Linux, `F_FULLFSYNC` on macOS, `FlushFileBuffers` on Windows); and a failed write or sync is the end of
the file, never retried (after a failed `fsync` Linux may drop the dirty pages, and a retry succeeds without them).
`ww_tape::FileBlocks` reads the longest valid prefix: it stops at the first block with a bad magic, length or CRC,
a cut one or one whose seq goes back, and says where; zeros at the end are the end. `Recovery::Resync` reads on past
damage instead. A capture ends with an index, a finish block and a tail ([below](#index-finish-and-tail)); cutting a recording into several files is [tape pieces](#tape-pieces); preallocating space is not built yet. The describe blocks carry the device's API
bundle, so `ww explain FILE` explains the requests and replies of a capture on any PC, without the device.

### Tape blocks

`ww_tape` is the generic block format the tap writes; nothing in it knows the tap. A block is a 32 byte header
(magic `WWTP`, version, kind, length, record count, the producer's block number), a body and a CRC-32C, as long as
its records and never padded. The body of a records or describe block starts with a clock table (the clocks its
records count), then records: a head (track and clock slot), a time delta, a length and the item bytes, as LEB128
varints. Record times are signed i64 ticks of the track's clock, each delta against the previous record on the same
clock, so a block reads alone and one clock costs no bit. No wall time is in a block.

A **describe** block carries, in a canonical order: the producer, the API bundles, the clocks (number, stable key,
exact rate and resolution as `Magnitude`s, origin: free-running, boot, Unix epoch, media start, run start, or the next epoch of a counter, fixed by an exact offset map), the
clock maps (segments `b = b0 + (a - a0) · slope`, the slope an exact_units `Ratio64`, with an error bound, strictly increasing across segments: the tap maps its own clock to the wall clock
read at its start), a `TrackDef` per track (number, stable key, clock, dimensions that are part of its identity, API,
resource path, optional item type, label, whether it is sticky) and typed items (any wire_weaver value by bundle
hash and type path), then the newest record of every sticky track. It is written at the start, when a track is
added, when a consumer joins and after a loss, never by time, so a consumer that joins late decodes from its first
block. `ww_tape::DescribeSet` keeps the records in that order, so the same input gives the same bytes whatever order
the producer added them in.

A **chunk** block holds one chunk of one track, read in place: an events chunk (up to a few thousand items of an event track) or a series chunk (a run of samples of a fixed-rate series). Its head says where it belongs without the describe set or an index: the track's identity hash and number, the first item number, the item count, the first and last tick and the latest end of any item (so an overlap query needs no decode of the ends), then the time and end modes, one entry per column (what it holds, its codec id, its channel, its offset) and a CRC-32C per 64 KiB segment of the payload. The columns follow back to back, then 16 zero bytes, so an unaligned 16-byte load at any column byte stays in the block; numbers are little endian. The head has a CRC of its own: when a payload is damaged, `Recovery::Resync` still trusts the head, skips the block by its length and hands it out with its bad segments marked (`Block::is_intact` false, `Chunk::bad_segments`, `Chunk::column_damaged`), so a flipped bit in a 4 MiB series chunk loses one 64 KiB segment. A block with a bad head is skipped one byte on, to the next magic. Block kind 5 is reserved for parity blocks; none are written yet.

A **summary** block holds a batch of consecutive nodes of one level of a track's summary tree, so an overview of a long track reads a few hundred KB instead of its items. Nodes run over item numbers, not time: a level-0 node of an event track holds 2048 items, each level up 16 nodes of the level below, and node `n` holds items `[n << shift, (n + 1) << shift)`, so the first and last node of a tape piece may hold fewer items and the nodes of two pieces line up. An event node holds its first and last item's start, the OR of the items' flags and, for spans, the latest end; counts follow from the item range. A series node holds `2^base_shift` samples and, per channel, the statistics the track declares (`SeriesSummarySpec`): minimum, maximum, sum, sum of squares, a count of clipped or NaN samples, the flags OR, and for complex samples the mean and maximum of `I² + Q²`; no ticks, since a series' ticks follow from its first tick and period. Every statistic merges in item order (`EventNode::merge`, `ChannelStats::merge`), so a reader building coarser bins gets the bits the writer stored one level up. The head names the track, the level, the node size, the element and channels of a series, the statistics stored, the items covered and their tick range, so an index lists the block without reading its body; the body is one column per field, little endian. The writer (`EventTree`, `SeriesSummaries`) writes a block when a level's batch fills and the partial nodes at `finish`, up to a level with a single node. The blocks are small (a few KiB) and go through `TapeWriter::defer_block`, so in an aligned file they fill the gaps before chunk payloads and cost no space; a lost one is rebuilt from the data.

The **times** of an events chunk are coded by `ww_tape::times`, and every read runs on the chunk bytes as they are, with no decode into a list first. Starts are either *implicit* (the chunk's first tick plus a period per item, for evenly spaced items) or a *linear frame of reference* per group of 128 items: a 25-byte record per group (its first tick, a slope in Q16 ticks per item, the smallest residual, a width and an offset), then each item's residual against the line, bit-packed at the group's width. Periodic items cost their jitter, not their period: 1 ms with ±300 ns jitter takes about 1.45 bytes per item, exactly periodic ones almost nothing, bursty nanosecond traffic about 3.8 (8 for raw i64). A get is one record and one load, a lower bound searches the group first ticks, then only the items the slope leaves possible. Ends are absent (instants), the next item's start (spans back to back, nothing stored) or packed durations, whose group minimum plus 2^width bounds the group's longest item for overlap queries. Flags are an optional u32 per item, packed per group with the OR of the group's flags, so a zoomed-out view reads the ORs without a decode. Groups decode with hand-written kernels (`ww_tape::bitpack`: AVX-512, AVX2 or NEON, picked at run time, else scalar) at a fraction of a nanosecond per item. A chunk whose CRC failed still reads through `EventTimes::salvage`: group records are checked one by one, and only the items of a group that doesn't hold up are marked suspect.

A file states in its describe set how its blocks are placed (`Layout`, alignment 0 or 4096; none means 0). In an aligned file (series files) every chunk payload starts on a 4 KiB boundary: the chunk's head sits in front of it, small blocks waiting for a place (`TapeWriter::defer_block`) fill the gap before the head, and what is left is zeros inside the head. Recovery looks for a block at every byte, aligned or not, and takes one only when its CRCs match.

### Index, finish and tail

A writer lists what it places. Every few MiB (`ww_tape::IndexPolicy`, 4 MiB by default; by time only when asked, since that makes the bytes depend on the clock) it writes a checkpoint **index** block: one 72-byte entry per block placed since the previous checkpoint (offset, length, kind, track id and number, first item number, item count, first and last tick, latest end; a summary block's level and tree), and a pointer back at that checkpoint. Chunk, summary, record and describe blocks are all listed; record and describe blocks carry no track id or ticks. In an aligned file a checkpoint goes into the gap before the next chunk payload. Blocks the file structure adds (index, finish, tail, piece head, and describe blocks the writer repeats) carry the seq of the block before them and are not counted, so seq stays the producer's count of its own blocks and a pass-through capture keeps its order. A crash costs at most the chunks not yet written, and a reader rescans only the stretch after the newest checkpoint.

`TapeWriter::finish` ends the file: a backup of the newest describe blocks, a **final index** listing every block again sorted by track, with a directory of tracks in its head and a CRC per track's entries (a reader of one track out of thousands reads the head and that track's list), the **finish** block (complete, continued or aborted; the BLAKE3 content hash of every byte before it; where the newest checkpoint, the final index and the describe backup are; per track the chunks, items and ticks the file holds) and a fixed-size **tail** of 164 bytes as the last bytes of the file, which says where the file's piece starts and where its finish is. Offsets count from the start of the piece, so the index stays valid when files are concatenated. The writer also writes the newest describe blocks again about every 128 MiB. A lost index block costs a scan to the neighbouring checkpoint; a lost tail or finish only the content-hash check.

### Tape pieces

A recording can be cut into **pieces**, each a complete file that reads alone: a piece head block, the describe blocks, the data with its checkpoints, the final index, the finish and the tail. `ww_tape::PieceChain` writes `dir/00000000.wwt`, `00000001.wwt`, ... and cuts before a chunk by size, chunk count or age (`CutPolicy`); `TapeWriter::cut` cuts by hand. The head names the result, the piece's sequence number, its id, the previous piece's content hash and id, and per track where its items continue; the previous piece's finish and tail name this piece's id, derived at the cut from the previous piece's id and the content hash in its finish, so the chain is checked both ways (`ww_tape::piece::check_chain`). Pieces written back to back are one valid tape. A chain whose first pieces were deleted (a rolling window) reads as *head cut*, not as damage; one whose last piece has no tail is open (being written, or cut off by a crash).

### Reading by position

`ww_tape::TapeReader` reads a tape through a `ByteSource`, anything with a size that reads bytes at an offset: a byte slice, a `File`, or an HTTP resource read by range requests. It needs no file system and no threads, so it builds for `wasm32-unknown-unknown` (feature `alloc` without `std`). It finds the pieces from the end: a good tail gives a piece's start and finish, and the previous piece's tail sits just before that start, so concatenated pieces cost one small read each. A piece without a tail is found from its newest checkpoint, looked for backwards from the end, and the stretch after it is read block by block; a piece without any checkpoint is read from the start. `entries` and `track_entries` give where a piece's blocks are, `read_block` reads and checks one, `verify` checks the content hash, `chain` how the pieces link up.

The header and the CRC at the end stay the same in every version: a later version changes only the body, and a reader skips a block of a version or kind it doesn't know by its length, as it skips describe records of a kind it doesn't know.

### Sample codecs

A series chunk's sample columns (`ColumnKind::Samples`) are coded by the column's codec id; what a sample is comes from the track: `ww_tape::SampleShape`, an `Element` (`I8` to `U64`, `F32`, `F64`, `ComplexI16`, `ComplexF32`, `Bool`; its code, 0 to 12, is how other blocks name it) and the channels interleaved in the column. `ww_tape::Samples` reads a column in place and decodes any range of samples; `ww_tape::samples::encode` writes one. Every codec gives the same bytes for the same values.

| Id | Codec | Elements | Stored as |
|---:|---|---|---|
| 0 | `RAW` | all | little endian; `Bool` 8 per byte, the first in bit 0 |
| 16 | `BIG_ENDIAN` | integers, floats | big endian |
| 17, 18 | `PACKED10_LSB`, `PACKED10_MSB` | integers of 16 bits and more | 10 bits per value |
| 19, 20 | `PACKED12_LSB`, `PACKED12_MSB` | integers of 16 bits and more | 12 bits per value, 2 values in 3 bytes |
| 21, 22 | `PACKED24_LSB`, `PACKED24_MSB` | integers of 32 bits and more | 24 bits per value |
| 24 | `GORILLA` | floats | XOR with the lane's previous value, Gorilla's control bits |
| 25 | `CHIMP` | floats | XOR with the lane's previous value, Chimp's control bits |
| 32, 33 | `DELTA`, `DELTA2` | integers | differences (or differences of differences), minus their minimum, bit-packed |
| 34, 35 | `FLOAT_DELTA`, `FLOAT_DELTA2` | floats | the same on the float bits mapped to ordered integers |
| 48 | `LZ4` | payload columns | one lz4 block after its decoded length (`ww_tape::lz4`) |

- **Raw device formats** (0 to 22) are what a device sends, stored as it is: the reader converts only the samples it reads, and any sample reads in place. An `_LSB` column holds value `i` at bits `i·w` up of the column read as one little-endian number; an `_MSB` column counts bits from the top of byte 0. A packed value of a signed element is sign-extended.
- **XOR codecs** (24, 25) are lossless for every bit pattern, NaN payloads and the sign of zero included. They code groups of 1024 samples, lane by lane.
- **Delta codecs** (32 to 35) code groups of 128 samples on the bit-pack primitive the event time codec uses (`ww_tape::bitpack`). Per lane, a group holds the first value, the minimum difference and a width, then the differences minus that minimum at that width, so a ramp or a constant costs no bits per sample.
- **Groups.** Both grouped families start the column with a table of where each group ends, and every group restarts its prediction. A range decodes from its group's start, and damage costs only the groups it touches: `Samples::group_bytes` names a group's bytes, to check against `Chunk::range_damaged`.

Which codec fits depends on the data. On a motor simulation (every variable of `motor_sim`, 25 kHz) and on decoded speech, lossless float coding gave these ratios against raw:

| Data | `GORILLA` | `CHIMP` | `FLOAT_DELTA` | `FLOAT_DELTA2` |
|---|---:|---:|---:|---:|
| motor, `f32` | 1.18 | 1.38 | 1.21 | 1.23 |
| motor, `f64` | 2.32 | 2.28 | 2.68 | 2.87 |
| speech, `f32` | 2.31 | 1.83 | 0.99 | 0.99 |

The delta codecs decoded at 1 to 3 ns per value, the XOR codecs at 2 to 9 (x86-64, one core). The codec id is stored per chunk, so a writer may try several on a chunk and keep the smallest.

## Decoding

`wire_weaver_client::tap::Decoder` takes blocks, live or from a file (`ww_tape::FileBlocks`), and gives out
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
