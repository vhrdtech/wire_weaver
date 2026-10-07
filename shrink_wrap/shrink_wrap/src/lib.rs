#![cfg_attr(not(feature = "std"), no_std)]
//#![cfg_attr(all(not(feature = "std"), not(test)), no_std)] ?

// So that the code `#[derive_shrink_wrap(..)]` generates can name this crate as `::shrink_wrap` from inside it too.
extern crate self as shrink_wrap;

pub mod buf_reader;

pub use buf_reader::BufReader;
use core::fmt::{Display, Formatter};
pub mod buf_writer;
pub use buf_writer::BufWriter;
#[cfg(feature = "std")]
pub mod buf_writer_owned;
#[cfg(feature = "std")]
pub use buf_writer_owned::BufWriterOwned;
pub mod nib32;
pub use crate::nib32::UNib32;
pub mod vlq32;
pub use crate::vlq32::{UVlq32, UVlq32Backfill};
pub mod ref_box;
pub use ref_box::RefBox;
pub mod ref_vec;
pub use ref_vec::{RefVec, RefVecIter};
pub mod either_any_vec;
pub mod traits;
pub use shrink_wrap_derive::{derive_shrink_wrap, ww_repr};
#[cfg(feature = "std")]
pub use traits::SerializeShrinkWrapOwned;
pub use traits::{
    DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, SerializeShrinkWrap,
};

#[cfg(feature = "std")]
pub mod alloc;
pub mod any_on_stack;
pub mod nib;
pub mod series;
pub mod tail_bytes;
pub use series::{Delta, DeltaOfDelta, XorFloat};
#[cfg(feature = "std")]
pub use series::{DeltaOfDeltaOwned, DeltaOwned, XorFloatOwned};
pub mod tail_size;
pub use tail_size::TailSize;
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
    MalformedUVlq32,
    MalformedLeb,
    MalformedUtf8,
    LenTooLong,
    EnumFutureVersionOrMalformedData,
    InvalidBitCount,
    SubtypeOutOfRange,
    /// A [Delta] / [DeltaOfDelta] / [XorFloat] sequence that
    /// cannot have been written by the encoder: more elements than bits, or a code that refers to state the
    /// decoder does not have.
    MalformedSeries,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

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
    pub use crate::any_on_stack::AnyOnStack;
    pub use crate::buf_reader::BufReader;
    pub use crate::buf_writer::BufWriter;
    #[cfg(feature = "std")]
    pub use crate::buf_writer_owned::BufWriterOwned;
    pub use crate::nib::Nibble;
    pub use crate::nib32::UNib32;
    pub use crate::ref_box::RefBox;
    pub use crate::ref_vec::{RefVec, RefVecIter};
    pub use crate::series::{Delta, DeltaOfDelta, XorFloat};
    #[cfg(feature = "std")]
    pub use crate::series::{DeltaOfDeltaOwned, DeltaOwned, XorFloatOwned};
    pub use crate::tail_size::TailSize;
    #[cfg(feature = "std")]
    pub use crate::traits::SerializeShrinkWrapOwned;
    pub use crate::traits::{
        DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, SerializeShrinkWrap,
    };
    pub use crate::un::*;
    pub use crate::vlq32::{UVlq32, UVlq32Backfill};
    pub use shrink_wrap_derive::{ShrinkWrap, derive_shrink_wrap};
}
