use crate::{BufReader, DeserializeShrinkWrap, ElementSize, Error};

/// Special type that is used in generated code, automatically replacing byte slices in streams.
/// It yields two small optimizations:
/// * byte slices are sent directly without serialization/deserialization
/// * slice size is not encoded nor sent over the wire (stream data length is known anyway)
///
/// Do not use this type directly in data types, instead use `RefVec<'i, u8>`.
pub struct RawSlice<'i>(pub &'i [u8]);

// Intentionally not implementing SerializeShrinkWrap

impl<'i> DeserializeShrinkWrap<'i> for RawSlice<'i> {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(RawSlice(rd.read_raw_slice(rd.bytes_left())?))
    }
}

/// See [RawSlice] for documentation.
#[cfg(feature = "std")]
pub struct RawSliceOwned(pub Vec<u8>);

#[cfg(feature = "std")]
impl crate::DeserializeShrinkWrapOwned for RawSliceOwned {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        Ok(RawSliceOwned(rd.read_raw_slice(rd.bytes_left())?.to_vec()))
    }
}

#[cfg(feature = "std")]
impl std::ops::Deref for RawSliceOwned {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
