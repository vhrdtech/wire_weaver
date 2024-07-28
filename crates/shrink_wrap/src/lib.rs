#![no_std]

pub use buf_reader::BufReader;
pub use buf_writer::BufWriter;
pub use traits::{DeserializeShrinkWrap, ElementSize, SerializeShrinkWrap};

pub mod buf_reader;
pub mod buf_writer;
pub(crate) mod nib16;
pub mod traits;
pub mod vec;

#[derive(Debug, Eq, PartialEq)]
pub enum Error {
    OutOfBounds,
    OutOfBoundsRev,
    OutOfBoundsRevCompact,
    MalformedVlu16N,
    MalformedLeb,
    MalformedUtf8,
    StrTooLong,
    VecTooLong,
    ItemTooLong,
    EnumFutureVersionOrMalformedData,
    ImpliedSizeInVec,
}
