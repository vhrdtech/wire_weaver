## Unreleased

### 🚀 Features

- New crate (STD-39): `BlockDevice`, a block device (SD card, eMMC, NOR or NAND flash) read, checked, written and
  erased by block address over WireWeaver.
  - `geometry` (`Geometry`: `MediumKind`, block size and count, `max_blocks` per call, erase unit, best write size,
    `Erased`, `erase_before_write`, `read_only`, `medium_id`, name), free `read` (up to `MAX_TRANSFER` = 4096 bytes)
    and `crc` (CRC-32/ISO-HDLC computed on the device; `crc32` and `crc32_update` for either side).
  - Writes only under a lease: `claim(lease_s)` stops the card's own user and returns it, `write` (with the data's
    CRC, checked before writing) and `erase` carry it and renew it, `release` gives the card back. Who may claim is
    the firmware's choice per link (`Common(Refused)`), no access rules in the crate.
  - `#[optional(bulk)]`: the `write_runs` credit sink of `WriteRun`, acknowledged per run on the counted `written`
    stream (`RunAck`, `RunStatus`), and the counted stream function `read_runs` of `ReadRun`.
  - `BlockError` with the `Common` variant and `NoMedium`, `OutOfRange`, `BadLease`, `BadCrc`, `ReadOnly`,
    `ReadFailed`, `WriteFailed`.
