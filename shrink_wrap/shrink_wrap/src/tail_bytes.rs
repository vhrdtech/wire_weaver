use crate::{BufReader, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};
use core::fmt::Debug;
use core::ops::Deref;

/// A Vec<u8> wrapper that consumes all remaining bytes in a buffer when deserializing.
///
/// Used in dynamic API calls to represent function return types, property values, etc.
///
/// In generated code, byte slices are automatically replaced with this type.
/// It yields two small optimizations:
/// * byte slices are sent directly without serialization/deserialization
/// * slice size is not encoded nor sent over the wire (full request/event length is known anyway)
///
/// WARNING: Only use this type when it is last in a type.
/// WARNING: If type is not Unsized, last means last in the first Unsized type on the way up.
pub struct TailBytes<'i>(pub &'i [u8]);

impl SerializeShrinkWrap for TailBytes<'_> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap(&self, wr: &mut crate::prelude::BufWriter) -> Result<(), Error> {
        wr.write_raw_slice(self.0)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for TailBytes<'i> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(TailBytes(rd.read_raw_slice(rd.bytes_left())?))
    }
}

impl Debug for TailBytes<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "tail({}:{:02x?})", self.0.len(), self.0)
    }
}

impl<'i> TailBytes<'i> {
    pub fn as_slice(&self) -> &'i [u8] {
        self.0
    }
}

impl Deref for TailBytes<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

/// See [TailBytes] for documentation.
#[cfg(feature = "std")]
#[derive(Clone, Debug)]
pub struct TailBytesOwned(pub Vec<u8>);

#[cfg(feature = "std")]
impl crate::DeserializeShrinkWrapOwned for TailBytesOwned {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        Ok(TailBytesOwned(rd.read_raw_slice(rd.bytes_left())?.to_vec()))
    }
}

#[cfg(feature = "std")]
impl<'i> DeserializeShrinkWrap<'i> for TailBytesOwned {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(TailBytesOwned(rd.read_raw_slice(rd.bytes_left())?.to_vec()))
    }
}

#[cfg(feature = "std")]
impl SerializeShrinkWrap for TailBytesOwned {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap(&self, wr: &mut crate::prelude::BufWriter) -> Result<(), Error> {
        wr.write_raw_slice(self.0.as_slice())
    }
}

#[cfg(feature = "std")]
impl Deref for TailBytesOwned {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(feature = "std")]
impl TailBytes<'_> {
    pub fn make_owned(&self) -> TailBytesOwned {
        TailBytesOwned(self.0.to_vec())
    }
}
