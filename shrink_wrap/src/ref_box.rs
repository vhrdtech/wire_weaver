use crate::{BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};
use std::fmt::{Debug, Formatter};

/// Box-like structure for no alloc use, serializes and deserializes the value as WireWeaver's Unsized.
/// Can be used to create self-referential structs and enums on no_std and no alloc systems.
///
/// RefBox is cheap, both in terms of serialized size (only one additional size) and compute (no additional work is performed).
/// Additionally, when RefBox is deserialized from the BufReader, a jump over the whole item is made.
/// Value is deserialized only when [read()](RefBox::read) is called.
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RefBox<'i, T> {
    Ref { value: &'i T },
    Buf { buf: BufReader<'i> },
}

impl<'i, T> RefBox<'i, T>
where
    T: SerializeShrinkWrap + DeserializeShrinkWrap<'i> + Clone,
{
    /// Create RefBox::Ref variant from the provided value
    pub fn new(value: &'i T) -> Self {
        RefBox::Ref { value }
    }

    /// Return the RefBox::Ref value clone or deserialize the value from RefBox::Buf and return it
    pub fn read(&self) -> Result<T, Error> {
        match self {
            RefBox::Ref { value } => Ok((*value).clone()),
            RefBox::Buf { buf } => {
                let mut rd = *buf;
                // Note the use of des_shrink_wrap instead of read here, this is intentional as RefBox is already Unsized
                let item = T::des_shrink_wrap(&mut rd)?;
                Ok(item)
            }
        }
    }
}

impl<'i, T> SerializeShrinkWrap for RefBox<'i, T>
where
    T: SerializeShrinkWrap + DeserializeShrinkWrap<'i>,
{
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            RefBox::Ref { value } => {
                value.ser_shrink_wrap(wr)?;
            }
            RefBox::Buf { buf } => {
                let mut rd = *buf;
                let value = T::des_shrink_wrap(&mut rd)?;
                value.ser_shrink_wrap(wr)?;
            }
        }
        Ok(())
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for RefBox<'i, T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        // save BufReader state
        let buf = *rd;
        // Save the buffer and do nothing, parent deserializer will skip over since RefBox is Unsized.
        // When read is called, actual deserialization will take place.
        Ok(Self::Buf { buf })
    }
}

impl<'i, T: Debug> Debug for RefBox<'i, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
