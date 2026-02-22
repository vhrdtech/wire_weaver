use crate::{
    BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, Error,
    SerializeShrinkWrap,
};

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

impl DeserializeShrinkWrapOwned for Nibble {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 4 };

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        Ok(Nibble(rd.read_u4()?))
    }
}

#[cfg(test)]
mod tests {
    use crate::{BufReader, BufWriter, DeserializeShrinkWrapOwned, Nibble};

    #[test]
    fn sanity_check() {
        assert_eq!(Nibble::new(7), Some(Nibble::new_masked(7)));
        assert_eq!(Nibble::zero().value(), 0);
        assert_eq!(Nibble::one().value(), 1);
        assert_eq!(Nibble::max().value(), 15);
        assert_eq!(Nibble::new_masked(0xFF).0, 0xF);
        assert_eq!(Nibble::new(0xFF), None);
    }

    #[test]
    fn serdes() {
        let mut buf = [0u8; 1];
        let mut wr = BufWriter::new(&mut buf[..]);
        wr.write(&Nibble::one()).unwrap();
        wr.write(&Nibble::max()).unwrap();
        let bytes = wr.finish().unwrap();

        let mut rd = BufReader::new(bytes);
        let one: Nibble = rd.read().unwrap();
        let f: Nibble = rd.read().unwrap();
        assert_eq!(one.value(), 1);
        assert_eq!(f.value(), 0xF);

        let mut rd = BufReader::new(bytes);
        let one = Nibble::des_shrink_wrap_owned(&mut rd).unwrap();
        assert_eq!(one.value(), 1);
    }
}
