# ww_framer changelog

## Unreleased

### 🚀 Features

- `Tx::write_full()` / `TxOwned::write_full()`: write a message only if it fits fully into the current frame, never splitting it, for stream media where frame boundaries are not preserved.
- `Head::MAX_HEAD_SIZE` (longest serialized head), `MIN_FRAME_SIZE` is now derived from it by default.
- First version: packs many small messages into one frame and splits big messages across frames, used by the USB and UDP transports. Versioned together with the other `wire_weaver_*` crates (0.5.0).
