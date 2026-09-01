use core::ops::Range;

use wire_weaver::shrink_wrap::{RefVecIter, UNib32};

use crate::MultiIndex;

impl MultiIndex<'_> {
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
    Range { range: Range<u32> },
    List { list: RefVecIter<'i, UNib32> },
    Mask32 { mask: u32, pos: usize },
}

impl<'i> Iterator for MultiIndexIter<'i> {
    type Item = UNib32;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            MultiIndexIter::Range { .. } => todo!(),
            MultiIndexIter::List { list } => {
                if let Some(Ok(id)) = list.next() {
                    Some(id)
                } else {
                    None
                }
            }
            MultiIndexIter::Mask32 { .. } => todo!(),
        }
    }
}
