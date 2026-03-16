use crate::{BufReader, DeserializeShrinkWrap, ElementSize, Error};

/// A Vec<u8> wrapper that consumes all remaining bytes in a buffer when deserializing.
///
/// Used in dynamic API calls to represent function return types, property values, etc.
///
/// In generated code, byte slices are automatically replaced with this type.
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
#[derive(Clone, Debug)]
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
