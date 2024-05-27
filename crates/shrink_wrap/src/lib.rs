// #![no_std]

pub mod buf_reader;
pub mod buf_writer;
pub mod traits;
mod vec;
pub(crate) mod vlu16n;

pub use buf_reader::BufReader;
pub use buf_writer::BufWriter;
pub use traits::{DeserializeShrinkWrap, SerializeShrinkWrap};

#[derive(Debug, Eq, PartialEq)]
pub enum Error {
    OutOfBounds,
    OutOfBoundsRev,
    OutOfBoundsRevCompact,
    MalformedVlu16N,
    MalformedLeb,
    MalformedUtf8,
    StrTooLong,
    EnumFutureVersionOrMalformedData,
    ImpliedSizeInVec,
}
