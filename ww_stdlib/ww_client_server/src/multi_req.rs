use core::range::Range;

use wire_weaver::shrink_wrap::{RefVecIter, UNib32};

use crate::MultiIndex;

impl MultiIndex<'_> {
    pub fn iter(&self) -> MultiIndexIter<'_> {
        todo!()
    }
}

pub enum MultiIndexIter<'i> {
    All,
    Range { range: Range<u32>, pos: u32 },
    List { list: RefVecIter<'i, u32> },
    Mask32 { mask: u32, pos: usize },
}

impl<'i> Iterator for MultiIndexIter<'i> {
    type Item = UNib32;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}
