#![cfg_attr(not(feature = "std"), no_std)]
//! A block device over WireWeaver (STD-39): an SD card, eMMC or flash read, checked, written and erased in whole
//! blocks by their address (LBA).
//!
//! [BlockDevice] is a thin layer over the driver a firmware has anyway, and it lets a host do almost everything
//! else: read a live card with a PC tool as if it were in a card reader, lay out a filesystem on the PC and write
//! it out, image a card over the link, or test a card with writes, CRCs and timings.
//!
//! - **Reads are free**: [BlockDevice::read], [BlockDevice::crc] and the bulk [BlockDevice::read_runs] work at any
//!   time, also while the device writes the card itself (it serialises the bus).
//! - **Writes need a lease**: [BlockDevice::claim] stops the card's own user first (a recorder syncs and unmounts
//!   its log), and every write and erase carries the lease it returned. One lease at a time, it lapses after
//!   `lease_s` seconds without a call, [BlockDevice::release] gives the card back.
//! - **Bulk** (`#[optional(bulk)]`): runs of blocks on the [BlockDevice::write_runs] credit sink, acknowledged on
//!   [BlockDevice::written], and a whole range read on the [BlockDevice::read_runs] stream, for slow links and long
//!   jobs. A device without it leaves the group out (`omit = ["card.bulk"]`).
//!
//! Who may claim the card and write it is the firmware's choice per link (e.g. USB and RTT yes, a remote UDP link
//! no): a link that may not answers [BlockDevice::claim] with `Common(Refused)`. This crate has no access rules of
//! its own.
//!
//! Register-level access to memory or a chip is `ww_memory`; raw SD commands and registers are `ww_sd`.
//!
//! Serve it with `ww api scaffold`, which prints the handler stubs for the `ww_codegen!` call you use.

use wire_weaver::prelude::*;
use ww_error::CommonError;

/// Most bytes one [BlockDevice::read], [BlockDevice::write] or bulk run carries: 8 blocks of 512 bytes, or one
/// 4 KiB flash sector. A device with smaller messages or buffers takes less, see [Geometry::max_blocks].
pub const MAX_TRANSFER: usize = 4096;

/// Longest name in [Geometry::name], in bytes.
pub const MAX_NAME: usize = 32;

/// A block device: an SD card, eMMC, or a NOR or NAND flash, addressed in blocks.
///
/// # Errors
///
/// [BlockError]: the `Common` variants and the block device's own, listed on each call.
#[ww_trait]
pub trait BlockDevice {
    /// Block size and count, blocks per call, erase unit, what erased blocks read as, read-only flag. `NoMedium`
    /// without a card.
    #[errors(NoMedium)]
    fn geometry() -> Result<Geometry<'i>, BlockError<'i>>;

    /// Reads `count` blocks from `lba`, at most [Geometry::max_blocks]. Allowed at any time, also while the device
    /// writes the card itself.
    ///
    /// `OutOfRange` past the last block, `Common(InvalidArgument)` for more than [Geometry::max_blocks].
    #[errors(Common(Busy, InvalidArgument), NoMedium, OutOfRange, ReadFailed)]
    fn read(lba: u64, count: u16) -> Result<MaxLen<RefVec<'i, u8>, MAX_TRANSFER>, BlockError<'i>>;

    /// CRC-32 (see [crc32]) of `count` blocks from `lba`, computed on the device: the host checks what it wrote
    /// without reading it back over a slow link. Takes as long as reading the blocks.
    #[errors(Common(Busy), NoMedium, OutOfRange, ReadFailed)]
    fn crc(lba: u64, count: u32) -> Result<u32, BlockError<'i>>;

    /// Takes the card for writing and returns a lease. Stops the card's own user first (a recorder syncs and
    /// unmounts its log), so it may take a while to answer. Every write and erase carries the lease, and each such
    /// call renews it; after `lease_s` seconds without one, or when the medium is removed, it lapses and the card
    /// goes back to its user. One lease at a time.
    ///
    /// `Common(Busy)` while another lease is held, `Common(Refused)` when this link may not write the card (the
    /// firmware's choice), `ReadOnly` for a read-only medium.
    #[errors(Common(Busy, Refused, InvalidArgument), NoMedium, ReadOnly)]
    fn claim(lease_s: u16) -> Result<u32, BlockError<'i>>;

    /// Gives the card back: its user mounts it again (and finds what the host wrote). `BadLease` for a lease that
    /// lapsed or is not the current one.
    #[errors(BadLease)]
    fn release(lease: u32) -> Result<(), BlockError<'i>>;

    /// Writes whole blocks at `lba`, at most [Geometry::max_blocks]. `crc` is the CRC-32 ([crc32]) of `data`, checked
    /// before anything is written. Answers when the blocks are on the medium.
    ///
    /// `Common(InvalidArgument)` when `data` is not whole blocks or more than [Geometry::max_blocks], `BadCrc` when `crc` doesn't match (nothing is
    /// written).
    #[errors(
        Common(InvalidArgument),
        NoMedium,
        OutOfRange,
        BadLease,
        BadCrc,
        WriteFailed
    )]
    fn write(
        lease: u32,
        lba: u64,
        data: MaxLen<RefVec<'i, u8>, MAX_TRANSFER>,
        crc: u32,
    ) -> Result<(), BlockError<'i>>;

    /// Erases `count` blocks from `lba` (CMD38 on an SD card, a sector erase on flash, a discard on eMMC). Whole
    /// erase units ([Geometry::erase_blocks]) where it can; afterwards the blocks read as [Geometry::erased].
    /// Takes as long as the medium needs.
    #[errors(NoMedium, OutOfRange, BadLease, WriteFailed)]
    fn erase(lease: u32, lba: u64, count: u32) -> Result<(), BlockError<'i>>;

    /// Bulk write: runs of blocks, each with the lease from [BlockDevice::claim], acknowledged one by one on
    /// [BlockDevice::written] once on the medium (or refused). Credit-based (API-28): the host never sends more
    /// than the device has room for. For long jobs and slow links: writing a prepared filesystem, an image.
    #[optional(bulk)]
    #[credit]
    sink!(write_runs: WriteRun<'i>);

    /// The outcome of each run of [BlockDevice::write_runs], in the order they came. A counted stream, unit: run.
    #[optional(bulk)]
    #[counted]
    stream!(written: RunAck);

    /// Bulk read: `count` blocks from `lba` as runs of up to [Geometry::max_blocks] on this stream, at the link's
    /// full speed instead of one call per run. The stream ends when the last run is sent; opening it again starts
    /// another range. A counted stream, unit: block.
    #[optional(bulk)]
    #[errors(Common(Busy), NoMedium, OutOfRange)]
    #[stream(ReadRun<'i>, counted)]
    fn read_runs(lba: u64, count: u64) -> Result<(), BlockError<'i>>;
}

/// The medium, see [BlockDevice::geometry].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct Geometry<'i> {
    pub kind: MediumKind,
    /// Bytes per block: 512 for SD cards and eMMC, the program unit (or the sector) for flash.
    pub block_size: u32,
    /// Blocks on the medium; the last LBA is one less.
    pub block_count: u64,
    /// Most blocks one [BlockDevice::read], [BlockDevice::write] or bulk run carries on this device, at most
    /// [MAX_TRANSFER] bytes: what fits its message size and buffers (1 on a device with 1 KiB messages).
    pub max_blocks: u16,
    /// Blocks in an erase unit: the allocation unit (AU) of an SD card, the sector of a NOR flash. Erases and large
    /// writes are fastest in whole, aligned units.
    pub erase_blocks: u32,
    /// The write size that measured fastest, in blocks, `None` when not measured.
    pub best_write_blocks: Option<u32>,
    /// What erased blocks read as.
    pub erased: Erased,
    /// The medium must be erased before a block can be written again (NOR flash): a write to a block that isn't
    /// erased fails or corrupts it.
    pub erase_before_write: bool,
    /// The medium is write-protected (switch, register bit, or a read-only firmware): [BlockDevice::claim] answers
    /// `ReadOnly`.
    pub read_only: bool,
    /// Changes when another medium is inserted (from the card's CID, or a counter): a host notices a swap between
    /// calls. 0 if the device can't tell.
    pub medium_id: u64,
    /// Product name of the medium (CID of a card, JEDEC part of a flash), empty if unknown.
    pub name: MaxLen<&'i str, MAX_NAME>,
}

/// Kind of medium, see [Geometry::kind].
///
/// `Unsized` (default): a later version may add a kind without breaking a host built against this one.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum MediumKind {
    /// SD card (SDSC, SDHC, SDXC, SDUC), in SD or SPI mode.
    SdCard,
    /// Soldered or socketed eMMC.
    Emmc,
    /// SPI, QSPI or parallel NOR flash.
    NorFlash,
    /// NAND flash with its own block management.
    NandFlash,
    /// RAM disk, file on a host, anything else.
    Other,
}

/// What erased blocks read as, see [Geometry::erased].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Erased {
    /// All bytes `0x00`.
    Zeros,
    /// All bytes `0xff`.
    Ones,
    /// Not defined (an SD card may answer either, eMMC discard leaves old data).
    Unspecified,
}

/// One run of [BlockDevice::write_runs]: whole blocks at `lba`, as [BlockDevice::write] takes them.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct WriteRun<'i> {
    pub lease: u32,
    pub lba: u64,
    /// CRC-32 ([crc32]) of `data`, checked before anything is written.
    pub crc: u32,
    pub data: MaxLen<RefVec<'i, u8>, MAX_TRANSFER>,
}

/// One run of [BlockDevice::read_runs]: the blocks from `lba`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct ReadRun<'i> {
    pub lba: u64,
    pub data: MaxLen<RefVec<'i, u8>, MAX_TRANSFER>,
}

/// The outcome of one run of [BlockDevice::write_runs], on [BlockDevice::written].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct RunAck {
    /// The run's first block and its length in blocks.
    pub lba: u64,
    pub count: u32,
    pub status: RunStatus,
}

/// What became of a run, see [RunAck::status]. The same as [BlockDevice::write]'s answer.
///
/// `Unsized` (default): a later version may add an outcome without breaking a host built against this one.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum RunStatus {
    /// On the medium.
    Written,
    /// Not whole blocks, or more than [Geometry::max_blocks].
    BadLength,
    OutOfRange,
    BadLease,
    BadCrc,
    WriteFailed,
    NoMedium,
}

/// Errors of the block device calls.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    display,
    ww_repr = unib32
)]
pub enum BlockError<'i> {
    /// Common error
    #[display("{0}")]
    #[common]
    Common(CommonError<'i>),
    /// No card in the slot, or the medium doesn't answer.
    #[display("no medium")]
    NoMedium,
    /// A block past the last one.
    #[display("block out of range")]
    OutOfRange,
    /// The lease lapsed, was released, or is not the current one: claim the card again.
    #[display("lease lapsed or not valid")]
    BadLease,
    /// The data's CRC-32 doesn't match the one given: nothing was written.
    #[display("CRC mismatch, nothing written")]
    BadCrc,
    /// The medium is write-protected.
    #[display("medium is read-only")]
    ReadOnly,
    /// The medium reported a read error (or its data failed the device's own check).
    #[display("read failed: {reason:?}")]
    ReadFailed { reason: MaxLen<&'i str, 64> },
    /// The medium reported a write or erase error: what was written may be partly there.
    #[display("write failed: {reason:?}")]
    WriteFailed { reason: MaxLen<&'i str, 64> },
}

impl BlockError<'_> {
    /// `Common(Busy)`.
    pub const fn busy() -> Self {
        BlockError::Common(CommonError::Busy)
    }
}

/// CRC-32 of `data` as [BlockDevice::crc], [BlockDevice::write] and [WriteRun] use it: CRC-32/ISO-HDLC, the one of
/// zip, PNG and Ethernet (polynomial `0x04c11db7` reflected, init and final xor `0xffffffff`; `crc32(b"123456789")`
/// is `0xcbf43926`). The same as an STM32 CRC unit set to reflect input and output, with the result inverted.
pub fn crc32(data: &[u8]) -> u32 {
    !crc32_update(!0, data)
}

/// Continues a CRC-32 over more data: start with `!0`, feed every piece, invert the end result. `crc32(a ++ b)` is
/// `!crc32_update(crc32_update(!0, a), b)`. Bitwise, no table: small, for checking a few KiB at a time.
pub fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
        let a = b"1234";
        let b = b"56789";
        assert_eq!(!crc32_update(crc32_update(!0, a), b), 0xcbf4_3926);
    }

    #[test]
    fn write_run_round_trip() {
        let data = [0x5au8; 1024];
        let run = WriteRun {
            lease: 7,
            lba: 1 << 33,
            crc: crc32(&data),
            data: MaxLen::new(RefVec::new_bytes(&data)).unwrap(),
        };
        let mut buf = [0u8; 1100];
        let bytes = run.to_ww_bytes(&mut buf).unwrap();
        // the data and a few bytes of header
        assert!(bytes.len() <= 1024 + 24, "{} bytes", bytes.len());
        assert_eq!(WriteRun::from_ww_bytes(bytes).unwrap(), run);
    }

    #[test]
    fn ack_is_small() {
        let ack = RunAck {
            lba: 2048,
            count: 8,
            status: RunStatus::Written,
        };
        let mut buf = [0u8; 32];
        let bytes = ack.to_ww_bytes(&mut buf).unwrap();
        assert!(bytes.len() <= 16, "{} bytes", bytes.len());
        assert_eq!(RunAck::from_ww_bytes(bytes).unwrap(), ack);
    }
}
