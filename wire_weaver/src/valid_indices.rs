use core::ops::Range;

use shrink_wrap::prelude::*;

/// List or range of available indices that can be used by a client.
/// For each array of traits, streams, or properties, ValidIndices are provided by a user implementation on the server side.
#[derive_shrink_wrap(ww_repr = u2, derive(Clone, Debug), owned(feature = "std"), cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)))]
pub enum ValidIndices<'i> {
    Range(Range<UNib32>),
    List(RefVec<'i, UNib32>),
}

impl ValidIndices<'_> {
    pub fn contains(&self, index: u32) -> bool {
        let index = UNib32(index);
        match self {
            ValidIndices::Range(range) => range.contains(&index),
            ValidIndices::List(list) => list.iter().any(|i| i == Ok(index)),
        }
    }

    pub fn range_u32(range: Range<u32>) -> Self {
        Self::Range(UNib32(range.start)..UNib32(range.end))
    }
}

#[cfg(feature = "std")]
impl ValidIndicesOwned {
    pub fn contains(&self, index: u32) -> bool {
        let index = UNib32(index);
        match self {
            ValidIndicesOwned::Range(range) => range.contains(&index),
            ValidIndicesOwned::List(list) => list.contains(&index),
        }
    }

    pub fn iter(&self) -> ValidIndicesOwnedIter<'_> {
        match self {
            ValidIndicesOwned::Range(range) => {
                ValidIndicesOwnedIter::Range(range.start.0..range.end.0)
            }
            ValidIndicesOwned::List(list) => ValidIndicesOwnedIter::List(list.iter()),
        }
    }
}

#[cfg(feature = "std")]
pub enum ValidIndicesOwnedIter<'i> {
    Range(core::ops::Range<u32>),
    List(core::slice::Iter<'i, UNib32>),
}

#[cfg(feature = "std")]
impl<'i> Iterator for ValidIndicesOwnedIter<'i> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            ValidIndicesOwnedIter::Range(range) => range.next(),
            ValidIndicesOwnedIter::List(list) => list.next().map(|i| i.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use shrink_wrap::UNib32;

    use crate::ValidIndicesOwned;

    #[test]
    fn valid_indices_empty() {
        let empty = ValidIndicesOwned::Range(UNib32(0)..UNib32(0));
        let mut iter = empty.iter();
        assert_eq!(iter.next(), None);

        let empty = ValidIndicesOwned::List(vec![]);
        let mut iter = empty.iter();
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn valid_indices_list() {
        let list = ValidIndicesOwned::List(vec![UNib32(0), UNib32(1), UNib32(2)]);
        let mut iter = list.iter();
        assert_eq!(iter.next(), Some(0));
        assert_eq!(iter.next(), Some(1));
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn valid_indices_range() {
        let range = ValidIndicesOwned::Range(UNib32(0)..UNib32(3));
        let mut iter = range.iter();
        assert_eq!(iter.next(), Some(0));
        assert_eq!(iter.next(), Some(1));
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next(), None);
    }
}
