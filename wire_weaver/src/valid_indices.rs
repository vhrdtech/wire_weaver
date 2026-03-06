use shrink_wrap::prelude::*;

/// List or range of available indices that can be used by a client.
/// For each array of traits, streams, or properties, ValidIndices are provided by a user implementation on the server side.
#[derive_shrink_wrap]
#[ww_repr(u2)]
#[derive(Clone, Debug)]
#[owned = "std"]
#[defmt = "defmt"]
pub enum ValidIndices<'i> {
    Range(Range<u32>),
    List(RefVec<'i, u32>),
}

impl ValidIndices<'_> {
    pub fn contains(&self, index: u32) -> bool {
        match self {
            ValidIndices::Range(range) => range.contains(&index),
            ValidIndices::List(list) => list.iter().any(|i| i == Ok(index)),
        }
    }
}

#[cfg(feature = "std")]
impl ValidIndicesOwned {
    pub fn contains(&self, index: u32) -> bool {
        match self {
            ValidIndicesOwned::Range(range) => range.contains(&index),
            ValidIndicesOwned::List(list) => list.contains(&index),
        }
    }
}
