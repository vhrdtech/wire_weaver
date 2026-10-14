//! Bounded lists and strings (`MaxLen`, SW-19) in every position of an API.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

/// Largest piece `write` takes, the bound is read from this const.
pub const MAX_PIECE: usize = 8;

#[ww_trait]
trait Bounded {
    /// Stores a piece, returns its length
    fn write(offset: u32, last: bool, data: MaxLen<RefVec<'i, u8>, MAX_PIECE>) -> u8;
    /// Returns `len` bytes of a name, longer than 4 fails to serialize
    fn name(len: u8) -> MaxLen<&'i str, 4>;
    /// A list of bounded strings, returns how many
    fn tags(tags: MaxLen<RefVec<'i, MaxLen<&'i str, 3>>, 2>) -> u8;
    /// A struct with a bounded field
    fn piece(piece: Piece<'i>) -> u32;
    /// No bound on the argument
    fn unbounded(data: RefVec<'i, u8>);
    property!(rw label: MaxLen<&'i str, 6>);
    stream!(chunks: MaxLen<RefVec<'i, u8>, 4>);
    sink!(blocks: MaxLen<RefVec<'i, u8>, 4>);
    /// Sends `len` bytes to `chunks`, returns whether that worked
    fn emit_chunk(len: u8) -> bool;
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Piece<'i> {
    pub offset: u32,
    pub data: MaxLen<RefVec<'i, u8>, 3>,
}
