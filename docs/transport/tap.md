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

The tape format the tap writes has a chapter of its own, [Tapes](../tape/index.md): what a file holds, how time
is stored, and what survives damage. The sections below are a reference to its parts.

A capture is [tape](#tape-blocks) blocks appended back to back. `ww_tape::TapeWriter` writes it the way databases
write their logs: a new file as `NAME.partial`, renamed when finished, the file and its directory synced after the
create and after the rename; the data synced by a `SyncPolicy` (none, every N blocks, or an interval, 1 s by default:
`fdatasync` on Linux, `F_FULLFSYNC` on macOS, `FlushFileBuffers` on Windows); and a failed write or sync is the end of
the file, never retried (after a failed `fsync` Linux may drop the dirty pages, and a retry succeeds without them).
`ww_tape::FileBlocks` reads the longest valid prefix: it stops at the first block with a bad magic, length or CRC,
a cut one or one whose seq goes back, and says where; blank bytes at the end (all 0x00 or all 0xFF) are the end. `Recovery::Resync` reads on past
damage instead. A capture ends with an index, a finish block and a tail ([below](#index-finish-and-tail)); cutting a recording into several files is [tape pieces](#tape-pieces); preallocating space is not built yet. The describe blocks carry the device's API
bundle, so `ww explain FILE` explains the requests and replies of a capture on any PC, without the device.

### Tape blocks

`ww_tape` is the generic block format the tap writes; nothing in it knows the tap. A block is a 32 byte header
(magic `WWTP`, version, kind, length, record count, the block number in its run, the run tag), a body and a CRC-32C,
as long as its records and never padded. A run is one writer's unbroken append (a producer start, a device boot): its
blocks carry the same random 32-bit run tag (`ww_tape::RunTag`, never 0 nor FFFFFFFFh, derived from the piece id
for a piece chain so the same input gives the same bytes), and their numbers count from 0. The body of a records or describe block starts with a clock table (the clocks its
records count), then records: a head (track and clock slot), a time delta, a length and the item bytes, as LEB128
varints. Record times are signed i64 ticks of the track's clock, each delta against the previous record on the same
clock, so a block reads alone and one clock costs no bit. No wall time is in a block.

A **describe** block carries, in a canonical order: the producer, the API bundles, the clocks (number, stable key,
exact rate and resolution as `Magnitude`s, origin: free-running, boot, Unix epoch, media start, run start, or the next epoch of a counter, fixed by an exact offset map), the
clock maps (segments `b = b0 + (a - a0) · slope`, the slope an exact_units `Ratio64`, with an error bound, strictly increasing across segments: the tap maps its own clock to the wall clock
read at its start), a `TrackDef` per track (number, stable key, clock, dimensions that are part of its identity, API,
resource path, optional item type, label, whether it is sticky, and its kind: events, with or without ends and flags, or a [series](#series-tracks)) and typed items (any wire_weaver value by bundle
hash and type path), then the newest record of every sticky track. It is written at the start, when a track is
added, when a consumer joins and after a loss, never by time, so a consumer that joins late decodes from its first
block. `ww_tape::DescribeSet` keeps the records in that order, so the same input gives the same bytes whatever order
the producer added them in.

A **chunk** block holds one chunk of one track, read in place: an events chunk (up to a few thousand items of an event track) or a series chunk (a run of samples of a fixed-rate series). Its head says where it belongs without the describe set or an index: the track's identity hash and number, the first item number, the item count, the first and last tick and the latest end of any item (so an overlap query needs no decode of the ends), then the time and end modes, one entry per column (what it holds, its codec id, its channel, its offset) and a CRC-32C per 64 KiB segment of the payload. The columns follow back to back, then 16 zero bytes, so an unaligned 16-byte load at any column byte stays in the block; numbers are little endian. The head has a CRC of its own: when a payload is damaged, `Recovery::Resync` still trusts the head, skips the block by its length and hands it out with its bad segments marked (`Block::is_intact` false, `Chunk::bad_segments`, `Chunk::column_damaged`), so a flipped bit in a 4 MiB series chunk loses one 64 KiB segment. A block with a bad head is skipped one byte on, to the next magic. Block kind 5 is reserved for parity blocks; none are written yet.

A **summary** block holds a batch of consecutive nodes of one level of a track's summary tree, so an overview of a long track reads a few hundred KB instead of its items. Nodes run over item numbers, not time: a level-0 node of an event track holds 2048 items, each level up 16 nodes of the level below, and node `n` holds items `[n << shift, (n + 1) << shift)`, so the first and last node of a tape piece may hold fewer items and the nodes of two pieces line up. An event node holds its first and last item's start, the OR of the items' flags and, for spans, the latest end; counts follow from the item range. A series node holds `2^base_shift` samples and, per channel, the statistics the track declares (`SeriesSummarySpec`): minimum, maximum, sum, sum of squares, a count of clipped or NaN samples, the flags OR, and for complex samples the mean and maximum of `I² + Q²`; no ticks, since a series' ticks follow from its segments' grids and period. Every statistic merges in item order (`EventNode::merge`, `ChannelStats::merge`), so a reader building coarser bins gets the bits the writer stored one level up. A segment may change the scale of a series' numbers mid-node (an autoranging device changes prefix); such a node is kept at the common scale of its samples (`common_scale`: `10⁻⁶` for `10⁻³` and `10⁻⁶`), the coarser numbers multiplied by a whole factor, so minimum and maximum stay exact integers in the sample type's unit. A series block states its scale in its head, the common scale of all its nodes, so it is read alone without the segment records; `SeriesNode::merge` merges nodes of two scales (two blocks, a view cache across a change) at their common scale, and refuses scales with no common one (a ratio that isn't a fraction of integers) rather than mixing them. The head names the track, the level, the node size, the element, channels and scale of a series, the statistics stored, the items covered and their tick range, so an index lists the block without reading its body; the body is one column per field, little endian. The writer (`EventTree`, `SeriesSummaries`) writes a block when a level's batch fills and the partial nodes at `finish`, up to a level with a single node. The blocks are small (a few KiB) and go through `TapeWriter::defer_block`, so in an aligned file they fill the gaps before chunk payloads and cost no space; a lost one is rebuilt from the data.

The **times** of an events chunk are coded by `ww_tape::times`, and every read runs on the chunk bytes as they are, with no decode into a list first. Starts are either *implicit* (the chunk's first tick plus a period per item, for evenly spaced items) or a *linear frame of reference* per group of 128 items: a 25-byte record per group (its first tick, a slope in Q16 ticks per item, the smallest residual, a width and an offset), then each item's residual against the line, bit-packed at the group's width. Periodic items cost their jitter, not their period: 1 ms with ±300 ns jitter takes about 1.45 bytes per item, exactly periodic ones almost nothing, bursty nanosecond traffic about 3.8 (8 for raw i64). A get is one record and one load, a lower bound searches the group first ticks, then only the items the slope leaves possible. Ends are absent (instants), the next item's start (spans back to back, nothing stored) or packed durations, whose group minimum plus 2^width bounds the group's longest item for overlap queries. Flags are an optional u32 per item, packed per group with the OR of the group's flags, so a zoomed-out view reads the ORs without a decode. Groups decode with hand-written kernels (`ww_tape::bitpack`: AVX-512, AVX2 or NEON, picked at run time, else scalar) at a fraction of a nanosecond per item. A chunk whose CRC failed still reads through `EventTimes::salvage`: group records are checked one by one, and only the items of a group that doesn't hold up are marked suspect.

A file states in its describe set how its blocks are placed (`Layout`, alignment 0 or 4096; none means 0). In an aligned file (series files) every chunk payload starts on a 4 KiB boundary: the chunk's head sits in front of it, small blocks waiting for a place (`TapeWriter::defer_block`) fill the gap before the head, and what is left is zeros inside the head. Recovery looks for a block at every byte, aligned or not, and takes one only when its CRCs match.

### Index, finish and tail

A writer lists what it places. Every few MiB (`ww_tape::IndexPolicy`, 4 MiB by default; by time only when asked, since that makes the bytes depend on the clock) it writes a checkpoint **index** block: one 72-byte entry per block placed since the previous checkpoint (offset, length, kind, track id and number, first item number, item count, first and last tick, latest end; a summary block's level and tree), and a pointer back at that checkpoint. Chunk, summary, record and describe blocks are all listed; record and describe blocks carry no track id or ticks. In an aligned file a checkpoint goes into the gap before the next chunk payload. Blocks the file structure adds (index, finish, tail, piece head, and describe blocks the writer repeats) carry the seq of the block before them and are not counted, so seq stays the producer's count of its own blocks and a pass-through capture keeps its order. A crash costs at most the chunks not yet written, and a reader rescans only the stretch after the newest checkpoint.

`TapeWriter::finish` ends the file: a backup of the newest describe blocks, a **final index** listing every block again sorted by track, with a directory of tracks in its head and a CRC per track's entries (a reader of one track out of thousands reads the head and that track's list), the **finish** block (complete, continued or aborted; the BLAKE3 content hash of every byte before it; where the newest checkpoint, the final index and the describe backup are; per track the chunks, items and ticks the file holds) and a fixed-size **tail** of 164 bytes as the last bytes of the file, which says where the file's piece starts and where its finish is. Offsets count from the start of the piece, so the index stays valid when files are concatenated. The writer also writes the newest describe blocks again as copies ([below](#runs-media-and-damage)). A lost index block costs a scan to the neighbouring checkpoint; a lost tail or finish only the content-hash check.

### Tape pieces

A recording can be cut into **pieces**, each a complete file that reads alone: a piece head block, the describe blocks, the data with its checkpoints, the final index, the finish and the tail. `ww_tape::PieceChain` writes `dir/00000000.wwt`, `00000001.wwt`, ... and cuts before a chunk by size, chunk count or age (`CutPolicy`); `TapeWriter::cut` cuts by hand. The head names the result, the piece's sequence number, its id, the previous piece's content hash and id, and per track where its items continue; the previous piece's finish and tail name this piece's id, derived at the cut from the previous piece's id and the content hash in its finish, so the chain is checked both ways (`ww_tape::piece::check_chain`). Pieces written back to back are one valid tape. A chain whose first pieces were deleted (a rolling window) reads as *head cut*, not as damage; one whose last piece has no tail is open (being written, or cut off by a crash).

### Reading by position

`ww_tape::TapeReader` reads a tape through a `ByteSource`, anything with a size that reads bytes at an offset: a byte slice, a `File`, or an HTTP resource read by range requests. It needs no file system and no threads, so it builds for `wasm32-unknown-unknown` (feature `alloc` without `std`). It finds the pieces from the end: a good tail gives a piece's start and finish, and the previous piece's tail sits just before that start, so concatenated pieces cost one small read each. A piece without a tail is found from its newest checkpoint, looked for backwards from the end, and the stretch after it is read block by block; a piece without any checkpoint is read from the start. `entries` and `track_entries` give where a piece's blocks are, `read_block` reads and checks one, `describe_set` gives the piece's describe set, `verify` checks the content hash, `chain` how the pieces link up. A raw medium is read forward instead (`OpenOptions::medium`, [below](#runs-media-and-damage)).

The header and the CRC at the end stay the same in every version: a later version changes only the body, and a reader skips a block of a version or kind it doesn't know by its length, as it skips describe records of a kind it doesn't know.

### Runs, media and damage

[Damage and recovery](../tape/recovery.md) explains these rules with examples.

A **run** is one writer's unbroken append: a producer start, a device boot, one pass of an importer. Every block of a run carries the same run tag in header bytes 24..28, and its seq counts from 0. A writer picks the tag at random, except for the first piece of a chain, whose tag is derived from the piece id so the same input gives the same bytes. A piece head names the run that ended before it (`PieceHead::prev_run`). A reader keeps to one run at a time: a block of another run is stale data, unless a run may start there. In a file a new run starts at a describe block (another producer appended) or a piece head; on a raw medium only at a piece head that names the current run as the one before it. A piece on a raw medium is therefore one run: a writer that carries a producer's blocks, each with the producer's own run, renumbers them into its own run with `TapeWriter::adopt_block`. Lost blocks stay visible: a jump in the producer's seq leaves the same jump in the writer's, and a producer restart (another run) leaves none. Passed through with `write_block`, they read whole from a file but as stale data on a raw medium.

The **medium** decides where a reader starts. A file is read from its start, or its pieces are found from its end. A raw medium (an SD card, raw NAND, a partition, `Medium::Raw`) may hold blocks of earlier recordings behind the end of the current data, so a reader walks it forward from its start or from a hint and never trusts what it finds at the end. For example, a card recorded twice holds the second recording's blocks followed by the tail of the first: the first recording's run tag ends the walk.

**Blank** space is bytes that are all 0x00 or 0xFF, as an erased or preallocated medium reads. A blank or damaged stretch is a **gap**, not the end, when the current run's next block follows it within the layout's `gap_max`, found at the first non-blank byte or at a multiple of `resume_align`. The describe set states both in its `Layout`:

| Medium | `gap_max` | `resume_align` |
|---|---|---|
| file (`Layout::UNALIGNED`, `ALIGNED`) | 0: no gaps | 1 |
| SD card or eMMC (`on_sd_card`) | 4 MiB | 512 bytes, a sector |
| raw NAND (`on_raw_nand`) | two erase blocks | one page |

A reader's cost per gap is bounded. A valid `Layout` has a `gap_max` of at most 64 MiB (`GAP_MAX_MAX`) and at most 8192 resume places in it (`GAP_PROBES_MAX`, `gap_max / resume_align`); a describe record of any other layout doesn't decode. Past a blank or bad stretch a reader reads one block header per resume place, and checks a block whole only when its header says it goes on with the data, at most 4 times per gap (`GAP_CHECKS_MAX`). When none of them does, the stretch is the end of data. On a raw medium, blank space longer than `gap_max` is the end too, and a reader reads nothing after it: an erased 64 GB card costs one gap's worth of reads.

A writer on raw NAND programs each page once, in page order: at a sync it pads the rest of the page with 0xFF and starts the next block on the next page. The padding reads as a gap, so the format needs nothing else; the cost is up to one page per sync. Keeping MLC paired pages of unsynced writes apart from synced data is the medium layer's task.

The same run and seq twice is a **duplicate**, not a cut: a writer that retried a write may leave a block twice. With the same bytes the second one is dropped; with other bytes the later one is taken and reported (`FileError::Replaced`, `PieceInfo::replaced`).

Header **flags** (bytes 6..8) mark three things. Bit 1 (`FLAG_COPY`) marks a describe block the writer wrote again, which carries the seq of the block before it and adds nothing to a reader that read the original. Bit 0 (`FLAG_BODY_CODED`) marks a records or describe body as coded: a codec byte (0 stored, 1 lz4), the decoded length and the coded bytes. Coding is off unless a writer calls `ww_tape::code_body`; a reader decodes with `Block::decode` and takes a body of a codec it doesn't know as a body it doesn't know. Chunk blocks are never coded, since their columns are read in place. Bits 2 and 3 (`FLAG_SET_FIRST`, `FLAG_SET_LAST`) mark the first and the last block of a describe set, so a reader tells a whole set from part of one. A set is whole when a block marked first is followed, back to back, by blocks up to one marked last; a set of one block carries both marks. `DescribeSet::write_blocks` sets them.

**Describe copies** keep a piece readable when its head describe set is damaged. The writer copies the newest describe blocks right after the piece's first records or chunk block, then about every 128 MiB (`TapeWriter::set_describe_repeat`), and once more before the finish. A piece so holds one describe set while it holds no data, two from its first data block on, one more per repeat, and one more once finished. The writer copies only a whole set, never one whose last block hasn't come yet. `TapeReader::describe_set` returns the head's set, or the next one that reads whole; past a scan's stop it looks only at blocks of the runs the piece goes on with, so a stale set of an earlier recording behind the end of data is never taken.

The **item of an events record** whose track has spans or flags is an `EventRecord`: the duration (spans only), the flags (only when the track has flags), then the payload, the record's time being the start. A track of plain instants carries the payload alone. Sticky records of events tracks carry the same item.

A chunk column whose codec byte has bit 7 set is **outer-coded**: an outer codec byte, the decoded length, then the bytes the inner codec (bits 0..6) applies to after decoding. No outer codec exists yet, so every reader refuses such a column. In general a reader refuses a column whose kind or codec it doesn't know and still reads the chunk's other columns.

Some numbers are reserved so later features need no format change; today's readers refuse each:

| Reserved | For |
|---|---|
| payload codec 49 (`Codec::SERIES`) | a payloads column holding one shrink_wrap `Series<T>` of the chunk's items, alone, at most 65535 items |
| sample codec 36 (`Codec::RUNS`) | run-coded discrete lanes (`U8`, `U16`, `U32`, `Bool`) |
| element 13 (`Element::LEAVES`) | a sample that is a struct of leaves of different element types |
| column kind 7 (`ColumnKind::PAYLOAD_LEAF`) | one leaf of the items' payload struct, the param naming the leaf |
| summary head bytes 54..56 | the first channel or leaf of the block, 0 today |

### Sample codecs

[Events and series](../tape/events_and_series.md#series) explains series, sample codecs and why fixed point comes
first.

A series chunk's sample columns (`ColumnKind::Samples`) are coded by the column's codec id; what a sample is comes from the track: `ww_tape::SampleShape`, an `Element` (`I8` to `U64`, `F32`, `F64`, `ComplexI16`, `ComplexF32`, `Bool`; its code, 0 to 12, is how other blocks name it) and the channels interleaved in the column. `ww_tape::Samples` reads a column in place and decodes any range of samples; `ww_tape::samples::encode` writes one. Every codec gives the same bytes for the same values.

| Id | Codec | Elements | Stored as |
|---:|---|---|---|
| 0 | `RAW` | all | little endian; `Bool` 8 per byte, the first in bit 0 |
| 16 | `BIG_ENDIAN` | integers, floats | big endian |
| 17, 18 | `PACKED10_LSB`, `PACKED10_MSB` | integers of 16 bits and more | 10 bits per value |
| 19, 20 | `PACKED12_LSB`, `PACKED12_MSB` | integers of 16 bits and more | 12 bits per value, 2 values in 3 bytes |
| 21, 22 | `PACKED24_LSB`, `PACKED24_MSB` | integers of 32 bits and more | 24 bits per value |
| 24, 25 | | | reserved: refused as an unknown codec |
| 26 | `ALP` | floats | decimals as integers, frame of reference, bit-packed; other floats by their front bits through a dictionary |
| 32, 33 | `DELTA`, `DELTA2` | integers | differences (or differences of differences), minus their minimum, bit-packed |
| 34, 35 | | | reserved: refused as an unknown codec |
| 48 | `LZ4` | payload columns | one lz4 block after its decoded length (`ww_tape::lz4`) |

- **Raw device formats** (0 to 22) are what a device sends, stored as it is: the reader converts only the samples it reads, and any sample reads in place. An `_LSB` column holds value `i` at bits `i·w` up of the column read as one little-endian number; an `_MSB` column counts bits from the top of byte 0. A packed value of a signed element is sign-extended.
- **Delta codecs** (32, 33) code groups of 128 samples on the bit-pack primitive the event time codec uses (`ww_tape::bitpack`). Per lane, a group holds the first value, the minimum difference and a width, then the differences minus that minimum at that width, so a ramp or a constant costs no bits per sample.
- **ALP** (26) follows ALP (Afroozeh, Kuffó and Boncz, SIGMOD 2024) in groups of 128 samples. Floats that began as decimals (a reading times 0.1, a price, a value printed and parsed back) are stored as integers: per group and lane one exponent `e` and factor `f`, each value `d` such that `d × 10^f × 10^-e` gives its bits back exactly, the `d`s minus their minimum bit-packed. Values that don't come back bit for bit (NaN payloads, `-0.0`, infinities, more digits) are kept whole as exceptions inside the group. Other floats take ALP's second scheme, `rd`: up to 16 front bits through a dictionary of up to 8 entries, the rest bit-packed without the low zero bits the group shares. The encoder keeps per group the smallest form, so a group never grows by more than 7 bytes over raw; the layout is in the `ww_tape::samples::alp` docs.
- **Groups.** The grouped codecs start the column with a table of where each group ends, and every group restarts its prediction. A range decodes from its group's start, and damage costs only the groups it touches: `Samples::group_bytes` names a group's bytes, to check against `Chunk::range_damaged`.

For data a device controls, fixed-point numbers are the default (an exact_units `Fixed` in an integer column with `DELTA`, see [series](../serdes/series.md)): they take the fewest bits and decode fastest. ALP, the one float codec, is for float sources nobody controls. Ids 24, 25, 34 and 35 are reserved; `ww_tape_benchmarks` keeps Gorilla, Chimp and delta on ordered float bits as bench-only codecs to compare against. The codec id is stored per chunk, so a writer may try several on a chunk and keep the smallest.

`just bench-sample-codecs` measures every float codec on four kinds of data, each next to the same quantity stored as fixed-point integers with `DELTA` (first row of each set). Ratio is to the raw floats, in chunks of 4096 samples; times are per value on one core (AMD Ryzen AI 9 HX 370, AVX-512 kernels):

- **motor_sim**: every variable of `motor_sim`'s VESC preset (github.com/vhrdtech/motor_sim at 35eab07, the commit `Cargo.lock` names) for 1 s at 25 kHz, driven from its rotor angle with a fan load, as `f32` and as `f64` (its `f32` variables widened). Fixed point: `i32` at the finest power-of-ten step that keeps 24 bits at each variable's peak.
- **speech**: Roosevelt's Pearl Harbor address (Wikimedia Commons, public domain, `Roosevelt_Pearl_Harbor.ogg`, SHA-1 `90239988...`), decoded to `f32` by lewton. Fixed point: lewton's 16-bit PCM of the same decode.
- **ADC**: 12-bit counts of 4 slow noisy channels, 250 000 samples each, turned into floats as firmware does: `counts × 3.3/4096` and `counts × 0.1`. Fixed point: the counts.

| Data | Codec | Ratio | Bits/value | Encode ns | Decode ns |
|---|---|---:|---:|---:|---:|
| motor_sim `f32` | `DELTA` of `i32` | 3.10 | 10.3 | 2.1 | 0.66 |
| | `GORILLA` | 1.49 | 21.5 | 5.4 | 2.42 |
| | `CHIMP` | 1.74 | 18.4 | 7.0 | 3.78 |
| | `FLOAT_DELTA` | 1.93 | 16.6 | 2.3 | 0.86 |
| | `FLOAT_DELTA2` | 1.99 | 16.1 | 2.5 | 1.02 |
| | `ALP` | 1.64 | 19.5 | 14.1 | 1.17 |
| motor_sim `f64` | `DELTA` of `i32` | 6.20 | 10.3 | 2.1 | 0.65 |
| | `GORILLA` | 2.51 | 25.5 | 5.9 | 2.60 |
| | `CHIMP` | 2.50 | 25.6 | 11.3 | 2.87 |
| | `FLOAT_DELTA` | 1.50 | 42.6 | 2.4 | 1.29 |
| | `FLOAT_DELTA2` | 1.56 | 41.1 | 2.5 | 1.28 |
| | `ALP` | 2.51 | 25.5 | 17.6 | 1.24 |
| speech `f32`, 2 channels | `DELTA` of `i16` PCM | 3.43 | 9.3 | 2.3 | 0.71 |
| | `GORILLA` | 1.02 | 31.3 | 5.7 | 2.30 |
| | `CHIMP` | 1.23 | 26.0 | 8.2 | 5.59 |
| | `FLOAT_DELTA` | 1.10 | 29.1 | 2.5 | 0.96 |
| | `FLOAT_DELTA2` | 1.10 | 29.0 | 2.7 | 1.13 |
| | `ALP` | 1.16 | 27.6 | 15.7 | 1.27 |
| ADC × 3.3/4096 `f32` | `DELTA` of the counts | 7.31 | 4.4 | 2.2 | 0.65 |
| | `GORILLA` | 1.51 | 21.3 | 5.6 | 2.71 |
| | `CHIMP` | 1.76 | 18.2 | 10.5 | 6.87 |
| | `FLOAT_DELTA` | 1.84 | 17.4 | 2.4 | 0.81 |
| | `FLOAT_DELTA2` | 1.73 | 18.5 | 2.6 | 1.08 |
| | `ALP` | 1.59 | 20.2 | 15.1 | 1.17 |
| ADC × 3.3/4096 `f64` | `DELTA` of the counts | 14.63 | 4.4 | 2.2 | 0.65 |
| | `GORILLA` | 1.36 | 47.1 | 7.9 | 3.19 |
| | `CHIMP` | 1.57 | 40.8 | 13.1 | 7.28 |
| | `FLOAT_DELTA` | 1.37 | 46.7 | 2.5 | 1.32 |
| | `FLOAT_DELTA2` | 1.34 | 47.8 | 2.7 | 1.28 |
| | `ALP` | 1.32 | 48.6 | 21.9 | 1.59 |
| ADC × 0.1 `f32` | `DELTA` of the counts | 7.31 | 4.4 | 2.2 | 0.65 |
| | `GORILLA` | 1.53 | 21.0 | 5.5 | 2.67 |
| | `CHIMP` | 1.81 | 17.7 | 10.5 | 6.85 |
| | `FLOAT_DELTA` | 1.84 | 17.4 | 2.5 | 0.83 |
| | `FLOAT_DELTA2` | 1.73 | 18.6 | 2.6 | 0.96 |
| | `ALP` | 4.67 | 6.9 | 8.5 | 0.81 |
| ADC × 0.1 `f64` | `DELTA` of the counts | 14.63 | 4.4 | 2.2 | 0.65 |
| | `GORILLA` | 1.38 | 46.5 | 7.8 | 3.15 |
| | `CHIMP` | 1.59 | 40.3 | 13.1 | 7.27 |
| | `FLOAT_DELTA` | 1.37 | 46.7 | 2.5 | 1.36 |
| | `FLOAT_DELTA2` | 1.34 | 47.8 | 2.7 | 1.32 |
| | `ALP` | 9.02 | 7.1 | 8.9 | 0.72 |

What the numbers say:

- Fixed point is 1.6 to 9.3 times smaller than the best float codec and decodes fastest; on the motor its step keeps more precision than the `f32`s it replaces.
- `ALP` is the float codec for floats that began as decimals: 2.5 to 5.7 times smaller than the others there, decoding at bit-pack speed (0.7 to 0.8 ns). On widened `f32`s it matches `GORILLA`; on floats with full mantissas it ranges from 6 % ahead of the delta codecs (speech) to 18 % behind them (motor `f32`).
- The XOR codecs win on two sets, by 6 % (speech) and 15 % (ADC × 3.3/4096 `f64`), both `CHIMP`, and decode 2 to 10 times slower than the bit-packed codecs.
- A writer that tries `ALP` and `FLOAT_DELTA` on each chunk and keeps the smaller is the best float codec on four of the seven sets and within 13 % of the best on the others.

The bench runs in about 20 s: `just bench-sample-codecs` fetches the speech file into `target/bench-data` once, checks its hash, and prints the table; `--chunk` and `--repeats` change the chunk size and the timed passes. Its code is `ww_tape_benchmarks`.

### Series tracks

A series track holds samples at an exact period with no time per sample. Its `TrackDef` kind is a `SeriesDef`: the type of one value as the API declares it (a `TypeRef` to an exact_units `Fixed<Ampere, Milli, i32>`, a `ww_si` quantity or a plain float), the stored element and channels derived from that type, the period in ticks per sample as an exact `Ratio64` (48 kHz on a 1 ns clock is `62500/3`), the scale of the stored numbers as a `Magnitude` (`10⁻³` for that `Fixed`), the summaries the writer builds (`SeriesSummarySpec`) and, for samples kept in a media file, the stream. `SeriesDef::of::<Fixed<Ampere, Milli, i32>>(..)` takes the element and the scale from the type (`ww_tape::SeriesValue`).

The track's records are its `SeriesSegment`s, at the tick of their first sample: one at the start and one at every gap, restart or change of scale. A segment names its first sample's index, a time grid (sample `i` is at `grid_tick + (i - grid_index) · period`, rounded down) and the scale from there on. A gap starts a new grid after the last tick of the one before; a prefix change of an autoranging device keeps the grid, so no sample time moves by rounding. The track is sticky, so every describe set repeats its newest segment and each piece holds the segment its samples start in. A series chunk never spans two segments, so its numbers are at one scale and its head's first and last tick are its samples' on the grid.

`ww_tape::SeriesTimeline` collects a track's segments from its records in any order (`from_records`), checks each against its record time and the ones before, and gives any sample's tick and scale; `check_chunk` checks a chunk head against them. A received `Series<Fixed<..>>` batch goes into chunk columns as integers: each item's stored number is put into the column as is (`SeriesValue::write_parts`), and a `ww_si` quantity's prefix becomes the segment's scale, a change of prefix starting a segment where the `Series` restarts its numeric column. The wire codecs of `Series<T>` and these sample codecs are separate layers: a batch is decoded once on the host and coded again for random access in the file.

### Payload codec

An events chunk's payloads take two columns, both or neither (`ww_tape::payloads`): `EncodedPayloads::encode` writes them from the items' bytes, `Payloads::read` reads them back by item.

- **Lengths** (`ColumnKind::PayloadLengths`, codec 4, `payloads::LENGTHS`): per group of 128 items a 13-byte record (where the group's first payload starts, the group's shortest length, a width of 0 to 32 bits, where its packed lengths start), then each group's lengths less that minimum, bit-packed at its width. Payloads of one length cost the records only, and the record's start places a group's payloads without the groups before it.
- **Payloads** (`ColumnKind::Payloads`): the items' bytes back to back, `RAW` and read in place, or `LZ4` (48) when that is smaller, decoded once when the chunk is opened.

A group's lengths are stored in one form only: its minimum is its shortest length, its width the bits the largest packed value needs, and the bits after its last value are 0. A reader refuses any other, so damage to a width or a minimum can't make a group read as other lengths of the same total. `Payloads::salvage` reads a chunk whose payload CRC failed: each group of lengths is checked against its neighbours, and only the items of a group that fails, or raw items with a byte in a bad segment, are suspect (`Payloads::is_suspect`). A damaged lz4 column makes every item suspect.

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
