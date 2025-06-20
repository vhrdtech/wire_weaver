#![cfg_attr(not(feature = "std"), no_std)]
//#![cfg_attr(all(not(feature = "std"), not(test)), no_std)] ?

use crate::nib32::UNib32;
pub use buf_reader::BufReader;
pub use buf_writer::BufWriter;
pub use traits::{DeserializeShrinkWrap, ElementSize, SerializeShrinkWrap};

pub mod buf_reader;
pub mod buf_writer;
pub mod nib32;
pub mod ref_box;
pub mod ref_vec;
pub mod traits;

#[cfg(feature = "std")]
pub mod alloc;
pub mod un;

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
    pub use crate::buf_reader::BufReader;
    pub use crate::buf_writer::BufWriter;
    pub use crate::nib32::UNib32;
    pub use crate::ref_box::RefBox;
    pub use crate::ref_vec::{RefVec, RefVecIter};
    pub use crate::traits::{DeserializeShrinkWrap, ElementSize, SerializeShrinkWrap};
    pub use crate::un::*;
    pub use crate::Error as ShrinkWrapError;
}
