use crate::serdes::{DeserializeVlu4, NibbleBuf, NibbleBufMut};
use crate::serdes::traits::SerializeVlu4;
use crate::serdes::nibble_buf::Error as NibbleBufError;

#[derive(Debug, Copy, Clone, PartialEq, Eq, )]
pub struct Vlu32(pub u32);

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

    fn len_nibbles(&self) -> usize {
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
            num = num | (nib as u32 & 0b111);
            if nib & 0b1000 == 0 {
                break;
            }
            num = num << 3;
        }
        Ok(Vlu32(num))
    }
}