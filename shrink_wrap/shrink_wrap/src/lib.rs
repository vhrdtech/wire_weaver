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
pub mod vlq64;
pub use crate::vlq64::UVlq64;
pub mod max_len;
pub use max_len::{ItemMaxSize, LenBounded, MaxLen};
pub mod max_size;
pub use max_size::{MaxSize, write_max_size};
pub mod ref_box;
pub use ref_box::RefBox;
pub mod ref_vec;
pub use ref_vec::{RefVec, RefVecIter};
pub mod either_any_vec;
pub mod fixed_layout;
pub mod head_tail;
pub mod option_any_vec;
#[cfg(feature = "std")]
pub use option_any_vec::OptionAnyVecBuilderOwned;
pub use option_any_vec::{OptionAnyVec, OptionAnyVecBuilder};
pub mod traits;
pub use shrink_wrap_derive::{derive_shrink_wrap, ww_repr};
#[cfg(feature = "std")]
pub use traits::SerializeShrinkWrapOwned;
pub use traits::{
    DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, SerializeShrinkWrap,
    write_pushes_len,
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
#[repr(u32)]
pub enum Error {
    OutOfBoundsWriteBool = 1,
    OutOfBoundsReadBool,
    OutOfBoundsWriteU4,
    OutOfBoundsReadU4,
    OutOfBoundsWriteU8,
    OutOfBoundsReadU8,
    OutOfBoundsWriteRawSlice,
    OutOfBoundsReadRawSlice,
    OutOfBoundsWriteUN,
    OutOfBoundsReadUN,
    OutOfBoundsSplit,
    OutOfBoundsRev,
    OutOfBoundsRevCompact,
    InternalSliceToArrayCast,
    MalformedUNib32,
    MalformedUVlq32,
    MalformedUVlq64,
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
    /// [BufWriter::finish_no_lens] on a writer that has lengths to encode: the type's
    /// [PUSHES_LEN](SerializeShrinkWrap::PUSHES_LEN) is `false`, but its `ser_shrink_wrap` pushed one.
    LenFifoNotEmpty,
    /// The enum variant is known, but compiled out of this build: it is behind a `#[cfg(..)]` (usually a cargo
    /// feature) that is off here. The other side has it enabled.
    EnumVariantDisabled,
    /// A [MaxLen] list or string is longer than its maximum: when serializing or deserializing it, or in
    /// [MaxLen::new]. A longer value is never cut.
    MaxLenExceeded,
    /// The presence header of an [OptionAnyVec] cannot have been written by the
    /// writer: a count or length past the end of the data, a slot id past `u32::MAX`, an empty group in the
    /// two-level mode.
    MalformedOptionAnyVec,
    /// A value written for a slot of an [OptionAnyVec] that is not the next present
    /// one, a present slot left without a value, or present slot ids that don't increase.
    OptionAnyVecWrongSlot,
    /// A present slot of an [OptionAnyVec] that the reader has no type for, before
    /// a slot it reads: a gap between the fields of a `sparse` struct (DER-13). Its size is unknown, so the slots
    /// after it can't be found. Slots are only ever appended, so the writer is not a compatible version.
    OptionAnyVecUnknownSlot,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl Error {
    /// Whether a read ran out of data: past the end of the buffer, or a length or split that does not fit.
    ///
    /// A field with `#[default = ..]` reads as its default when it starts at the end of the data (written by an
    /// older version) and its read fails with one of these, any other error is reported (SW-15).
    pub const fn is_read_eob(&self) -> bool {
        use Error::*;
        matches!(
            self,
            OutOfBoundsReadBool
                | OutOfBoundsReadU4
                | OutOfBoundsReadU8
                | OutOfBoundsReadRawSlice
                | OutOfBoundsReadUN
                | OutOfBoundsSplit
                | OutOfBoundsRev
        )
    }
}

pub mod prelude {
    pub use crate::Error as ShrinkWrapError;
    pub use crate::any_on_stack::AnyOnStack;
    pub use crate::buf_reader::BufReader;
    pub use crate::buf_writer::BufWriter;
    #[cfg(feature = "std")]
    pub use crate::buf_writer_owned::BufWriterOwned;
    pub use crate::fixed_layout::FixedLayout;
    pub use crate::head_tail::HeadTail;
    pub use crate::max_len::MaxLen;
    pub use crate::max_size::MaxSize;
    pub use crate::nib::Nibble;
    pub use crate::nib32::UNib32;
    #[cfg(feature = "std")]
    pub use crate::option_any_vec::OptionAnyVecBuilderOwned;
    pub use crate::option_any_vec::{OptionAnyVec, OptionAnyVecBuilder};
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
    pub use crate::vlq64::UVlq64;
    pub use shrink_wrap_derive::{ShrinkWrap, derive_shrink_wrap};
}
