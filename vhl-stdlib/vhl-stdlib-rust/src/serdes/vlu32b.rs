use crate::serdes::buf::Error as BufError;
use crate::serdes::{Buf, BufMut, DeserializeBytes, SerDesSize, SerializeBytes};

/// Variable length encoded u32 based on bytes.
/// Each byte carries 1 bit indicating whether there are more + 7 bits from the original number.
/// Bit order is Big Endian.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Vlu32B(pub u32);

impl Vlu32B {
    pub fn len_bytes_known_to_be_sized(&self) -> usize {
        match self.0 {
            0..=127 => 1,
            128..=16_383 => 2,
            16_384..=2_097_151 => 3,
            2_097_152..=268_435_455 => 4,
            _ => 5,
        }
    }
}

impl SerializeBytes for Vlu32B {
    type Error = BufError;

    fn ser_bytes(&self, wr: &mut BufMut) -> Result<(), Self::Error> {
        let mut val = self.0;
        let mut msb_found = false;
        let b = (val >> 28) as u8; // get bits 31:28
        if b != 0 {
            wr.put_u8(b | 0b1000_0000)?;
            msb_found = true;
        }
        val <<= 4;
        for i in 0..=3 {
            if (val & (127 << 25) != 0) || msb_found {
                let b = (val >> 25) as u8;
                if i == 3 {
                    wr.put_u8(b)?;
                } else {
                    wr.put_u8(b | 0b1000_0000)?;
                }
                msb_found = true;
            }
            if i == 3 && !msb_found {
                wr.put_u8(0)?;
            }
            val <<= 7;
        }
        Ok(())
    }

    fn len_bytes(&self) -> SerDesSize {
        let bytes = self.len_bytes_known_to_be_sized();
        SerDesSize::Sized(bytes)
    }
}

impl<'i> DeserializeBytes<'i> for Vlu32B {
    type Error = BufError;

    fn des_bytes<'di>(rd: &'di mut Buf<'i>) -> Result<Self, Self::Error> {
        let mut num = 0;
        for i in 0..=4 {
            let b = rd.get_u8()?;
            if i == 4 {
                // maximum 32 bits in 5 bytes, 5th byte should be the last
                if b & 0b1000_0000 != 0 {
                    return Err(BufError::MalformedVlu32B);
                }
            }
            num |= b as u32 & 0b0111_1111;
            if b & 0b1000_0000 == 0 {
                break;
            }
            num <<= 7;
        }
        Ok(Vlu32B(num))
    }
}

#[cfg(test)]
mod test {
    use crate::serdes::{Buf, BufMut};

    // ≈ 29min to complete on all 32 bit numbers on i7 8700K
    #[test]
    fn round_trip_vlu32b() {
        let mut buf = [0u8; 5];
        let numbers = [
            0,
            127,
            128,
            16_383,
            16_384,
            2_097_151,
            2_097_152,
            268_435_455,
            268_435_456,
            u32::MAX - 1,
            u32::MAX,
        ];
        for i in numbers {
            {
                let mut wr = BufMut::new(&mut buf);
                wr.put_vlu32b(i).unwrap();
            }

            let mut rgr = Buf::new(&mut buf);
            assert_eq!(rgr.get_vlu32b(), Ok(i));
        }
    }
}
