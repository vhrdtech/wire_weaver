//! On-medium layout of a [LogStore](super::LogStore). Numbers are little endian, every structure has a CRC-32C.
//!
//! ```text
//! slot A | slot B | segment 0 | segment 1 | ... | segment n-1
//! ```
//!
//! - **Slots**: the superblock, written to A and B in turn, the one with the higher `write_seq` and a good CRC wins.
//!   It says where the log was when written (head and tail segment, session, clock epoch, released position); a
//!   mount walks on from its tail through the segment headers to the real tail.
//! - **Segments**: a ring. The segment with sequence number `seq` sits at index `seq % n`. Each starts with a
//!   header (log id, `seq`, session, clock epoch, the position the stream was at when it was opened), then records.
//! - **Records**: tag (u16), head length (u8), 0 (u8), item length (u32), CRC (u32), the encoded [StreamHead]
//!   (`crate::StreamHead`), the item. The tag and the CRC both depend on the log id and the segment's `seq`, so a
//!   record left over from an earlier lap of the ring never reads as part of the current one.

use crc::{CRC_32_ISCSI, Crc};

pub(super) const CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISCSI);
pub(super) const VERSION: u8 = 1;
const SB_MAGIC: u32 = u32::from_le_bytes(*b"WWLS");
const SEG_MAGIC: u32 = u32::from_le_bytes(*b"WWSG");

/// Bytes of an encoded [Superblock].
pub(super) const SB_LEN: usize = 80;
/// Bytes of an encoded [SegHeader].
pub(super) const SEG_HEADER_LEN: usize = 48;
/// Bytes of a record's own header, before the head.
pub(super) const REC_HEADER_LEN: usize = 12;
/// Longest encoded head a record takes.
pub(super) const MAX_HEAD: usize = 64;

/// Where the log was when it was written, see the [module docs](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Superblock {
    pub(super) write_seq: u64,
    pub(super) log_id: u32,
    pub(super) segment_size: u32,
    pub(super) segments: u32,
    pub(super) session: u32,
    pub(super) epoch: u32,
    pub(super) head_seq: u64,
    pub(super) tail_seq: u64,
    pub(super) low: u64,
    pub(super) next: u64,
    pub(super) dropped_units: u64,
}

impl Superblock {
    pub(super) fn encode(&self) -> [u8; SB_LEN] {
        let mut b = [0u8; SB_LEN];
        b[0..4].copy_from_slice(&SB_MAGIC.to_le_bytes());
        b[4] = VERSION;
        b[8..16].copy_from_slice(&self.write_seq.to_le_bytes());
        b[16..20].copy_from_slice(&self.log_id.to_le_bytes());
        b[20..24].copy_from_slice(&self.segment_size.to_le_bytes());
        b[24..28].copy_from_slice(&self.segments.to_le_bytes());
        b[28..32].copy_from_slice(&self.session.to_le_bytes());
        b[32..36].copy_from_slice(&self.epoch.to_le_bytes());
        b[36..44].copy_from_slice(&self.head_seq.to_le_bytes());
        b[44..52].copy_from_slice(&self.tail_seq.to_le_bytes());
        b[52..60].copy_from_slice(&self.low.to_le_bytes());
        b[60..68].copy_from_slice(&self.next.to_le_bytes());
        b[68..76].copy_from_slice(&self.dropped_units.to_le_bytes());
        let crc = CRC.checksum(&b[..SB_LEN - 4]);
        b[SB_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
        b
    }

    pub(super) fn decode(b: &[u8]) -> Option<Self> {
        if b.len() < SB_LEN
            || u32_at(b, 0) != SB_MAGIC
            || b[4] != VERSION
            || CRC.checksum(&b[..SB_LEN - 4]) != u32_at(b, SB_LEN - 4)
        {
            return None;
        }
        Some(Superblock {
            write_seq: u64_at(b, 8),
            log_id: u32_at(b, 16),
            segment_size: u32_at(b, 20),
            segments: u32_at(b, 24),
            session: u32_at(b, 28),
            epoch: u32_at(b, 32),
            head_seq: u64_at(b, 36),
            tail_seq: u64_at(b, 44),
            low: u64_at(b, 52),
            next: u64_at(b, 60),
            dropped_units: u64_at(b, 68),
        })
    }
}

/// Start of a segment, see the [module docs](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SegHeader {
    pub(super) log_id: u32,
    pub(super) seq: u64,
    pub(super) session: u32,
    pub(super) epoch: u32,
    /// Where the stream was when the segment was opened: every record in it is at or after this position.
    pub(super) first_pos: u64,
    /// Position of the newest event pushed before the segment was opened.
    pub(super) newest: Option<u64>,
}

impl SegHeader {
    pub(super) fn encode(&self) -> [u8; SEG_HEADER_LEN] {
        let mut b = [0u8; SEG_HEADER_LEN];
        b[0..4].copy_from_slice(&SEG_MAGIC.to_le_bytes());
        b[4] = VERSION;
        b[8..12].copy_from_slice(&self.log_id.to_le_bytes());
        b[12..20].copy_from_slice(&self.seq.to_le_bytes());
        b[20..24].copy_from_slice(&self.session.to_le_bytes());
        b[24..28].copy_from_slice(&self.epoch.to_le_bytes());
        b[28..36].copy_from_slice(&self.first_pos.to_le_bytes());
        b[36..44].copy_from_slice(&self.newest.unwrap_or(u64::MAX).to_le_bytes());
        let crc = CRC.checksum(&b[..SEG_HEADER_LEN - 4]);
        b[SEG_HEADER_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
        b
    }

    /// The header of segment `seq` of log `log_id`, `None` when the bytes are anything else.
    pub(super) fn decode(b: &[u8], log_id: u32, seq: u64) -> Option<Self> {
        if b.len() < SEG_HEADER_LEN
            || u32_at(b, 0) != SEG_MAGIC
            || b[4] != VERSION
            || CRC.checksum(&b[..SEG_HEADER_LEN - 4]) != u32_at(b, SEG_HEADER_LEN - 4)
            || u32_at(b, 8) != log_id
            || u64_at(b, 12) != seq
        {
            return None;
        }
        let newest = u64_at(b, 36);
        Some(SegHeader {
            log_id,
            seq,
            session: u32_at(b, 20),
            epoch: u32_at(b, 24),
            first_pos: u64_at(b, 28),
            newest: (newest != u64::MAX).then_some(newest),
        })
    }
}

/// Tag of the records of segment `seq` of log `log_id`: tells a record of this segment from leftovers.
pub(super) fn record_tag(log_id: u32, seq: u64) -> u16 {
    let x = (seq ^ ((log_id as u64) << 17)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let tag = (x >> 48) as u16;
    // never the erased pattern
    if tag == 0xffff { 0x5757 } else { tag }
}

/// Header of a record, see the [module docs](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecHeader {
    pub(super) head_len: usize,
    pub(super) item_len: usize,
    pub(super) crc: u32,
}

/// What the bytes at a record's place hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecSlot {
    /// A record of this segment (its CRC not checked yet).
    Record(RecHeader),
    /// Nothing written after the last record: erased bytes or leftovers of an earlier lap.
    End,
    /// The start of a record of this segment that can't be whole: a write cut short.
    Torn,
}

impl RecHeader {
    pub(super) fn encode(&self, tag: u16) -> [u8; REC_HEADER_LEN] {
        let mut b = [0u8; REC_HEADER_LEN];
        b[0..2].copy_from_slice(&tag.to_le_bytes());
        b[2] = self.head_len as u8;
        b[4..8].copy_from_slice(&(self.item_len as u32).to_le_bytes());
        b[8..12].copy_from_slice(&self.crc.to_le_bytes());
        b
    }

    /// Classify the bytes at a record's place; `room` is how many bytes are left in the segment from there.
    pub(super) fn decode(b: &[u8; REC_HEADER_LEN], tag: u16, room: usize) -> RecSlot {
        if b.iter().all(|&x| x == 0xff) || u16::from_le_bytes([b[0], b[1]]) != tag {
            return RecSlot::End;
        }
        let head_len = b[2] as usize;
        let item_len = u32_at(b, 4) as usize;
        if b[3] != 0
            || head_len == 0
            || head_len > MAX_HEAD
            || REC_HEADER_LEN + head_len + item_len > room
        {
            return RecSlot::Torn;
        }
        RecSlot::Record(RecHeader {
            head_len,
            item_len,
            crc: u32_at(b, 8),
        })
    }
}

/// Start of a record's CRC: the log id, the segment's `seq` and the first 8 bytes of the record header. The head
/// and the item follow.
pub(super) fn record_crc_start(
    digest: &mut crc::Digest<'_, u32>,
    log_id: u32,
    seq: u64,
    header: &[u8; REC_HEADER_LEN],
) {
    digest.update(&log_id.to_le_bytes());
    digest.update(&seq.to_le_bytes());
    digest.update(&header[..8]);
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superblock_round_trip_and_crc() {
        let sb = Superblock {
            write_seq: 7,
            log_id: 0xdead_beef,
            segment_size: 4096,
            segments: 10,
            session: 3,
            epoch: 2,
            head_seq: 11,
            tail_seq: 19,
            low: 1234,
            next: 5678,
            dropped_units: 9,
        };
        let mut b = sb.encode();
        assert_eq!(Superblock::decode(&b), Some(sb));
        b[40] ^= 1;
        assert_eq!(Superblock::decode(&b), None);
    }

    #[test]
    fn segment_header_is_bound_to_log_and_seq() {
        let h = SegHeader {
            log_id: 5,
            seq: 42,
            session: 1,
            epoch: 3,
            first_pos: 1 << 40,
            newest: None,
        };
        let b = h.encode();
        assert_eq!(SegHeader::decode(&b, 5, 42), Some(h));
        assert_eq!(SegHeader::decode(&b, 5, 42 + 10), None);
        assert_eq!(SegHeader::decode(&b, 6, 42), None);
    }

    #[test]
    fn record_slots() {
        let tag = record_tag(1, 2);
        assert_ne!(tag, record_tag(1, 2 + 8));
        let h = RecHeader {
            head_len: 3,
            item_len: 10,
            crc: 1,
        };
        let b = h.encode(tag);
        assert_eq!(RecHeader::decode(&b, tag, 100), RecSlot::Record(h));
        assert_eq!(RecHeader::decode(&b, tag, 20), RecSlot::Torn);
        assert_eq!(RecHeader::decode(&b, tag ^ 1, 100), RecSlot::End);
        assert_eq!(RecHeader::decode(&[0xff; 12], tag, 100), RecSlot::End);
    }
}
