# ww_logbook changelog

## Unreleased

### 🚀 Features

- First version (STD-42): logbook blocks, the generic unit of records for live feeds and log files: a 32-byte header (magic `WWLB`, version, kind, length, record count, block seq, time base), records (`uvar` track, `ivar` time delta, `uvar` length, item bytes) and a CRC-32C, variable length and never padded. `BlockWriter` (`push`, `push_with` to encode an item in place, `finish`), `Block::read` / `Block::records` reading in place, `block_len`; describe records on track 0 (`Describe`: `Producer`, `TrackDef` with stable key, API, bundle hash, resource path, label and `sticky`, `Bundle`), `api_root_id` and `track_key` as in logring; with `std`, `FileBlocks` reads blocks appended back to back from a file or stream, skipping a bad block to the next magic and reporting a cut last one (`FileBlocks::next_good` skips them, handing each to a callback), `BlockBuf` an owned checked block.
