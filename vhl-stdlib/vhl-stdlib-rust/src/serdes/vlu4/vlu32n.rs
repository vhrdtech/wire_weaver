use crate::serdes::nibble_buf::Error as NibbleBufError;
use crate::serdes::traits::SerializeVlu4;
use crate::serdes::{DeserializeVlu4, NibbleBuf, NibbleBufMut, SerDesSize};

/// Variable length encoded u32 based on nibbles.
/// Each nibbles carries 1 bit indicating whether there are more nibbles + 3 bits from the original number.
/// Bit order is Big Endian.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Vlu32N(pub u32);

/// Used for in-place buffer writing without prior knowledge of it's length.
/// Originally bigger number is written (representing remaining space or max bound of SerDesSize).
/// Afterwards original number is updated with actual size, and if it is smaller than original,
/// `additional_empty_nibbles` = 0b1000 are written if needed,  since copying is undesired.
pub struct Vlu32Suboptimal {
    pub additional_empty_nibbles: usize,
    pub value: u32,
}

impl Vlu32N {
    pub fn len_nibbles_known_to_be_sized(&self) -> usize {
        match self.0 {
            0..=7 => 1,
            8..=63 => 2,
            64..=511 => 3,
            512..=4095 => 4,
            4096..=32_767 => 5,
            32_768..=262_143 => 6,
            262_144..=2_097_151 => 7,
            2_097_152..=16_777_215 => 8,
            16_777_216..=134_217_727 => 9,
            134_217_728..=1_073_741_823 => 10,
            _ => 11,
        }
    }
}

impl SerializeVlu4 for Vlu32N {
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

impl<'i> DeserializeVlu4<'i> for Vlu32N {
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
        Ok(Vlu32N(num))
    }
}

impl SerializeVlu4 for Vlu32Suboptimal {
    type Error = NibbleBufError;

    fn ser_vlu4(&self, nwr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        for _ in 0..self.additional_empty_nibbles {
            nwr.put_nibble(0b1000)?;
        }
        Vlu32N(self.value).ser_vlu4(nwr)
    }

    fn len_nibbles(&self) -> SerDesSize {
        SerDesSize::Sized(
            Vlu32N(self.value).len_nibbles_known_to_be_sized() + self.additional_empty_nibbles,
        )
    }
}

macro_rules! serialize_unsigned {
    ($ty:ty) => {
        impl SerializeVlu4 for $ty {
            type Error = NibbleBufError;

            fn ser_vlu4(&self, nwr: &mut NibbleBufMut) -> Result<(), Self::Error> {
                nwr.put(&Vlu32N(*self as u32))?;
                Ok(())
            }

            fn len_nibbles(&self) -> SerDesSize {
                Vlu32N(*self as u32).len_nibbles()
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
                let num: Vlu32N = nrd.des_vlu4()?;
                Ok(num.0 as $ty)
            }
        }
    };
}
// deserialize_unsigned!(u8, get_u8);
// deserialize_unsigned!(u16, get_u16_be);
deserialize_unsigned!(u32, get_u32_be);

#[cfg(test)]
mod test {
    extern crate std;

    use crate::serdes::{NibbleBuf, NibbleBufMut};
    use crate::serdes::nibble_buf::Error;

    #[test]
    fn read_vlu4_u32_single_nibble() {
        let buf = [0b0111_0010, 0b0000_0001];
        let mut rdr = NibbleBuf::new_all(&buf);
        assert_eq!(rdr.get_vlu4_u32(), Ok(7));
        assert_eq!(rdr.get_vlu4_u32(), Ok(2));
        assert_eq!(rdr.get_vlu4_u32(), Ok(0));
        assert_eq!(rdr.get_vlu4_u32(), Ok(1));
        assert!(rdr.is_at_end());
    }

    #[test]
    fn read_vlu4_u32_multi_nibble() {
        let buf = [0b1111_0111, 0b1001_0000, 0b1000_0111];
        let mut rdr = NibbleBuf::new_all(&buf);
        assert_eq!(rdr.get_vlu4_u32(), Ok(63));
        assert_eq!(rdr.nibbles_pos(), 2);
        assert_eq!(rdr.get_vlu4_u32(), Ok(0b001000));
        assert_eq!(rdr.nibbles_pos(), 4);
        assert_eq!(rdr.get_vlu4_u32(), Ok(0b111));
        assert!(rdr.is_at_end());
    }

    #[test]
    fn read_vlu4_u32_max() {
        let buf = [0b1011_1111, 0xff, 0xff, 0xff, 0xff, 0x70];
        let mut rdr = NibbleBuf::new_all(&buf);
        assert_eq!(rdr.get_vlu4_u32(), Ok(u32::MAX));
        assert_eq!(rdr.get_nibble(), Ok(0));
        assert!(rdr.is_at_end());
    }

    #[test]
    fn read_vlu4_u32_max_plus1() {
        // ignore bit 33
        let buf = [0b1111_1111, 0xff, 0xff, 0xff, 0xff, 0x70];
        let mut rdr = NibbleBuf::new_all(&buf);
        assert_eq!(rdr.get_vlu4_u32(), Ok(u32::MAX));
        assert_eq!(rdr.get_nibble(), Ok(0));
        assert!(rdr.is_at_end());
    }

    #[test]
    fn read_vlu4_u32_max_plus_nibble() {
        // more nibbles than expected for u32
        let buf = [0xff, 0xff, 0xff, 0xff, 0xff, 0xf0];
        let mut rdr = NibbleBuf::new_all(&buf);
        assert_eq!(rdr.get_vlu4_u32(), Err(Error::MalformedVlu4U32));
        assert_eq!(rdr.nibbles_left(), 1);
    }

    #[test]
    fn write_vlu4_u32_3() {
        let mut buf = [0u8; 4];
        let mut wgr = NibbleBufMut::new_all(&mut buf);
        wgr.put_vlu4_u32(3).unwrap();
        assert_eq!(wgr.nibbles_pos(), 1);
        assert_eq!(buf[0], 0b0011_0000);
    }

    // ≈ 1.5M/s on Core i7 8700K
    // ≈ 47min to complete on all 32 bit numbers
    #[test]
    fn round_trip_vlu4_u32() {
        let mut buf = [0u8; 11];
        let numbers = [0, 7, 8, 63, 64, 511, 512, u32::MAX - 1, u32::MAX];
        for i in numbers {
            {
                let mut wgr = NibbleBufMut::new_all(&mut buf);
                wgr.put_vlu4_u32(i).unwrap();
                assert!(!wgr.is_at_end());
            }
            // if i % 10_000_000 == 0 {
            //     println!("{}", i);
            //     std::io::stdout().flush().unwrap();
            // }

            let mut rgr = NibbleBuf::new_all(&mut buf);
            assert_eq!(rgr.get_vlu4_u32(), Ok(i));
        }
    }
}