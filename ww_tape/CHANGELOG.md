# ww_tape changelog

## Unreleased

### ⚠️ Breaking

- Format version 2 (STD-117): record times are signed i64 ticks of the track's clock (`Record::at: Option<ClockTime>`), deltas coded per clock after a clock table in the body; header bytes 24..32 are reserved, `Block::t_base` is gone.
- `BlockWriter::new` takes the block's clocks instead of a time base; `push` / `push_with` take a `ClockTime`, describe records go through `push_describe` / `push_describe_with`.
- `BlockKind::Data` is `BlockKind::Records`; `Producer` has only its name: `wall_ns_at_zero` is a wall clock and a `ClockMap` (STD-117).
- `TrackDef` names its `clock`, carries `dimensions` (part of its identity, `track_id`) and an optional item type `item` (STD-117).

### 🚀 Features

- `Describe::Clock` (`ClockDef`: number, key, exact rate and resolution as `Magnitude`, `ClockOrigin`, label), `clock_key`, `UNIX_CLOCK_KEY` (STD-117).
- `Describe::Map`: `ClockMap` of `MapSegment`s, slope an exact_units `Ratio64`, error bound; `MapSegment::map` takes a `Rounding` (STD-117).
- `Describe::Item`: any wire_weaver value by `TypeRef` (bundle hash, crate, type path) (STD-117).
- `DescribeSet`: describe records in canonical order (`Describe::rank`), written as describe blocks with sticky records; same input, same bytes (STD-117).
- `DescribeSet` refuses a clock resolution under one tick and a sticky record off its track's clock (STD-117).
- `ClockOrigin::Epoch`: a counter's next epoch, needing an exact offset map from the one before (STD-117).
- A `ClockMap` must increase strictly across its segments, checked on decode and by `DescribeSet` (STD-117).
- Golden file `golden/v2.hex` and a test that two writes of the same input are byte-identical (STD-117).

### 🚀 Features

- First version (STD-42): tape blocks, the generic unit of records for live feeds and log files: a 32-byte header (magic `WWTP`, version, kind, length, record count, block seq, time base), records (`uvar` track, `ivar` time delta, `uvar` length, item bytes) and a CRC-32C, variable length and never padded. `BlockWriter` (`push`, `push_with` to encode an item in place, `finish`), `Block::read` / `Block::records` reading in place, `block_len`; describe records on track 0 (`Describe`: `Producer`, `TrackDef` with stable key, API, bundle hash, resource path, label and `sticky`, `Bundle`), `api_root_id` and `track_key` as in logring; with `std`, `FileBlocks` reads blocks appended back to back from a file or stream, skipping a bad block to the next magic and reporting a cut last one (`FileBlocks::next_good` skips them, handing each to a callback), `BlockBuf` an owned checked block. Extensible: the 32-byte header and the CRC trailer are the same in every version, so a block of a later version (`Block::version`) or of a kind a reader doesn't know (`BlockKind::Other`) is checked and given out without records (`Block::body_known` false) instead of being taken for damage; `Describe` is evolvable, a variant a reader doesn't know reads as `Describe::Unknown`.
- `TapeWriter` (`create`, `append`, `write_block`, `tick`, `sync`, `finish`): blocks into a file with database-style durability: a new file as `NAME.partial` renamed at `finish`, file and directory synced after create and rename, data synced by a `SyncPolicy` (`None`, `EveryBlocks(n)`, `Interval(t)`, default 1 s; `File::sync_data`: `fdatasync` / `F_FULLFSYNC` / `FlushFileBuffers`), a failed write or sync fatal for the file (`WriteError::Failed` afterwards, never retried).
- `FileBlocks` reads the longest valid prefix by default (`Recovery::Prefix`): it stops at the first block with a bad magic, length or CRC, a cut one or a data block whose seq goes back, reporting `FileError::Cut`; zeros at the end are the end. `Recovery::Resync` (`FileBlocks::with_recovery`) skips damage to the next magic, also past a damaged length. A block is at most `MAX_BLOCK_LEN` (64 MiB) and the reader's buffer grows only with the bytes that come, so a damaged length doesn't allocate what it says. Tests cut a capture at every byte and flip every byte; fuzz target `tape-reader`.
