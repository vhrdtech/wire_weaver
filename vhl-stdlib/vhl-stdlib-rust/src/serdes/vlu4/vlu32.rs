use crate::serdes::nibble_buf::Error as NibbleBufError;
use crate::serdes::traits::SerializeVlu4;
use crate::serdes::{DeserializeVlu4, NibbleBuf, NibbleBufMut, SerDesSize};

/// Variable length encoded u32 based on nibbles.
/// Each nibbles carries 1 bit indicating whether there are more nibbles + 3 bits from the original number.
/// Bit order is Big Endian.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Vlu32(pub u32);

/// Used for in-place buffer writing without prior knowledge of it's length.
/// Originally bigger number is written (representing remaining space or max bound of SerDesSize).
/// Afterwards original number is updated with actual size, and if it is smaller than original,
/// `additional_empty_nibbles` = 0b1000 are written if needed,  since copying is undesired.
pub struct Vlu32Suboptimal {
    pub additional_empty_nibbles: usize,
    pub value: u32,
}

impl Vlu32 {
    pub fn len_nibbles_known_to_be_sized(&self) -> usize {
        match self.0 {
            0..=7 => 1,
            8..=63 => 2,
            64..=511 => 3,
            512..=4095 => 4,
            4096..=32767 => 5,
            32768..=262143 => 6,
            262144..=2097151 => 7,
            2097152..=16777215 => 8,
            16777216..=134217727 => 9,
            134217728..=1073741823 => 10,
            1073741824..=4294967295 => 11,
        }
    }
}

impl SerializeVlu4 for Vlu32 {
    type Error = NibbleBufError;

    fn ser_vlu4(&self, nwr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        let mut val = self.0;
        let mut msb_found = false;
        let nib = (val >> 30) as u8; // get bits 31:30
        if nib != 0 {
            nwr.put_nibble(nib | 0b1000)?;
            msb_found = true;
        }
        val <<= 2;
        for i in 0..=9 {
            if (val & (7 << 29) != 0) || msb_found {
                let nib = (val >> 29) as u8;
                if i == 9 {
                    nwr.put_nibble(nib)?;
                } else {
                    nwr.put_nibble(nib | 0b1000)?;
                }
                msb_found = true;
            }
            if i == 9 && !msb_found {
                nwr.put_nibble(0)?;
            }
            val <<= 3;
        }
        Ok(())
    }

    fn len_nibbles(&self) -> SerDesSize {
        let nibbles = self.len_nibbles_known_to_be_sized();
        SerDesSize::Sized(nibbles)
    }
}

impl<'i> DeserializeVlu4<'i> for Vlu32 {
    type Error = NibbleBufError;

    fn des_vlu4<'di>(nrd: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let mut num = 0;
        for i in 0..=10 {
            let nib = nrd.get_nibble()?;
            if i == 10 {
                // maximum 32 bits in 11 nibbles, 11th nibble should be the last
                if nib & 0b1000 != 0 {
                    return Err(NibbleBufError::MalformedVlu4U32);
                }
            }
            num |= nib as u32 & 0b111;
            if nib & 0b1000 == 0 {
                break;
            }
            num <<= 3;
        }
        Ok(Vlu32(num))
    }
}

impl SerializeVlu4 for Vlu32Suboptimal {
    type Error = NibbleBufError;

    fn ser_vlu4(&self, nwr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        for _ in 0..self.additional_empty_nibbles {
            nwr.put_nibble(0b1000)?;
        }
        Vlu32(self.value).ser_vlu4(nwr)
    }

    fn len_nibbles(&self) -> SerDesSize {
        SerDesSize::Sized(
            Vlu32(self.value).len_nibbles_known_to_be_sized() + self.additional_empty_nibbles,
        )
    }
}

macro_rules! serialize_unsigned {
    ($ty:ty) => {
        impl SerializeVlu4 for $ty {
            type Error = NibbleBufError;

            fn ser_vlu4(&self, nwr: &mut NibbleBufMut) -> Result<(), Self::Error> {
                nwr.put(&Vlu32(*self as u32))?;
                Ok(())
            }

            fn len_nibbles(&self) -> SerDesSize {
                Vlu32(*self as u32).len_nibbles()
            }
        }
    };
}
serialize_unsigned!(u8);
serialize_unsigned!(u16);
serialize_unsigned!(u32);

macro_rules! deserialize_unsigned {
    ($ty:ty, $method:ident) => {
        impl<'i> DeserializeVlu4<'i> for $ty {
            type Error = NibbleBufError;

            fn des_vlu4<'di>(nrd: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
                let num: Vlu32 = nrd.des_vlu4()?;
                Ok(num.0 as $ty)
            }
        }
    };
}
// deserialize_unsigned!(u8, get_u8);
// deserialize_unsigned!(u16, get_u16_be);
deserialize_unsigned!(u32, get_u32_be);
