# ww_tape changelog

## Unreleased

### 🚀 Features

- First version (STD-42): tape blocks, the generic unit of records for live feeds and log files: a 32-byte header (magic `WWTP`, version, kind, length, record count, block seq, time base), records (`uvar` track, `ivar` time delta, `uvar` length, item bytes) and a CRC-32C, variable length and never padded. `BlockWriter` (`push`, `push_with` to encode an item in place, `finish`), `Block::read` / `Block::records` reading in place, `block_len`; describe records on track 0 (`Describe`: `Producer`, `TrackDef` with stable key, API, bundle hash, resource path, label and `sticky`, `Bundle`), `api_root_id` and `track_key` as in logring; with `std`, `FileBlocks` reads blocks appended back to back from a file or stream, skipping a bad block to the next magic and reporting a cut last one (`FileBlocks::next_good` skips them, handing each to a callback), `BlockBuf` an owned checked block. Extensible: the 32-byte header and the CRC trailer are the same in every version, so a block of a later version (`Block::version`) or of a kind a reader doesn't know (`BlockKind::Other`) is checked and given out without records (`Block::body_known` false) instead of being taken for damage; `Describe` is evolvable, a variant a reader doesn't know reads as `Describe::Unknown`.
