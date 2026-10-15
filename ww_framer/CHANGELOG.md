# ww_framer changelog

## Unreleased

### ⚡ Performance

- `U2Head` writes and reads whole bytes at a byte boundary (every head at the start of a frame or after a message) instead of going through the bit writer and reader, which took ~15% of the CPU per USB packet on a Cortex-M0+; same bytes on the wire (TR-12).

### 🚀 Features

- `Tx::write_full()` / `TxOwned::write_full()`: write a message only if it fits fully into the current frame, never splitting it, for stream media where frame boundaries are not preserved.
- `Head::MAX_HEAD_SIZE` (longest serialized head), `MIN_FRAME_SIZE` is now derived from it by default.
- First version: packs many small messages into one frame and splits big messages across frames, used by the USB and UDP transports. Versioned together with the other `wire_weaver_*` crates (0.5.0).
