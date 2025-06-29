use crate::{BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};

/// 4 bits, serialized with alignment of four-bits.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Nibble(u8);

impl Nibble {
    /// Fallible constructor, returns Some only if x <= 15
    #[inline(always)]
    pub const fn new(x: u8) -> Option<Self> {
        if x <= Self::max().0 {
            Some(Self(x))
        } else {
            None
        }
    }

    /// Infallible constructor, masks bits 7:4
    #[inline(always)]
    pub const fn new_masked(x: u8) -> Self {
        Self(x & 0xF)
    }

    #[inline(always)]
    pub const fn zero() -> Self {
        Self(0)
    }

    #[inline(always)]
    pub const fn one() -> Self {
        Self(1)
    }

    #[inline(always)]
    pub const fn max() -> Self {
        Self(15)
    }

    #[inline(always)]
    pub const fn value(&self) -> u8 {
        self.0
    }
}

impl SerializeShrinkWrap for Nibble {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 4 };

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_u4(self.value())
    }
}

impl<'i> DeserializeShrinkWrap<'i> for Nibble {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 4 };

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(Nibble(rd.read_u4()?))
    }
}
