---
level: advanced
---

# ww_block

`ww_block` (STD-39) serves a **block device** over WireWeaver: an SD card, eMMC, or a NOR or NAND flash, read,
checked, written and erased in whole blocks by their address (LBA). It is a thin layer over the driver a firmware has
anyway, and it lets a host do almost everything else: read a live card with a PC tool as if it were in a card reader,
lay out a filesystem on the PC and write it out, image a card over the link, or test a card with writes, CRCs and
timings. It is `no_std` by default and depends on `wire_weaver` and `ww_error`.

```toml
[dependencies]
ww_block = "0.1"
```

Raw SD commands and the card's registers are [`ww_sd`](ww_sd.md), served next to it for the same card.
Word-addressed register access, with side-effect flags and watches, is [`ww_memory`](ww_memory.md): a block device has
a different contract (geometry and erase units, a write lease, CRCs, bulk transfers), so the two are separate crates.

```rust
pub const MAX_TRANSFER: usize = 4096;

#[ww_trait]
pub trait BlockDevice {
    fn geometry() -> Result<Geometry<'i>, BlockError<'i>>;
    fn read(lba: u64, count: u16) -> Result<MaxLen<RefVec<'i, u8>, MAX_TRANSFER>, BlockError<'i>>;
    fn crc(lba: u64, count: u32) -> Result<u32, BlockError<'i>>;

    fn claim(lease_s: u16) -> Result<u32, BlockError<'i>>;
    fn release(lease: u32) -> Result<(), BlockError<'i>>;
    fn write(lease: u32, lba: u64, data: MaxLen<RefVec<'i, u8>, MAX_TRANSFER>, crc: u32)
        -> Result<(), BlockError<'i>>;
    fn erase(lease: u32, lba: u64, count: u32) -> Result<(), BlockError<'i>>;

    #[optional(bulk)]
    #[credit]
    sink!(write_runs: WriteRun<'i>);
    #[optional(bulk)]
    #[counted]
    stream!(written: RunAck);
    #[optional(bulk)]
    #[stream(ReadRun<'i>, counted)]
    fn read_runs(lba: u64, count: u64) -> Result<(), BlockError<'i>>;
}
```

## Geometry

`geometry()` answers the medium's `Geometry`, `NoMedium` without a card:

| Field | Meaning |
|---|---|
| `kind` | `SdCard`, `Emmc`, `NorFlash`, `NandFlash`, `Other` |
| `block_size`, `block_count` | bytes per block (512 for SD and eMMC) and blocks on the medium |
| `max_blocks` | most blocks one `read`, `write` or bulk run carries on this device, at most `MAX_TRANSFER` bytes: what fits its message size and buffers |
| `erase_blocks` | blocks in an erase unit: the SD card's allocation unit (AU), a NOR flash's sector |
| `best_write_blocks` | the write size that measured fastest, if measured |
| `erased` | what erased blocks read as: `Zeros`, `Ones` or `Unspecified` |
| `erase_before_write` | NOR flash: a block must be erased before it is written again |
| `read_only` | write-protected: `claim` answers `ReadOnly` |
| `medium_id` | changes when another medium is inserted, so a host notices a swap between calls |
| `name` | product name (the card's CID, a flash's part), may be empty |

`max_blocks` is what a host sizes its calls by. A device with 1 KiB messages takes one 512-byte block per call, one
with 4 KiB messages and buffers eight.

## Reads are free

`read`, `crc` and the bulk `read_runs` work at any time, also while the device writes the card itself (it serialises
the bus). `crc(lba, count)` is the CRC-32 of a range computed on the device, so a host checks what it wrote without
reading it back over a slow link. The CRC is CRC-32/ISO-HDLC, the one of zip and Ethernet; `ww_block::crc32` and
`crc32_update` compute it on either side, and an STM32 CRC unit gives the same with reflected input and output.

## Writes need a lease

A write can destroy what the card's own user (a recorder logging to it) keeps there, so writes are only done under a
**lease**:

1. `claim(lease_s)` stops the card's user first (a recorder syncs and unmounts its log) and returns the lease, a
   `u32`. It may take a while to answer. One lease at a time: `Common(Busy)` while another is held.
2. `write(lease, lba, data, crc)` writes whole blocks, `erase(lease, lba, count)` erases a range (CMD38 on an SD
   card, a sector erase on flash, a discard on eMMC). `write` checks `crc` (the CRC-32 of `data`) before anything is
   written and answers `BadCrc` otherwise; it answers once the blocks are on the medium.
3. `release(lease)` gives the card back, and its user mounts it again and finds what the host wrote.

Every call carrying the lease renews it. After `lease_s` seconds without one, or when the medium is removed, the
lease lapses and the card goes back to its user; a call with it then answers `BadLease`. A host that crashed mid-job
leaves the device working again after `lease_s`.

There is no writing beside a running user (e.g. outside the extent a recorder uses): a PC that wants to change
files on a live card does it through the card's USB mass storage view, if the device has one.

### Who may write

Who may claim the card is the **firmware's choice per link**: for example USB and RTT yes, a remote UDP link no. A
link that may not answers `claim` with `Common(Refused)`, and without a lease nothing else writes. `ww_block` has no
access rules of its own; the firmware's handler of `claim` knows which link a request came on from `cx.medium()`
(see [Methods](../api/methods.md#handler-context)).

## Bulk

The `bulk` group is for long jobs and slow links: writing a prepared filesystem, imaging a card. A device without it
leaves it out with `omit = ["card.bulk"]` (the path from your API root, see
[Optional items](../api/optional_items.md)), and a host calling it gets `Unimplemented`.

- `write_runs` is a [credit sink](../api/credit_sinks.md) of `WriteRun { lease, lba, crc, data }`: the host never
  sends more than the device has room for. Each run is written as `write` would, and its outcome comes on `written`
  as `RunAck { lba, count, status }`, in the order the runs came. `RunStatus` is `Written` or the reason it was not
  (`BadLength`, `OutOfRange`, `BadLease`, `BadCrc`, `WriteFailed`, `NoMedium`). `written` is a counted stream, unit:
  run, so a host learns about lost acknowledgements and checks those ranges with `crc`.
- `read_runs(lba, count)` opens a counted stream of `ReadRun { lba, data }`, unit: block: the whole range at the
  link's full speed instead of one call per run. Opening it again starts another range.

A host preparing a card on the PC does, for example: `geometry()`, lay out the filesystem, `claim`, the sectors over
`write_runs`, a `crc` per region, `release`. The device needs no format code for that.

## Errors

`BlockError` nests `CommonError` first (see [ww_error](ww_error.md)), then its own variants; each call lists those it
returns (`#[errors]`):

| Variant | When |
|---|---|
| `NoMedium` | no card in the slot, or the medium doesn't answer |
| `OutOfRange` | a block past the last one |
| `BadLease` | the lease lapsed, was released, or is not the current one |
| `BadCrc` | `write`'s CRC doesn't match the data: nothing was written |
| `ReadOnly` | the medium is write-protected |
| `ReadFailed { reason }`, `WriteFailed { reason }` | the medium reported an error |
| `Common(Busy)` | another lease is held (`claim`), or the bus is taken |
| `Common(Refused)` | this link may not write the card (`claim`) |
| `Common(InvalidArgument)` | more than `max_blocks`, or `data` that isn't whole blocks |

## Serving it

`ww api scaffold <crate>` prints the server struct (the `SinkCredit` of `write_runs` and the `Counted` states of the
streams included) and every handler stub. `tests/block_sd` serves a mock card as a block device and an SD card, and a
small read-only flash that leaves `bulk` out.
