#![cfg_attr(not(feature = "std"), no_std)]
//#![cfg_attr(all(not(feature = "std"), not(test)), no_std)] ?

use crate::nib32::UNib32;
pub use buf_reader::BufReader;
pub use buf_writer::BufWriter;
pub use traits::{DeserializeShrinkWrap, ElementSize, SerializeShrinkWrap};

pub mod buf_reader;
pub mod buf_writer;
pub mod nib32;
pub mod traits;
pub mod vec;

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
    OutOfBoundsSplit,
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
    ImpliedSizeInVec,
    InvalidBitCount,
    SubtypeOutOfRange,
}

pub mod prelude {
    pub use crate::buf_reader::BufReader;
    pub use crate::buf_writer::BufWriter;
    pub use crate::traits::{DeserializeShrinkWrap, ElementSize, SerializeShrinkWrap};
    pub use crate::un::*;
    pub use crate::vec::RefVec;
    pub use crate::Error as ShrinkWrapError;
}
