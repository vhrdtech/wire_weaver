#![cfg_attr(not(feature = "std"), no_std)]
//#![cfg_attr(all(not(feature = "std"), not(test)), no_std)] ?

pub mod buf_reader;
pub use buf_reader::BufReader;
pub mod buf_writer;
pub use buf_writer::BufWriter;
pub mod nib32;
pub use crate::nib32::UNib32;
pub mod ref_box;
pub use ref_box::RefBox;
pub mod ref_vec;
pub use ref_vec::{RefVec, RefVecIter};
pub mod traits;
pub use shrink_wrap_derive::ww_repr;
pub use traits::{
    DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, SerializeShrinkWrap,
};

#[cfg(feature = "std")]
pub mod alloc;
pub mod nib;
pub mod raw_slice;
pub mod stack_vec;
pub mod un;

pub use nib::Nibble;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    OutOfBoundsWriteBool,
    OutOfBoundsReadBool,
    OutOfBoundsWriteU4,
    OutOfBoundsReadU4,
    OutOfBoundsWriteU8,
    OutOfBoundsReadU8,
    OutOfBoundsWriteRawSlice,
    OutOfBoundsReadRawSlice,
    OutOfBoundsWriteUN(UNib32),
    OutOfBoundsReadUN(UNib32),
    OutOfBoundsSplit(UNib32),
    OutOfBoundsRev,
    OutOfBoundsRevCompact,
    InternalSliceToArrayCast,
    MalformedUNib32,
    MalformedLeb,
    MalformedUtf8,
    StrTooLong,
    VecTooLong,
    ItemTooLong,
    EnumFutureVersionOrMalformedData,
    InvalidBitCount,
    SubtypeOutOfRange,
}

// impl Error {
//     pub fn is_read_eob(&self) -> bool {
//         use Error::*;
//         matches!(
//             self,
//             OutOfBoundsReadBool
//                 | OutOfBoundsReadU4
//                 | OutOfBoundsReadU8
//                 | OutOfBoundsReadRawSlice
//                 | OutOfBoundsReadUN(_)
//                 | OutOfBoundsRev
//         )
//     }
// }

pub mod prelude {
    pub use crate::Error as ShrinkWrapError;
    pub use crate::buf_reader::BufReader;
    pub use crate::buf_writer::BufWriter;
    pub use crate::nib::Nibble;
    pub use crate::nib32::UNib32;
    pub use crate::ref_box::RefBox;
    pub use crate::ref_vec::{RefVec, RefVecIter};
    pub use crate::stack_vec::StackVec;
    pub use crate::traits::{
        DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, SerializeShrinkWrap,
    };
    pub use crate::un::*;
    pub use shrink_wrap_derive::{ShrinkWrap, derive_shrink_wrap};
}
