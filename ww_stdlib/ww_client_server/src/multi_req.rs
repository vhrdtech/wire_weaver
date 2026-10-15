use core::ops::Range;

use wire_weaver::shrink_wrap::{RefVecIter, UNib32};

use crate::MultiIndex;

impl MultiIndex<'_> {
    /// Indices this multi-request covers, in order. A `Range` with `start >= end` and an empty `Mask32` cover none,
    /// a `List` stops at its first element that fails to deserialize.
    pub fn iter(&self) -> MultiIndexIter<'_> {
        match self {
            MultiIndex::Range(range) => {
                let range = range.start.0..range.end.0;
                MultiIndexIter::Range { range }
            }
            MultiIndex::List(ref_vec) => MultiIndexIter::List {
                list: ref_vec.iter(),
            },
            MultiIndex::Mask32(mask) => MultiIndexIter::Mask32 {
                mask: *mask,
                pos: 0,
            },
        }
    }
}

pub enum MultiIndexIter<'i> {
    // All,
    Range {
        range: Range<u32>,
    },
    List {
        list: RefVecIter<'i, UNib32>,
    },
    /// Set bits of `mask` from bit `pos` up, lowest first
    Mask32 {
        mask: u32,
        pos: usize,
    },
}

impl<'i> Iterator for MultiIndexIter<'i> {
    type Item = UNib32;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            MultiIndexIter::Range { range } => range.next().map(UNib32),
            MultiIndexIter::List { list } => {
                if let Some(Ok(id)) = list.next() {
                    Some(id)
                } else {
                    None
                }
            }
            MultiIndexIter::Mask32 { mask, pos } => {
                while *pos < 32 {
                    let bit = *pos as u32;
                    *pos += 1;
                    if *mask & (1 << bit) != 0 {
                        return Some(UNib32(bit));
                    }
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire_weaver::shrink_wrap::RefVec;

    fn collect(idx: MultiIndex<'_>) -> Vec<u32> {
        idx.iter().map(|i| i.0).collect()
    }

    #[test]
    fn range() {
        assert_eq!(collect(MultiIndex::Range(UNib32(2)..UNib32(5))), [2, 3, 4]);
        assert!(collect(MultiIndex::Range(UNib32(5)..UNib32(5))).is_empty());
        assert!(collect(MultiIndex::Range(UNib32(7)..UNib32(3))).is_empty());
        let mut it = MultiIndex::Range(UNib32(u32::MAX - 1)..UNib32(u32::MAX)).iter();
        assert_eq!(it.next(), Some(UNib32(u32::MAX - 1)));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn mask32() {
        assert!(collect(MultiIndex::Mask32(0)).is_empty());
        assert_eq!(collect(MultiIndex::Mask32(0b1010_0001)), [0, 5, 7]);
        assert_eq!(collect(MultiIndex::Mask32(1 << 31)), [31]);
        assert_eq!(
            collect(MultiIndex::Mask32(u32::MAX)),
            (0..32).collect::<Vec<_>>()
        );
    }

    #[test]
    fn list() {
        let list = [UNib32(3), UNib32(1)];
        assert_eq!(
            collect(MultiIndex::List(RefVec::Slice { slice: &list })),
            [3, 1]
        );
    }
}
