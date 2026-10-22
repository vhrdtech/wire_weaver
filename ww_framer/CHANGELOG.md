# ww_framer changelog

## Unreleased

### ⚠️ Breaking

- A checksum now covers the frame prefix and the head of its piece (the `Full`, or the `End` of a split message) besides the message, so that a flipped bit in a user kind or a length fails it too (found by the `framer-lossy` fuzz target: a corrupted user kind was delivered under the wrong kind). `Checksum::write` / `read` take a `header: &[u8]` before the message; implementations add the parameter (and may hash it). Wire change: `CrcChecksum` values differ from before for the same message (TR-15).
- `CrcAlgorithm` is a function now, `fn checksum(header, message) -> Width`, instead of a `const CRC: crc::Crc<Width>`, so a CRC can come from elsewhere (a hardware unit, `ww_block::crc32_update`); `crc::crc_of(&CRC, header, message)` keeps a `crc` crate algorithm a few lines. `CrcWidth::checksum` is replaced by `checksum2` (TR-15).
- `pieces::verify_split` takes the End's frame prefix and head: `verify_split::<C>(prefix, end_head, message, trailer)`; `PieceErrorKind::TooShort` is new (TR-15).
- `FramedRx::stage` / `commit_staged` (and `FramedRxOwned::stage`) return `Result<FrameReport, ()>` instead of `Result<(), ()>`: callers that only used `?`, `unwrap` or `is_err` don't change (TR-15).
- `FramePrefix::Rx` must be `Send`; in return `Tx`, `FramedRx`, `TxOwned`, `FramedRxOwned`, `Pieces` and `CrcChecksum` are `Send` and `Sync` whatever their head, checksum, tail and prefix types are (they only mark them), so a framer generic over a configuration moves between threads (TR-15).

### ⚡ Performance

- `U2Head` writes and reads whole bytes at a byte boundary (every head at the start of a frame or after a message) instead of going through the bit writer and reader, which took ~15% of the CPU per USB packet on a Cortex-M0+; same bytes on the wire (TR-12).

### 🚀 Features

- Taking over a framer mid-stream (CLT-44): `Tx::continue_at(frame)` / `TxOwned::continue_at` go on with another sender's frame counter, `FramedRx::carry()` / `FramedRxOwned::carry()` give what a receiver holds between frames (`Carry`: the split message assembled so far, staged bytes, the frame expected next) and `continue_from(&carry)` takes it into another one, so a counting prefix sees no gap and a message split across the takeover comes out whole. `FramePrefix::expected` / `continue_at`, with defaults (nothing to carry) for prefixes without a counter.
- Frame prefix (TR-15): `prefix::FramePrefix`, a fourth generic parameter of `Tx`, `FramedRx`, `TxOwned`, `FramedRxOwned` and `Pieces` (default `NopPrefix`, nothing, as before). `Tx` writes it once at the start of every frame from a `u32` frame counter and a session epoch (`Tx::new_session()`, `frame_ctx()`), `FramedRx` checks it when a frame is staged and reports a `FrameReport` per frame. `SeqPrefix` is one byte (4-bit counter, 2-bit epoch, 2 spare bits): its `SeqRx` rebuilds the full counter and tells in-order frames from gaps, duplicates (bitmap of the last 16 frames), late and stale-epoch ones, which are dropped, and from a new session; a gap or a new session drops a split message in progress. `FramedRx::resync()`, `prefix_stats()` (`PrefixStats`), `pieces_with_prefix()`. With `NopPrefix` nothing is added to `Tx` or `FramedRx` but the 5-byte counter and epoch in `Tx`.
- `defmt` feature: `defmt::Format` for the prefix types.
- Fuzz target `framer-lossy` (`just fuzz-framer-lossy`): `SeqPrefix` and a CRC-32 on every message over a channel that loses, duplicates, delays and corrupts frames, with new sessions; delivered messages must be sent ones, in order unless a frame was corrupted.
- `pieces::<H, C, T>(frame)` (`Pieces`, `Piece`, `PieceError`, `verify_split`): walks one frame into its pieces (kind, user kind, length field, head, payload and trailer ranges, Full checksum and tail checked) without reassembling, for the link tap's decoder (TR-14). `FramedRx` reads heads and piece extents with the same code, so the two can't drift; on a Cortex-M3 at `opt-level = 3` receiving 59 USB packets of 60 messages takes 1.1% fewer instructions (75202 vs 76039, `examples_mcu/mcu_qemu` `qemu-framer-rx`), a G0 USB firmware at `opt-level = "z"` is 24 B larger.
- `Tx::write_full()` / `TxOwned::write_full()`: write a message only if it fits fully into the current frame, never splitting it, for stream media where frame boundaries are not preserved.
- `Head::MAX_HEAD_SIZE` (longest serialized head), `MIN_FRAME_SIZE` is now derived from it by default.
- First version: packs many small messages into one frame and splits big messages across frames, used by the USB and UDP transports. Versioned together with the other `wire_weaver_*` crates (0.5.0).

### 📚 Documentation

- `ww_framer.md`: frame prefix section, what a checksum covers; `ww_framer_wire.md`: prefix bytes, the UDP configuration's bytes with a CRC-32 on every message, CRC values updated for the covered head.
- Docs: product names written WireWeaver and ShrinkWrap in prose.
