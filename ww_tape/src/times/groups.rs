//! The part the three packed columns share: per group a fixed-size record with a width and an offset, then the
//! packed values.

use super::TimesError;
use crate::FromFile;
use crate::bitpack::{self, GROUP, WIDTH_MAX};
use crate::chunk::ColumnKind;

/// Groups in a column of `n` items.
pub(crate) const fn group_count(n: usize) -> usize {
    n.div_ceil(GROUP)
}

/// Items in group `g` of a column of `n` items; 0 past the last group.
pub(crate) const fn group_len(n: usize, g: usize) -> usize {
    let rest = n.saturating_sub(g.saturating_mul(GROUP));
    if rest < GROUP { rest } else { GROUP }
}

/// Where a column's records keep the width and the offset of their group's values.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Layout<const R: usize> {
    /// The width a record states; over 64 in a damaged record.
    pub(crate) width: fn(&[u8; R]) -> FromFile<u32>,
    /// The offset of the group's values a record states, from the end of the records.
    pub(crate) offset: fn(&[u8; R]) -> FromFile<u32>,
}

/// A group's values where its record places them, checked against the column.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Placed<'b> {
    /// At most [WIDTH_MAX].
    pub(crate) width: u32,
    /// Where the values start in the packed values, at most the column's.
    pub(crate) offset: usize,
    /// Where they end, at most the column's.
    pub(crate) end: usize,
    /// The values from their first byte on, with everything after them in the payload for the kernels' reach.
    pub(crate) values: &'b [u8],
}

/// The records and packed values of one column.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Groups<'b, const R: usize> {
    pub(crate) kind: ColumnKind,
    pub(crate) n: usize,
    /// One record per group.
    records: &'b [[u8; R]],
    layout: Layout<R>,
    /// The packed values and everything after them in the payload, for the kernels' reach.
    data: &'b [u8],
    /// Bytes of packed values in the column.
    data_len: usize,
}

impl<'b, const R: usize> Groups<'b, R> {
    /// Split `column` (its bytes; `tail` is the payload from the column's start on) into records and packed values.
    pub(crate) fn split(
        kind: ColumnKind,
        n: usize,
        layout: Layout<R>,
        column: &'b [u8],
        tail: &'b [u8],
    ) -> Result<Self, TimesError> {
        let corrupt = |what| TimesError::Corrupt { kind, what };
        let (all, _) = column.as_chunks::<R>();
        let records = all
            .get(..group_count(n))
            .ok_or(corrupt("shorter than its group records"))?;
        let head = records.as_flattened().len();
        let data = tail.get(head..).ok_or(corrupt("column past the payload"))?;
        let data_len = column
            .len()
            .checked_sub(head)
            .ok_or(corrupt("shorter than its group records"))?;
        if data.len() < data_len {
            return Err(corrupt("column past the payload"));
        }
        Ok(Groups {
            kind,
            n,
            records,
            layout,
            data,
            data_len,
        })
    }

    pub(crate) fn count(&self) -> usize {
        self.records.len()
    }

    /// Group `g`'s record.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group: the public accessors assert it first.
    pub(crate) fn record(&self, g: usize) -> &'b [u8; R] {
        #[expect(
            clippy::expect_used,
            reason = "callers assert g under the group count, the records' length"
        )]
        let r = self.records.get(g).expect("group under the group count");
        r
    }

    /// Group `g`'s values where its record places them; None for a width over 64, values past the column or a
    /// group past the last.
    pub(crate) fn placed(&self, g: usize) -> Option<Placed<'b>> {
        let r = self.records.get(g)?;
        let width = (self.layout.width)(r).bounded(WIDTH_MAX).ok()?;
        let offset = (self.layout.offset)(r).within(self.data_len).ok()?;
        let end = offset.checked_add(bitpack::packed_len(group_len(self.n, g), width))?;
        if end > self.data_len {
            return None;
        }
        let values = self.data.get(offset..)?;
        Some(Placed {
            width,
            offset,
            end,
            values,
        })
    }

    /// Group `g`'s values in a column [Self::check] passed.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group: the public accessors assert it first.
    pub(crate) fn checked(&self, g: usize) -> Placed<'b> {
        #[expect(
            clippy::expect_used,
            reason = "check placed every group; callers assert g under the group count"
        )]
        let p = self
            .placed(g)
            .expect("a checked group under the group count");
        p
    }

    /// Check every record: widths at most 64, each offset where the previous group ends, the last group ending at
    /// the column's end.
    pub(crate) fn check(&self) -> Result<(), TimesError> {
        let mut at = 0usize;
        for g in 0..self.count() {
            let r = self.record(g);
            if (self.layout.width)(r).bounded(WIDTH_MAX).is_err() {
                return Err(self.corrupt("width over 64"));
            }
            match self.placed(g) {
                Some(p) if p.offset == at => at = p.end,
                _ => return Err(self.corrupt("group offset not where the previous group ends")),
            }
        }
        if at != self.data_len {
            return Err(self.corrupt("packed values don't end at the column's end"));
        }
        Ok(())
    }

    /// The offset group `g + 1`'s record states, the column's end for the last group; None past it.
    fn next_offset(&self, g: usize) -> Option<usize> {
        let next = g.checked_add(1)?;
        if next == self.count() {
            return Some(self.data_len);
        }
        let r = self.records.get(next)?;
        (self.layout.offset)(r).within(self.data_len).ok()
    }

    /// Whether group `g`'s values end where group `g + 1`'s start, the last group's at the column's end.
    fn ends_at_next(&self, g: usize) -> bool {
        match (self.placed(g), self.next_offset(g)) {
            (Some(p), Some(next)) => p.end == next,
            _ => false,
        }
    }

    /// Whether group `g` starts where group `g - 1` ends, group 0 at 0.
    fn starts_at_previous(&self, g: usize) -> bool {
        match g.checked_sub(1) {
            None => self.placed(0).is_some_and(|p| p.offset == 0),
            Some(p) => self.ends_at_next(p),
        }
    }

    /// Group `g`'s values when its record fits the column: width at most 64, values inside the column, and no
    /// broken link to a neighbour that `g` is to blame for.
    ///
    /// A damaged offset breaks the links on both sides of its group; a damaged width only the link after it. So a
    /// broken link after `g` is `g`'s fault when the link before `g` is broken too, when `g` is the last group, or
    /// when the link after `g + 1` holds; otherwise `g + 1`'s offset is the damaged one. A good neighbour of a
    /// damaged group stays good.
    pub(crate) fn group_fits(&self, g: usize) -> Option<Placed<'b>> {
        let p = self.placed(g)?;
        let fits = self.ends_at_next(g)
            || (self.starts_at_previous(g)
                && g.checked_add(1)
                    .is_some_and(|next| next < self.count() && !self.ends_at_next(next)));
        fits.then_some(p)
    }

    fn corrupt(&self, what: &'static str) -> TimesError {
        TimesError::Corrupt {
            kind: self.kind,
            what,
        }
    }
}

/// The largest value `width` bits hold: 0 at width 0, all ones from 64.
pub(crate) const fn width_span(width: u32) -> u64 {
    match 1u64.checked_shl(width) {
        // b is at least 1
        Some(b) => b.wrapping_sub(1),
        None => u64::MAX,
    }
}

/// Write a column of `values` (one per item): per group the record `record(group values, offset)` returns with
/// the group's packed values and their width, then the packed values. None when the packed values pass a u32
/// offset or a width is over 64.
#[cfg(feature = "std")]
pub(crate) fn encode_groups<T: Copy, const R: usize>(
    values: &[T],
    mut record: impl FnMut(&[T], u32) -> Option<([u8; R], std::vec::Vec<u64>, u32)>,
) -> Option<std::vec::Vec<u8>> {
    let mut out = std::vec::Vec::with_capacity(group_count(values.len()).checked_mul(R)?);
    let mut packed = std::vec::Vec::new();
    for c in values.chunks(GROUP) {
        let at = packed.len();
        let (rec, vals, width) = record(c, u32::try_from(at).ok()?)?;
        if width > WIDTH_MAX {
            return None;
        }
        out.extend_from_slice(&rec);
        packed.resize(at.checked_add(bitpack::packed_len(c.len(), width))?, 0);
        bitpack::pack(packed.get_mut(at..)?, vals, width);
    }
    out.extend_from_slice(&packed);
    Some(out)
}

const _: () = assert!(
    usize::BITS >= u32::BITS,
    "a chunk's u32 item count fits a usize"
);

/// A chunk's item count as a usize.
pub(crate) const fn items(count: u32) -> usize {
    #[expect(
        clippy::as_conversions,
        reason = "usize is at least 32 bits, asserted above; From is not const"
    )]
    let n = count as usize;
    n
}

/// An item or group index as an i64.
///
/// # Panics
///
/// When `i` is over `i64::MAX`: callers pass indexes under a chunk's u32 item count, or [GROUP].
pub(crate) fn item_i64(i: usize) -> i64 {
    #[expect(
        clippy::expect_used,
        reason = "indexes are under a chunk's u32 item count, which fits an i64"
    )]
    let i = i64::try_from(i).expect("an index under a u32 count");
    i
}

/// The first of `lo..hi` where `below` is false, `hi` when it never is; `below` is true, then false, along the
/// range.
pub(crate) fn partition(mut lo: usize, mut hi: usize, below: impl Fn(usize) -> bool) -> usize {
    while lo < hi {
        let mid = lo.midpoint(hi);
        if below(mid) {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "mid is under hi, so mid + 1 fits"
            )]
            let next = mid + 1;
            lo = next;
        } else {
            hi = mid;
        }
    }
    lo
}
