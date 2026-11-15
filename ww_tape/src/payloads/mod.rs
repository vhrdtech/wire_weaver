//! The payload codec: the payload lengths and payload bytes of an events chunk.
//!
//! **Lengths** ([ColumnKind::PayloadLengths], codec [LENGTHS], in the event codecs' range 1 to 15): a record per
//! group of 128 items, then each group's lengths less its minimum, packed at the group's width
//! ([bitpack]):
//!
//! ```text
//! per group: base u32 (where its first item's payload starts in the decoded payload bytes), minimum u32 (its
//!            shortest payload), width u8 (0 to 32), offset u32 (of its packed lengths, from the end of the records)
//! then:      packed lengths, group after group, a group's items × width bits each
//! ```
//!
//! Payloads of one length cost their records only. The base places a group without the groups before it, so item
//! `i`'s payload is found from one record and one group of lengths.
//!
//! **Payloads** ([ColumnKind::Payloads]) are the items' bytes back to back, in item order: [Codec::RAW], read in
//! place, or [Codec::LZ4], decoded once when the chunk is opened. The encoder takes lz4 when it is smaller.
//!
//! A chunk has both columns or neither (every payload empty). Numbers are little endian, unaligned. A payloads column
//! in the reserved [Codec::SERIES] stands alone; this reader refuses it ([PayloadsError::UnknownCodec]).
//!
//! A group's lengths are in one form only: the minimum is the group's shortest payload (so the smallest packed value
//! is 0), the width the bits the largest packed value needs, and the bits after the last value in its last byte 0.
//! A reader takes no other, so a damaged width or minimum can't re-read a group as other lengths of the same total.
//!
//! **What read checks.** Every group: width at most 32, its lengths in that form and inside the column, its offset where the previous
//! group's lengths end and its base where the previous group's payloads end (group 0's both 0), the last group's
//! lengths at the column's end and its payloads at the payload bytes' end. So every item's payload lies in the
//! payload bytes, and [Payloads::get] never fails on a chunk [Payloads::read] took.
//!
//! **Salvage.** A chunk whose payload CRC failed ([Block::salvage](crate::Block::salvage)) still reads through
//! [Payloads::salvage]: each group of lengths is checked against its neighbours as in [EventTimes::salvage](
//! crate::times::EventTimes::salvage) (a damaged offset or base breaks the links on both sides of its group, a damaged
//! width, minimum or length only the link after it), and the items of a group that fails have no payload and are
//! suspect ([Payloads::is_suspect]). A raw item's payload with a byte in a bad segment reads as stored and is
//! suspect; an lz4 column with a byte in a bad segment makes every item suspect, its payloads read as decoded when
//! the block still decodes to its stated length, else empty.

mod encode;
#[cfg(test)]
mod tests;

pub use encode::{EncodeError, EncodedPayloads};

use crate::bitpack::{self, GROUP};
use crate::chunk::{Chunk, Codec, Column, ColumnKind};
use crate::lz4::{Lz4Error, lz4_decode, lz4_decoded_len};
use crate::times::u32_to_usize;
use crate::{BlockKind, FromFile};
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

/// Codec of a payload lengths column.
pub const LENGTHS: Codec = Codec(4);

/// Bytes of a group record of the lengths column.
pub(crate) const RECORD_LEN: usize = 13;

/// Widest packed length, in bits: lengths are u32.
const WIDTH_MAX: u8 = 32;

/// Why a chunk's payload columns can't be read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PayloadsError {
    /// The chunk is not an events chunk.
    NotEvents,
    /// One payload column is there without the other.
    Missing(ColumnKind),
    /// A payload column is there twice.
    Unexpected(ColumnKind),
    /// A payload column in a codec this reader doesn't know.
    UnknownCodec(ColumnKind, Codec),
    /// A column breaks the format.
    Corrupt {
        kind: ColumnKind,
        what: &'static str,
    },
    /// The lz4 payloads column doesn't decode.
    Lz4(Lz4Error),
}

impl core::fmt::Display for PayloadsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PayloadsError::NotEvents => write!(f, "not an events chunk"),
            PayloadsError::Missing(k) => write!(f, "no {k:?} column"),
            PayloadsError::Unexpected(k) => write!(f, "{k:?} column twice"),
            PayloadsError::UnknownCodec(k, c) => write!(f, "{k:?} column in unknown codec {}", c.0),
            PayloadsError::Corrupt { kind, what } => write!(f, "{kind:?} column: {what}"),
            PayloadsError::Lz4(_) => write!(f, "lz4 payloads column doesn't decode"),
        }
    }
}

impl core::error::Error for PayloadsError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            PayloadsError::Lz4(e) => Some(e),
            _ => None,
        }
    }
}

/// A group record of the lengths column, its values bounded: width at most 32, offset inside the packed lengths,
/// base and minimum inside the payload bytes.
#[derive(Copy, Clone, Debug)]
struct Record {
    base: usize,
    min: usize,
    width: u32,
    offset: usize,
}

/// The lengths column: records and packed lengths, read in place.
#[derive(Copy, Clone, Debug)]
struct Lengths<'b> {
    /// Items in the chunk.
    n: usize,
    /// One record per group.
    records: &'b [[u8; RECORD_LEN]],
    /// The packed lengths and everything after them in the payload, for the unpack kernels' reach.
    data: &'b [u8],
    /// Bytes of packed lengths in the column.
    data_len: usize,
    /// Bytes of payloads; None when an lz4 column doesn't decode.
    total: Option<usize>,
}

impl<'b> Lengths<'b> {
    /// Split `column` (its bytes; `tail` the payload from the column's start on) into records and packed lengths,
    /// for `total` bytes of payloads.
    fn split(
        n: usize,
        column: &'b [u8],
        tail: &'b [u8],
        total: Option<usize>,
    ) -> Result<Self, PayloadsError> {
        let corrupt = PayloadsError::Corrupt {
            kind: ColumnKind::PayloadLengths,
            what: "shorter than its group records",
        };
        let (all, _) = column.as_chunks::<RECORD_LEN>();
        let records = all.get(..n.div_ceil(GROUP)).ok_or(corrupt)?;
        let head = records.as_flattened().len();
        let data = tail.get(head..).ok_or(corrupt)?;
        let data_len = column.len().checked_sub(head).ok_or(corrupt)?;
        Ok(Lengths {
            n,
            records,
            data,
            data_len,
            total,
        })
    }

    fn count(&self) -> usize {
        self.records.len()
    }

    /// Items in group `g`; 0 past the last group.
    fn group_len(&self, g: usize) -> usize {
        let first = g.saturating_mul(GROUP);
        self.n.saturating_sub(first).min(GROUP)
    }

    /// Group `g`'s record; what is out of bounds when it isn't one.
    fn checked_record(&self, g: usize) -> Result<Record, &'static str> {
        let r = self
            .records
            .get(g)
            .ok_or("shorter than its group records")?;
        let [b0, b1, b2, b3, m0, m1, m2, m3, w, o0, o1, o2, o3] = *r;
        let width = FromFile::new(w)
            .bounded(WIDTH_MAX)
            .or(Err("width over 32"))?;
        let offset = FromFile::new(u32::from_le_bytes([o0, o1, o2, o3]))
            .within(self.data_len)
            .or(Err("packed lengths past the column"))?;
        let total = self.total.ok_or("payloads that don't decode")?;
        let past = "group base or minimum past the payload bytes";
        let base = FromFile::new(u32::from_le_bytes([b0, b1, b2, b3]))
            .within(total)
            .or(Err(past))?;
        let min = FromFile::new(u32::from_le_bytes([m0, m1, m2, m3]))
            .within(total)
            .or(Err(past))?;
        Ok(Record {
            base,
            min,
            width: u32::from(width),
            offset,
        })
    }

    fn record(&self, g: usize) -> Option<Record> {
        self.checked_record(g).ok()
    }

    /// Where group `g`'s packed lengths end; None for a record out of bounds or an end past the column.
    fn packed_end(&self, g: usize) -> Option<usize> {
        let r = self.record(g)?;
        let end = r
            .offset
            .checked_add(bitpack::packed_len(self.group_len(g), r.width))?;
        (end <= self.data_len).then_some(end)
    }

    /// Unpack group `g`'s lengths less its minimum into `out`; None when its record doesn't fit the column.
    fn unpack(&self, g: usize, out: &mut [u32; GROUP]) -> Option<Record> {
        self.packed_end(g)?;
        let r = self.record(g)?;
        if r.width == 0 {
            out.fill(0);
        } else {
            // the width is at most 32, the kernels' widest
            bitpack::unpack_u32(self.data.get(r.offset..)?, r.width, out);
        }
        Some(r)
    }

    /// Bytes of group `g`'s payloads; None when its record doesn't fit the column or its lengths are not in the
    /// encoder's form.
    fn group_bytes(&self, g: usize) -> Option<usize> {
        let mut v = [0u32; GROUP];
        let r = self.unpack(g, &mut v)?;
        let len = self.group_len(g);
        let vals = v.get(..len)?;
        if r.width > 0 {
            let min = vals.iter().copied().min()?;
            let max = vals.iter().copied().max()?;
            let packed = bitpack::packed_len(len, r.width);
            let spare = packed
                .checked_mul(8)?
                .checked_sub(len.checked_mul(u32_to_usize(r.width))?)?;
            let last = *self
                .data
                .get(r.offset.checked_add(packed)?.checked_sub(1)?)?;
            // the bits after the last value are 0
            let tail_zero =
                spare == 0 || last.checked_shr(8u32.checked_sub(spare_bits(spare)?)?)? == 0;
            if min != 0 || bitpack::width_of(u64::from(max)) != r.width || !tail_zero {
                return None;
            }
        }
        let packed = sum(vals)?;
        packed.checked_add(r.min.checked_mul(len)?)
    }

    /// Whether group `g`'s lengths end where group `g + 1`'s start and its payloads where group `g + 1`'s base says,
    /// the last group's at the column's end and at the payload bytes' end.
    fn ends_at_next(&self, g: usize) -> bool {
        let next = if g.saturating_add(1) < self.count() {
            self.record(g.saturating_add(1)).map(|r| (r.offset, r.base))
        } else {
            self.total.map(|t| (self.data_len, t))
        };
        let (Some((offset, base)), Some(end), Some(bytes), Some(r)) = (
            next,
            self.packed_end(g),
            self.group_bytes(g),
            self.record(g),
        ) else {
            return false;
        };
        end == offset && r.base.checked_add(bytes) == Some(base)
    }

    /// Whether group `g` starts where group `g - 1` ends, group 0 at 0.
    fn starts_at_previous(&self, g: usize) -> bool {
        match g.checked_sub(1) {
            None => self.record(0).is_some_and(|r| r.offset == 0 && r.base == 0),
            Some(p) => self.ends_at_next(p),
        }
    }

    /// Whether group `g`'s record fits the column and its neighbours, blaming a broken link on the group whose
    /// record broke it (as the time codec's groups do).
    fn group_fits(&self, g: usize) -> bool {
        if self.packed_end(g).is_none() {
            return false;
        }
        let next = g.saturating_add(1);
        self.ends_at_next(g)
            || (self.starts_at_previous(g) && next < self.count() && !self.ends_at_next(next))
    }

    /// Check every record against the column and the payload bytes.
    fn check(&self) -> Result<(), PayloadsError> {
        let corrupt = |what| PayloadsError::Corrupt {
            kind: ColumnKind::PayloadLengths,
            what,
        };
        if !self.starts_at_previous(0) {
            return Err(corrupt("group 0 doesn't start at 0"));
        }
        for g in 0..self.count() {
            self.checked_record(g).map_err(corrupt)?;
            if self.packed_end(g).is_none() {
                return Err(corrupt("packed lengths past the column"));
            }
            if !self.ends_at_next(g) {
                return Err(corrupt(
                    "a group doesn't end where the next starts, or the last at the column's end",
                ));
            }
        }
        Ok(())
    }

    /// Where item `i`'s payload lies in the payload bytes; None when its group's record doesn't fit the column.
    fn range(&self, i: usize) -> Option<Range<usize>> {
        let (g, j) = (i / GROUP, i % GROUP);
        let mut v = [0u32; GROUP];
        let r = self.unpack(g, &mut v)?;
        let before = sum(v.get(..j)?)?;
        let start = r
            .base
            .checked_add(r.min.checked_mul(j)?)?
            .checked_add(before)?;
        let len = r.min.checked_add(u32_to_usize(*v.get(j)?))?;
        Some(start..start.checked_add(len)?)
    }
}

/// Spare bits of a group's last byte: 1 to 7.
fn spare_bits(spare: usize) -> Option<u32> {
    u32::try_from(spare).ok().filter(|s| *s < 8)
}

/// The sum of packed lengths; None past usize.
fn sum(vals: &[u32]) -> Option<usize> {
    vals.iter()
        .try_fold(0usize, |a, x| a.checked_add(u32_to_usize(*x)))
}

/// The payload bytes, in place or decoded.
#[derive(Clone, Debug)]
enum Bytes<'b> {
    InPlace(&'b [u8]),
    Decoded(Vec<u8>),
}

impl Bytes<'_> {
    fn as_slice(&self) -> &[u8] {
        match self {
            Bytes::InPlace(b) => b,
            Bytes::Decoded(b) => b,
        }
    }
}

/// The payloads of an events chunk: raw columns read in place, lz4 ones decoded once.
#[derive(Clone, Debug)]
pub struct Payloads<'b> {
    lengths: Lengths<'b>,
    bytes: Bytes<'b>,
    /// Per group, whether a salvage found its record damaged; empty when read intact.
    suspect_groups: Vec<bool>,
    /// Every item is suspect: an lz4 payloads column in a bad segment.
    all_suspect: bool,
    /// Byte ranges of a raw payloads column in bad segments, ascending.
    bad: Vec<Range<usize>>,
}

impl<'b> Payloads<'b> {
    /// Read the payload columns of an intact events chunk, checking every group record; None when the chunk has no
    /// payload columns.
    pub fn read(chunk: &Chunk<'b>) -> Result<Option<Self>, PayloadsError> {
        let Some((lengths, payloads)) = columns(chunk)? else {
            return Ok(None);
        };
        let tail = chunk
            .payload()
            .get(lengths.offset..)
            .ok_or(PayloadsError::Missing(ColumnKind::PayloadLengths))?;
        let n = u32_to_usize(chunk.head().count);
        let l = Lengths::split(n, lengths.bytes, tail, None)?;
        let (l, bytes) = if payloads.codec == Codec::LZ4 {
            // at most LZ4_DECODED_MAX, and what the block can fill
            let len = lz4_decoded_len(payloads.bytes).map_err(PayloadsError::Lz4)?;
            let l = Lengths {
                total: Some(len),
                ..l
            };
            l.check()?;
            let mut out = vec![0u8; len];
            lz4_decode(payloads.bytes, &mut out).map_err(PayloadsError::Lz4)?;
            (l, Bytes::Decoded(out))
        } else {
            let l = Lengths {
                total: Some(payloads.bytes.len()),
                ..l
            };
            l.check()?;
            (l, Bytes::InPlace(payloads.bytes))
        };
        Ok(Some(Payloads {
            lengths: l,
            bytes,
            suspect_groups: Vec::new(),
            all_suspect: false,
            bad: Vec::new(),
        }))
    }

    /// Read the payload columns of a chunk whose payload may be damaged ([Chunk::bad_segments]): group records are
    /// checked one by one and only the items of a group that doesn't hold up, or whose bytes are in a bad segment,
    /// are suspect ([Self::is_suspect]). None when the chunk has no payload columns; fails only when the chunk's head
    /// names columns this reader can't take, or the lengths column is shorter than its group records.
    pub fn salvage(chunk: &Chunk<'b>) -> Result<Option<Self>, PayloadsError> {
        let Some((lengths, payloads)) = columns(chunk)? else {
            return Ok(None);
        };
        let tail = chunk
            .payload()
            .get(lengths.offset..)
            .ok_or(PayloadsError::Missing(ColumnKind::PayloadLengths))?;
        let n = u32_to_usize(chunk.head().count);
        let l = Lengths::split(n, lengths.bytes, tail, None)?;
        let damaged = chunk.column_damaged(&payloads);
        let (bytes, total, all_suspect, bad) = if payloads.codec == Codec::LZ4 {
            // at most LZ4_DECODED_MAX, and what the block can fill
            let decoded = lz4_decoded_len(payloads.bytes).ok().and_then(|len| {
                let mut out = vec![0u8; len];
                lz4_decode(payloads.bytes, &mut out).ok().map(|_| out)
            });
            match decoded {
                Some(out) => {
                    let total = out.len();
                    (Bytes::Decoded(out), Some(total), damaged, Vec::new())
                }
                None => (Bytes::Decoded(Vec::new()), None, true, Vec::new()),
            }
        } else {
            let bad = if damaged {
                bad_ranges(chunk, &payloads)
            } else {
                Vec::new()
            };
            let total = payloads.bytes.len();
            (Bytes::InPlace(payloads.bytes), Some(total), false, bad)
        };
        let l = Lengths { total, ..l };
        // a record per group fits the column: its length bounds the groups
        let suspect_groups = (0..l.count()).map(|g| !l.group_fits(g)).collect();
        Ok(Some(Payloads {
            lengths: l,
            bytes,
            suspect_groups,
            all_suspect,
            bad,
        }))
    }

    /// Items in the chunk.
    pub fn len(&self) -> usize {
        self.lengths.n
    }

    /// Always false: a chunk has an item at least.
    pub fn is_empty(&self) -> bool {
        self.lengths.n == 0
    }

    fn group_ok(&self, g: usize) -> bool {
        self.suspect_groups.get(g) != Some(&true)
    }

    /// Where item `i`'s payload lies in the payload bytes; None when a salvage lost its place.
    fn range(&self, i: usize) -> Option<Range<usize>> {
        if !self.group_ok(i / GROUP) {
            return None;
        }
        self.placed(self.lengths.range(i))
    }

    /// `range` when it lies in the payload bytes.
    fn placed(&self, range: Option<Range<usize>>) -> Option<Range<usize>> {
        range.filter(|r| r.end <= self.bytes.as_slice().len())
    }

    /// The bytes and suspicion of an item placed at `range` (None: place lost).
    fn item(&self, range: Option<Range<usize>>) -> Item<'_> {
        let suspect = self.all_suspect
            || match &range {
                None => true,
                Some(r) => self.bad.iter().any(|b| b.start < r.end && r.start < b.end),
            };
        let bytes = range
            .and_then(|r| self.bytes.as_slice().get(r))
            .unwrap_or(&[]);
        Item { bytes, suspect }
    }

    /// The payload of item `i`, None past the last item. A salvaged item whose place was lost reads as empty.
    pub fn get(&self, i: usize) -> Option<&[u8]> {
        (i < self.len()).then(|| self.item(self.range(i)).bytes)
    }

    /// Whether item `i`'s payload may be damaged: its group record failed a salvage's check, its bytes are in a bad
    /// segment, or the lz4 column holding it is. False past the last item.
    pub fn is_suspect(&self, i: usize) -> bool {
        i < self.len() && self.item(self.range(i)).suspect
    }

    /// Every item in order, as [Self::get] and [Self::is_suspect] give it, each group of lengths unpacked once.
    pub fn iter(&self) -> Iter<'_, 'b> {
        Iter {
            payloads: self,
            i: 0,
            lengths: [0; GROUP],
            group: None,
        }
    }

    /// The item ranges whose payloads are suspect, one per item run, ascending.
    pub fn suspect_items(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        let mut items = self.iter().map(|it| it.suspect).enumerate().peekable();
        core::iter::from_fn(move || {
            let (start, _) = items.find(|(_, suspect)| *suspect)?;
            let mut end = start.saturating_add(1);
            while let Some((i, _)) = items.next_if(|(_, suspect)| *suspect) {
                end = i.saturating_add(1);
            }
            Some(start..end)
        })
    }
}

/// An item's payload, as [Payloads::iter] gives it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Item<'a> {
    /// The payload; empty for a salvaged item whose place was lost.
    pub bytes: &'a [u8],
    /// Whether the payload may be damaged ([Payloads::is_suspect]).
    pub suspect: bool,
}

/// The items of [Payloads] in order ([Payloads::iter]).
#[derive(Clone, Debug)]
pub struct Iter<'a, 'b> {
    payloads: &'a Payloads<'b>,
    i: usize,
    /// The current group's lengths less its minimum.
    lengths: [u32; GROUP],
    /// The current group's minimum and where its next payload starts; None when its place is lost.
    group: Option<(usize, usize)>,
}

impl<'a> Iterator for Iter<'a, '_> {
    type Item = Item<'a>;

    fn next(&mut self) -> Option<Item<'a>> {
        let p = self.payloads;
        if self.i >= p.len() {
            return None;
        }
        let (g, j) = (self.i / GROUP, self.i % GROUP);
        if j == 0 {
            self.group = if p.group_ok(g) {
                p.lengths
                    .unpack(g, &mut self.lengths)
                    .map(|r| (r.min, r.base))
            } else {
                None
            };
        }
        let range = self.group.as_mut().and_then(|(min, start)| {
            let len = min.checked_add(u32_to_usize(*self.lengths.get(j)?))?;
            let from = *start;
            let to = from.checked_add(len)?;
            *start = to;
            Some(from..to)
        });
        if range.is_none() {
            self.group = None;
        }
        self.i = self.i.saturating_add(1);
        Some(p.item(p.placed(range)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.payloads.len().saturating_sub(self.i);
        (n, Some(n))
    }
}

impl ExactSizeIterator for Iter<'_, '_> {}

/// The two payload columns of `chunk`, codecs checked; None when it has neither.
fn columns<'b>(chunk: &Chunk<'b>) -> Result<Option<(Column<'b>, Column<'b>)>, PayloadsError> {
    if chunk.head().kind != BlockKind::Events {
        return Err(PayloadsError::NotEvents);
    }
    let mut lengths = None;
    let mut payloads = None;
    for c in chunk.columns() {
        let slot = match c.kind {
            ColumnKind::PayloadLengths => &mut lengths,
            ColumnKind::Payloads => &mut payloads,
            _ => continue,
        };
        if slot.replace(c).is_some() {
            return Err(PayloadsError::Unexpected(c.kind));
        }
    }
    if let Some(p) = payloads
        && p.codec == Codec::SERIES
    {
        return Err(PayloadsError::UnknownCodec(p.kind, p.codec));
    }
    let (l, p) = match (lengths, payloads) {
        (None, None) => return Ok(None),
        (Some(l), Some(p)) => (l, p),
        (None, Some(_)) => return Err(PayloadsError::Missing(ColumnKind::PayloadLengths)),
        (Some(_), None) => return Err(PayloadsError::Missing(ColumnKind::Payloads)),
    };
    if l.codec != LENGTHS {
        return Err(PayloadsError::UnknownCodec(l.kind, l.codec));
    }
    if p.codec != Codec::RAW && p.codec != Codec::LZ4 {
        return Err(PayloadsError::UnknownCodec(p.kind, p.codec));
    }
    Ok(Some((l, p)))
}

/// The byte ranges of `column` that lie in a bad segment of `chunk`, in the column's own offsets.
fn bad_ranges(chunk: &Chunk<'_>, column: &Column<'_>) -> Vec<Range<usize>> {
    let seg = chunk.segment_len();
    let from = column.offset;
    let to = from.saturating_add(column.bytes.len());
    chunk
        .bad_segments()
        .filter_map(|s| {
            let a = s.saturating_mul(seg).max(from);
            let b = s.saturating_add(1).saturating_mul(seg).min(to);
            (a < b).then(|| a.saturating_sub(from)..b.saturating_sub(from))
        })
        .collect()
}
