//! The part the three packed columns share: per group a fixed-size record with a width and an offset, then the
//! packed values.

use super::TimesError;
use crate::bitpack::{self, GROUP, WIDTH_MAX};
use crate::chunk::ColumnKind;

/// Groups in a column of `n` items.
pub(crate) const fn group_count(n: u32) -> usize {
    (n as usize).div_ceil(GROUP)
}

/// Items in group `g` of a column of `n` items.
pub(crate) const fn group_len(n: u32, g: usize) -> usize {
    let rest = n as usize - g * GROUP;
    if rest < GROUP { rest } else { GROUP }
}

pub(crate) fn get_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

pub(crate) fn get_i64(b: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

/// Where a record keeps its width.
#[derive(Copy, Clone, Debug)]
pub(crate) enum WidthAt {
    /// A u8 at this byte of the record.
    Byte(usize),
    /// The bits a u32 OR at this byte of the record needs.
    OrOf(usize),
}

/// The records and packed values of one column.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Groups<'b> {
    pub(crate) kind: ColumnKind,
    pub(crate) n: u32,
    /// `group_count × record_len` bytes.
    pub(crate) records: &'b [u8],
    pub(crate) record_len: usize,
    pub(crate) width_at: WidthAt,
    /// Offset of the u32 offset in a record.
    pub(crate) offset_at: usize,
    /// The packed values and everything after them in the payload, for the kernels' reach.
    pub(crate) data: &'b [u8],
    /// Bytes of packed values in the column.
    pub(crate) data_len: usize,
}

/// How a column's group records look.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Layout {
    pub(crate) record_len: usize,
    pub(crate) width_at: WidthAt,
    /// Offset of the u32 offset in a record.
    pub(crate) offset_at: usize,
}

impl<'b> Groups<'b> {
    /// Split `column` (its bytes; `tail` is the payload from the column's start on) into records and packed values.
    pub(crate) fn split(
        kind: ColumnKind,
        n: u32,
        layout: &Layout,
        column: &'b [u8],
        tail: &'b [u8],
    ) -> Result<Self, TimesError> {
        let head = group_count(n) * layout.record_len;
        if column.len() < head {
            return Err(TimesError::Corrupt {
                kind,
                what: "shorter than its group records",
            });
        }
        debug_assert!(tail.len() >= column.len(), "tail starts with the column");
        Ok(Groups {
            kind,
            n,
            records: &column[..head],
            record_len: layout.record_len,
            width_at: layout.width_at,
            offset_at: layout.offset_at,
            data: &tail[head..],
            data_len: column.len() - head,
        })
    }

    pub(crate) fn count(&self) -> usize {
        group_count(self.n)
    }

    pub(crate) fn record(&self, g: usize) -> &'b [u8] {
        &self.records[g * self.record_len..(g + 1) * self.record_len]
    }

    /// The width as stored; over 64 in a damaged record.
    pub(crate) fn raw_width(&self, g: usize) -> u32 {
        let r = self.record(g);
        match self.width_at {
            WidthAt::Byte(at) => u32::from(r[at]),
            WidthAt::OrOf(at) => bitpack::width_of(u64::from(get_u32(r, at))),
        }
    }

    pub(crate) fn raw_offset(&self, g: usize) -> usize {
        get_u32(self.record(g), self.offset_at) as usize
    }

    /// Bytes of group `g`'s values at the width it states.
    fn packed(&self, g: usize) -> usize {
        bitpack::packed_len(group_len(self.n, g), self.raw_width(g))
    }

    /// Check every record: widths at most 64, each offset where the previous group ends, the last group ending at
    /// the column's end.
    pub(crate) fn check(&self) -> Result<(), TimesError> {
        let mut at = 0usize;
        for g in 0..self.count() {
            if self.raw_width(g) > WIDTH_MAX {
                return Err(self.corrupt("width over 64"));
            }
            if self.raw_offset(g) != at {
                return Err(self.corrupt("group offset not where the previous group ends"));
            }
            at += self.packed(g);
        }
        if at != self.data_len {
            return Err(self.corrupt("packed values don't end at the column's end"));
        }
        Ok(())
    }

    /// Where group `g`'s values end at the offset and width its record states; None for a width over 64 or an end
    /// past the column.
    fn end(&self, g: usize) -> Option<usize> {
        if self.raw_width(g) > WIDTH_MAX {
            return None;
        }
        let end = self.raw_offset(g).checked_add(self.packed(g))?;
        (end <= self.data_len).then_some(end)
    }

    /// Whether group `g`'s values end where group `g + 1`'s start, the last group's at the column's end.
    fn ends_at_next(&self, g: usize) -> bool {
        let next = if g + 1 < self.count() {
            self.raw_offset(g + 1)
        } else {
            self.data_len
        };
        self.end(g) == Some(next)
    }

    /// Whether group `g` starts where group `g - 1` ends, group 0 at 0.
    fn starts_at_previous(&self, g: usize) -> bool {
        if g == 0 {
            self.raw_offset(0) == 0
        } else {
            self.ends_at_next(g - 1)
        }
    }

    /// Whether group `g`'s record fits the column: width at most 64, values inside the column, and no broken link
    /// to a neighbour that `g` is to blame for.
    ///
    /// A damaged offset breaks the links on both sides of its group; a damaged width only the link after it. So a
    /// broken link after `g` is `g`'s fault when the link before `g` is broken too, when `g` is the last group, or
    /// when the link after `g + 1` holds; otherwise `g + 1`'s offset is the damaged one. A good neighbour of a
    /// damaged group stays good.
    pub(crate) fn group_fits(&self, g: usize) -> bool {
        if self.end(g).is_none() {
            return false;
        }
        self.ends_at_next(g)
            || (self.starts_at_previous(g) && g + 1 < self.count() && !self.ends_at_next(g + 1))
    }

    /// Group `g`'s values from their first byte on; only for a group that [Self::check] or [Self::group_fits]
    /// passed.
    pub(crate) fn values(&self, g: usize) -> &'b [u8] {
        &self.data[self.raw_offset(g)..]
    }

    fn corrupt(&self, what: &'static str) -> TimesError {
        TimesError::Corrupt {
            kind: self.kind,
            what,
        }
    }
}
