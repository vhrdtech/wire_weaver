# ww_tape changelog

## Unreleased

### ⚠️ Breaking

- Describe sets mark their first and last block (`FLAG_SET_FIRST`, `FLAG_SET_LAST`); only a whole set counts as one (STD-129).
- `PieceChain::create` takes the chain number: each chain of a result gets its own piece ids and run tag (STD-129).
- `PieceId::first` is `first_of(result, 0)`: its bytes change; `PieceId::first_of` names the first piece of each chain (STD-129).
- `Block::records` of an lz4-coded body gives `BlockError::Coded`: read the records of `Block::decode` (STD-129).
- Header bytes 24..28 hold the run tag in every block, 28..32 the head CRC; chunk payload and index entries offsets move to 32..36 (STD-129).
- `PieceHead` gains `prev_run`: the run before the piece head on the medium, piece head bytes 92..96 (STD-129).
- Within a run a describe block is in seq order unless marked a copy; `FileError` gains `Replaced` (STD-129).
- `OpenOptions` gains `medium`, `start` and `layout`; `PieceInfo` gains `gaps` and `replaced` (STD-129).
- `Layout` gains `gap_max` and `resume_align`, its fields are `Bytes<u32>`; `FileError` gains `Gap` and `Stale`, `Cut` gains `Stale` (STD-129).
- Block makers take a `BlockId` (run and seq) instead of a seq; `set_seq` is `set_id`; `DescribeSet::write_blocks` counts a `&mut BlockId` (STD-129).
- Series summary blocks state their scale (head bytes 52..54, `Magnitude` after the head); `SeriesStart` takes `segment` instead of `first_tick` (STD-128).
- `summary_len` returns a `Result`; `StatColumn::value` is `get`, None past the end; `SeriesSummaries::pending` returns `SeriesNode`s (STD-128).

- `TrackDef` gains `kind: TrackKind` (events with `Ends` and flags, or a series); set `TrackKind::Events(EventsDef::INSTANTS)` for plain records (STD-127).
- `DescribeSet::insert` refuses a series track that isn't sticky or names an item type: `DescribeError::Kind` (STD-127).
- `TapeWriter::finish` returns `Finished` and ends the file with a final index, describe backup, finish and tail block (STD-122).
- `EndMode::NextStart` chunks have no ends column: the last item ends at the head's latest end (STD-120).
- Header bytes 24..32 hold a chunk block's payload offset and head CRC; `BlockError::Chunk` for a bad chunk head (STD-118).
- `BlockError` gains `Summary` for a bad summary block head or body (STD-121).
- `WriteError` gains `Block` and `Chunk`; `Describe` gains `Layout`, which takes variant 6 (STD-118).

- Format version 2 (STD-117): record times are signed i64 ticks of the track's clock (`Record::at: Option<ClockTime>`), deltas coded per clock after a clock table in the body; header bytes 24..32 are reserved, `Block::t_base` is gone.
- `BlockWriter::new` takes the block's clocks instead of a time base; `push` / `push_with` take a `ClockTime`, describe records go through `push_describe` / `push_describe_with`.
- `BlockKind::Data` is `BlockKind::Records`; `Producer` has only its name: `wall_ns_at_zero` is a wall clock and a `ClockMap` (STD-117).
- `TrackDef` names its `clock`, carries `dimensions` (part of its identity, `track_id`) and an optional item type `item` (STD-117).

### 🐛 Fixes

- `TapeReader` reads the stretch after a checkpoint with the piece's gap rules, as `FileBlocks` does (STD-129).
- `TapeWriter` copies only a whole describe set, never part of one still being written (STD-129).
- A replaced block leaves the index as it leaves a walk, in `TapeWriter`'s index and in scans (STD-129).
- On a raw medium blank space longer than the gap ends the walk; the rest of the medium isn't read (STD-129).
- Gap probing is bounded: `Layout::is_valid`, `GAP_MAX_MAX`, `GAP_PROBES_MAX`, `GAP_CHECKS_MAX`; probes read header by header (STD-129).
- A duplicate of the last block after a gap resumes the data instead of cutting it (STD-129).

### 🚀 Features

- `TapeWriter::adopt_block`: a producer's block in the writer's run, seq gaps kept; a raw-medium piece is one run (STD-129).
- Reserved, refused until built: `Codec::SERIES` (49), `Codec::RUNS` (36), `Element::LEAVES` (13), `ColumnKind::PAYLOAD_LEAF` (7), summary head bytes 54..56 (STD-129).
- `TapeWriter` copies the describe set after a piece's first data block; `TapeReader::describe_set` takes the next good copy (STD-129).
- `EventRecord`: an events track's record item, duration and flags as its `EventsDef` says, then the payload (STD-129).
- `Codec::OUTER`, `is_outer`, `inner`: bit 7 of a column's codec byte marks an outer-coded column, refused until a codec exists (STD-129).
- Header flag `FLAG_BODY_CODED`: records and describe bodies coded stored or lz4 (`code_body`, `BodyCodec`, `Block::decode`), off by default (STD-129).
- The same (run, seq) twice is a duplicate, not a cut: same bytes dropped, other bytes the later wins (STD-129).
- Header flag `FLAG_COPY` marks describe copies a writer makes; `Block::flags`, `is_copy`, `crc`; `mark_copy` (STD-129).
- `TapeReader` keeps to runs and reads past gaps (`PieceInfo::gaps`); `OpenOptions::medium` walks a raw medium forward from `start` (STD-129).
- `FileBlocks` keeps to one run: stale blocks of another run end the data; gaps within `Layout::gap_max` are read past (STD-129).
- `Medium` (`File`, `Raw`), `FileBlocks::on`, `starting_at`, `with_layout`; `Layout::on_sd_card`, `on_raw_nand` (STD-129).
- Blank is 0x00 or 0xFF: `is_blank`; `FileBlocks` and `TapeReader` take either after the last block as the end (STD-129).
- `RunTag` (random, from a seed, of a piece id) and `Block::run`; `TapeWriter::next_id`, `run`, `set_run`; a chain's run is derived (STD-129).
- `Codec::ALP` (26): lossless floats as decimals stored as bit-packed integers, or front bits through a dictionary; `samples::ALP_GROUP` (STD-130).
- Series summaries across prefix changes: nodes at their common scale, exact min and max; `SeriesSummaries::segment`, `SeriesNode::merge` (STD-128).
- `SeriesDef`: sample type by `TypeRef`, element and channels, exact period, scale, summaries, media stream; `SeriesValue` derives them from `Fixed` (STD-127).
- `SeriesSegment` records (first index, time grid, scale) and `SeriesTimeline`: ticks, scales and chunk checks, damaged records refused (STD-127).
- Golden file `golden/v2-series.hex`; fuzz target `tape-series` (STD-127).
- Summary blocks (`BlockKind::Summary`, kind 6): event item tree (`EventTree`) and mergeable series summaries (`SeriesSummaries`), read in place (STD-121).
- `SeriesSummarySpec`, `SeriesStats`, `ClipRange`: a series track's summary declaration as shrink_wrap types (STD-121).
- Index blocks (kind 7): checkpoints with a back chain every 4 MiB (`IndexPolicy`), a final index sorted by track (STD-122).
- Finish (kind 8) with BLAKE3 content hash and per-track ranges, fixed-size tail (kind 9) found from the file's end (STD-122).
- Tape pieces: piece head (kind 10), `TapeWriter::cut`, `PieceChain` cutting by size, chunks or time, `check_chain` (STD-123).
- `TapeReader` over a `ByteSource` (slice, `Vec`, `File`): pieces from tails, checkpoints or a bounded scan; builds for wasm32 (STD-124).
- The writer repeats the newest describe blocks about every 128 MiB and in each piece's head and backup (STD-123).
- Index, finish, tail, piece head and repeated describe blocks carry the last counted seq and stay out of seq order (STD-122).
- `TapeWriter::defer_block` refuses structural blocks; `set_seq` rewrites an index block's head CRC (STD-122).
- Golden file `golden/v2-index.hex`; fuzz target `tape-index` (STD-122, STD-124).
- `times`: event time codec: implicit or linear frame of reference per 128 starts, three end modes, packed flags with group OR (STD-120).
- `EventTimes` reads a chunk's times in place: get, lower bound, last at or before, iteration both ways, group decode (STD-120).
- `EventTimes::salvage`: a damaged chunk's group records checked one by one, only a bad group's items suspect (STD-120).
- `Element` and `SampleShape`: what a series column holds, element codes 0 to 12 (STD-119).
- `Samples` reads a sample column in place and decodes any range; `samples::encode`, `max_encoded_len` (STD-119).
- Raw device sample codecs: `Codec::BIG_ENDIAN`, `PACKED10/12/24_LSB/MSB`, converted on read (STD-119).
- Lossless float codecs `Codec::GORILLA` and `CHIMP`, groups of 1024 samples that decode alone (STD-119).
- Delta codecs `Codec::DELTA`, `DELTA2`, `FLOAT_DELTA`, `FLOAT_DELTA2`: bit-packed per group of 128 (STD-119).
- `Codec::LZ4` payload columns through `lz4_flex` (`ww_tape::lz4`) (STD-119).
- Golden file `golden/v2-samples.hex` and fuzz target `tape-samples` (STD-119).
- `bitpack`: groups of 128 values at one width, read in place; AVX-512, AVX2 and NEON unpack kernels (STD-120).
- `payloads`: an events chunk's payload lengths, bit-packed per group, and payloads, raw or lz4 when smaller (STD-126).
- `Payloads::salvage`: a damaged chunk's payloads read item by item, only damaged ones marked suspect (STD-126).
- `Payloads::iter`: every item's payload and suspicion in order, each group's lengths unpacked once (STD-126).
- Golden file `golden/v2-payloads.hex` and fuzz target `tape-payloads` (STD-126).
- `Lz4Error` implements `Display` and `core::error::Error`; payload errors name it as their source (STD-126).
- Chunk blocks (`BlockKind::Events`, `Series`): self-locating `ChunkHead` (track, first item, tick range, latest end), typed columns with codec ids, read in place (STD-118).
- A CRC-32C per 64 KiB payload segment; `Block::salvage` and `Recovery::Resync` hand out damaged chunks with bad segments marked (STD-118).
- `Describe::Layout` and `TapeWriter::set_alignment`: chunk payloads on 4 KiB boundaries, deferred blocks filling the gaps (STD-118).
- `TypeRef` and `TypedItem` (and their owned forms) derive `Eq` and `Hash` (P2401#15nov-1358).
- `set_seq` renumbers a block; `BlockKind::Parity` reserved, not written (STD-118).
- `Describe::Clock` (`ClockDef`: number, key, exact rate and resolution as `Magnitude`, `ClockOrigin`, label), `clock_key`, `UNIX_CLOCK_KEY` (STD-117).
- `Describe::Map`: `ClockMap` of `MapSegment`s, slope an exact_units `Ratio64`, error bound; `MapSegment::map` takes a `Rounding` (STD-117).
- `Describe::Item`: any wire_weaver value by `TypeRef` (bundle hash, crate, type path) (STD-117).
- `DescribeSet`: describe records in canonical order (`Describe::rank`), written as describe blocks with sticky records; same input, same bytes (STD-117).
- `DescribeSet` refuses a clock resolution under one tick and a sticky record off its track's clock (STD-117).
- `ClockDef.rate` is a `ClockRate` (`MagnitudeOf<Per<Tick, Second>>`), `resolution` a `ClockResolution` (`MagnitudeOf<Second>`): same bytes (STD-117).
- `ClockOrigin::Epoch`: a counter's next epoch, linked by an exact offset map to the previous epoch's clock (STD-117).
- A `ClockMap` must increase strictly across its segments, checked on decode and by `DescribeSet` (STD-117).
- Golden file `golden/v2.hex` and a test that two writes of the same input are byte-identical (STD-117).

### 🐛 Fixes

- The format text says what describe copies (repeats, backup, next piece) hold: sticky state as of the run they copy (STD-122).

### 🚀 Features

- First version (STD-42): tape blocks, the generic unit of records for live feeds and log files: a 32-byte header (magic `WWTP`, version, kind, length, record count, block seq, time base), records (`uvar` track, `ivar` time delta, `uvar` length, item bytes) and a CRC-32C, variable length and never padded. `BlockWriter` (`push`, `push_with` to encode an item in place, `finish`), `Block::read` / `Block::records` reading in place, `block_len`; describe records on track 0 (`Describe`: `Producer`, `TrackDef` with stable key, API, bundle hash, resource path, label and `sticky`, `Bundle`), `api_root_id` and `track_key` as in logring; with `std`, `FileBlocks` reads blocks appended back to back from a file or stream, skipping a bad block to the next magic and reporting a cut last one (`FileBlocks::next_good` skips them, handing each to a callback), `BlockBuf` an owned checked block. Extensible: the 32-byte header and the CRC trailer are the same in every version, so a block of a later version (`Block::version`) or of a kind a reader doesn't know (`BlockKind::Other`) is checked and given out without records (`Block::body_known` false) instead of being taken for damage; `Describe` is evolvable, a variant a reader doesn't know reads as `Describe::Unknown`.
- `TapeWriter` (`create`, `append`, `write_block`, `tick`, `sync`, `finish`): blocks into a file with database-style durability: a new file as `NAME.partial` renamed at `finish`, file and directory synced after create and rename, data synced by a `SyncPolicy` (`None`, `EveryBlocks(n)`, `Interval(t)`, default 1 s; `File::sync_data`: `fdatasync` / `F_FULLFSYNC` / `FlushFileBuffers`), a failed write or sync fatal for the file (`WriteError::Failed` afterwards, never retried).
- `FileBlocks` reads the longest valid prefix by default (`Recovery::Prefix`): it stops at the first block with a bad magic, length or CRC, a cut one or a data block whose seq goes back, reporting `FileError::Cut`; zeros at the end are the end. `Recovery::Resync` (`FileBlocks::with_recovery`) skips damage to the next magic, also past a damaged length. A block is at most `MAX_BLOCK_LEN` (64 MiB) and the reader's buffer grows only with the bytes that come, so a damaged length doesn't allocate what it says. Tests cut a capture at every byte and flip every byte; fuzz target `tape-reader`.
