## Unreleased

### 🚀 Features

- `crc32`/`crc32_update` (STD-39) are table-driven on the [crc](https://docs.rs/crc) crate (already used by
  `ww_framer`), table size picked by cargo feature: none by default (bitwise, 0 B, today's behaviour),
  `crc-table-256` (1 KiB) or `crc-table-slice16` (16 KiB, fastest); enabling both picks `crc-table-slice16` instead
  of failing the build, so `--all-features` builds still work. Same signatures, bit-identical CRC-32/ISO-HDLC
  results at every table size, checked in `ww_block`'s own tests against a fixed reference table at lengths and
  splits on and off a slice-by-16 boundary. `docs/std_library/ww_block.md` has a size/speed table, including the
  `crc-table-256` throughput measured on an RP2350 (rePort 027): 1.36 MB/s to 2.6-3.0 MB/s.

- New crate (STD-39): `BlockDevice`, a block device (SD card, eMMC, NOR or NAND flash) read, checked, written and
  erased by block address over WireWeaver.
  - `geometry` (`Geometry`: `MediumKind`, block size (`Bytes<u32>`) and count (`Count<Block, u64>`), `max_blocks` per
    call, erase unit, best write size, `Erased`, `erase_before_write`, `read_only`, `medium_id` (a `MediumId`
    newtype, `0` when the device can't tell), name), free `read` (up to `MAX_TRANSFER` = 4096 bytes) and `crc`
    (CRC-32/ISO-HDLC computed on the device; `crc32` and `crc32_update` for either side). `lba` everywhere is an
    `Index<Block, u64>`, block counts a `Count<Block, T>`.
  - Writes only under a lease: `claim(lease)` (the requested duration, a `Secs<u16>`) stops the card's own user and
    returns a `Lease` (a newtype of `u32`, `ww_sd` depends on it too), `write` (with the data's CRC, checked before
    writing) and `erase` carry it and renew it, `release` gives the card back. Who may claim is the firmware's
    choice per link (`Common(Refused)`), no access rules in the crate.
  - `#[optional(bulk)]`: the `write_runs` credit sink of `WriteRun`, acknowledged per run on the counted `written`
    stream (`RunAck`, `RunStatus`), and the counted stream function `read_runs` of `ReadRun`.
  - `BlockError` with the `Common` variant and `NoMedium`, `OutOfRange`, `BadLease`, `BadCrc`, `ReadOnly`,
    `ReadFailed`, `WriteFailed`.
  - Every number that means a block position, a block count, a size or a lease duration is typed (`ww_si`'s
    `Index<Block, _>`, `Count<Block, _>`, `Bytes`, `Secs`) rather than a bare integer (P2401#11nov-0947b): the same
    bytes on the wire. The crate depends on `ww_si` (`time`, `information`, `counting`).
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.
