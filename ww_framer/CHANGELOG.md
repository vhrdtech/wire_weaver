# ww_framer changelog

## Unreleased

### ⚡ Performance

- `U2Head` writes and reads whole bytes at a byte boundary (every head at the start of a frame or after a message) instead of going through the bit writer and reader, which took ~15% of the CPU per USB packet on a Cortex-M0+; same bytes on the wire (TR-12).

### 🚀 Features

- `pieces::<H, C, T>(frame)` (`Pieces`, `Piece`, `PieceError`, `verify_split`): walks one frame into its pieces (kind, user kind, length field, head, payload and trailer ranges, Full checksum and tail checked) without reassembling, for the link tap's decoder (TR-14). `FramedRx` reads heads and piece extents with the same code, so the two can't drift; on a Cortex-M3 at `opt-level = 3` receiving 59 USB packets of 60 messages takes 1.1% fewer instructions (75202 vs 76039, `examples_mcu/mcu_qemu` `qemu-framer-rx`), a G0 USB firmware at `opt-level = "z"` is 24 B larger.
- `Tx::write_full()` / `TxOwned::write_full()`: write a message only if it fits fully into the current frame, never splitting it, for stream media where frame boundaries are not preserved.
- `Head::MAX_HEAD_SIZE` (longest serialized head), `MIN_FRAME_SIZE` is now derived from it by default.
- First version: packs many small messages into one frame and splits big messages across frames, used by the USB and UDP transports. Versioned together with the other `wire_weaver_*` crates (0.5.0).

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.
