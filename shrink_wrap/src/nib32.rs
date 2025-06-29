use core::fmt::{Debug, Formatter};

use crate::{BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};

/// Variable length encoded u32 based on nibbles.
/// Each nibbles carries 1 bit indicating whether there are more nibbles + 3 bits from the original number.
#[derive(Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct UNib32(pub u32);

const ONE_MORE_NIBBLE: u8 = 0b1000;

impl UNib32 {
    pub fn len_nibbles(&self) -> usize {
        // TODO: measure what is faster, this impl or the one in tests
        if self.0 == 0 {
            1
        } else {
            ((32 - self.0.leading_zeros()) as usize).div_ceil(3)
        }
    }

    pub(crate) fn write_forward(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let mut val = self.0;
        let mut nibbles_left = self.len_nibbles();
        while nibbles_left > 0 {
            let nib = (val & 0b111) as u8;
            let nib = if nibbles_left > 1 {
                nib | ONE_MORE_NIBBLE
            } else {
                nib
            };
            wr.write_u4(nib)?;
            val >>= 3;
            nibbles_left -= 1;
        }
        Ok(())
    }

    pub(crate) fn write_reversed(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let mut val = self.0;
        let len = self.len_nibbles();
        for i in 0..len {
            let nib = (val & 0b111) as u8;
            // reversed unib is written left to write, but read from right to left, so "one more nibble" bits must also be reversed
            if len >= 1 && i == 0 {
                // (len == 1 && i == 0) || (len > 1 && i == 0)
                // no flag if only one nibble or if last nibble (seen from right to left, so at i == 0)
                wr.write_u4(nib).map_err(|_| Error::OutOfBoundsRevCompact)?;
            } else {
                wr.write_u4(nib | ONE_MORE_NIBBLE)
                    .map_err(|_| Error::OutOfBoundsRevCompact)?;
            }
            val >>= 3;
        }
        Ok(())
    }

    pub(crate) fn read_forward(rd: &mut BufReader) -> Result<Self, Error> {
        let mut num = 0;
        let mut offset = 0;
        for i in 0..=10 {
            let nib = rd.read_u4()?;
            if i == 10 {
                // 11th nibble should be the last for u32
                if nib & ONE_MORE_NIBBLE != 0 {
                    return Err(Error::MalformedUNib32);
                }
            }
            num |= (nib as u32 & 0b111) << offset;
            if nib & ONE_MORE_NIBBLE == 0 {
                break;
            }
            offset += 3;
        }
        Ok(UNib32(num))
    }

    pub(crate) fn read_reversed(rd: &mut BufReader) -> Result<Self, Error> {
        let mut num = 0;
        for i in 0..=10 {
            let nib = rd.read_u4_rev()?;
            if i == 10 {
                // 11th nibble should be the last for u16
                if nib & ONE_MORE_NIBBLE != 0 {
                    return Err(Error::MalformedUNib32);
                }
            }
            num |= nib as u32 & 0b111;
            if nib & ONE_MORE_NIBBLE == 0 {
                break;
            }
            num <<= 3;
        }
        Ok(UNib32(num))
    }
}

impl SerializeShrinkWrap for UNib32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        self.write_forward(wr)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for UNib32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        UNib32::read_forward(rd)
    }
}

impl Debug for UNib32 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod test {
    use crate::nib32::UNib32;
    use crate::{BufReader, BufWriter, Error};

    #[test]
    fn u32_max_plus1() {
        // ignore bit 33
        let buf = [0xff, 0xff, 0xff, 0xff, 0xff, 0x70];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UNib32::read_forward(&mut rd), Ok(UNib32(u32::MAX)));
        assert_eq!(rd.read_u4(), Ok(0));
        assert_eq!(rd.nibbles_left(), 0);
    }

    #[test]
    fn u32_max_plus_nibble() {
        // more nibbles than expected for u32
        let buf = [0xff, 0xff, 0xff, 0xff, 0xff, 0xf0];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UNib32::read_forward(&mut rd), Err(Error::MalformedUNib32));
        assert_eq!(rd.nibbles_left(), 1);
    }

    #[test]
    fn write_unib16_reversed() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u4(0).unwrap();
        UNib32(0b1000).write_reversed(&mut wr).unwrap();
        UNib32(0b101010).write_reversed(&mut wr).unwrap();
        UNib32(5).write_reversed(&mut wr).unwrap();
        assert_eq!(
            wr.finish().unwrap(),
            // note that "one more nibble" bits are true from right to left, opposite of the forward version
            //               |            |
            &[0b0000_0000, 0b1001_0010, 0b1101_0101]
        );
    }

    #[test]
    fn read_unib16_reversed() {
        // Note that 0 should be read from the back even if it wasn't written.
        // This edge case is handled by writing one 0 nibble before writing reversed Vlu16N in BufWriter::finish().
        let buf = [0b0000_1001, 0b0010_1101, 0b0101_0000];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UNib32::read_reversed(&mut rd).unwrap().0, 0);
        assert_eq!(UNib32::read_reversed(&mut rd).unwrap().0, 5);
        assert_eq!(UNib32::read_reversed(&mut rd).unwrap().0, 42);
        assert_eq!(UNib32::read_reversed(&mut rd).unwrap().0, 8);
    }

    #[inline]
    fn test_forward(num: u32, nib_count: usize, repr: &[u8]) {
        const SIZE: usize = 8;
        let mut buf = [0u8; SIZE];
        let mut wr = BufWriter::new(&mut buf);
        UNib32(num).write_forward(&mut wr).unwrap();
        assert_eq!(SIZE * 2 - wr.nibbles_left(), nib_count);
        let buf = wr.finish().unwrap();
        let mut rd = BufReader::new(buf);
        assert_eq!(UNib32::read_forward(&mut rd), Ok(UNib32(num)));
        assert_eq!(buf, repr);
    }

    #[test]
    fn unib32_sanity_check() {
        test_forward(0, 1, &[0x00]); // 000
        test_forward(7, 1, &[0b0111_0000]); // 111
        test_forward(8, 2, &[0b1000_0001]); // 000 001
        test_forward(0o124, 3, &[0b1100_1010, 0b0001_0000]); // 0o 4 2 1
        test_forward(0o777, 3, &[0b1111_1111, 0b0111_0000]); // 0o 7 7 7
    }

    #[inline]
    fn test_reversed(num: u16) {
        const SIZE: usize = 8;
        let mut buf = [0u8; SIZE];
        let mut wr = BufWriter::new(&mut buf);
        // UNib32(num).write_reversed(&mut wr).unwrap();
        wr.write_u16_rev(num).unwrap();
        // assert_eq!(SIZE * 2 - wr.nibbles_left(), nib_count);
        let buf = wr.finish().unwrap();
        let mut rd = BufReader::new(buf);
        assert_eq!(UNib32::read_reversed(&mut rd), Ok(UNib32(num as u32)));
        // assert_eq!(buf, repr);
    }

    #[test]
    fn unib32_reversed_sanity_check() {
        for i in 0..65_535 {
            test_reversed(i);
        }
    }

    // #[test]
    // fn unib32_round_trip() {
    //     let mut buf = [0u8; 8];
    //     for i in 0..=u32::MAX {
    //         let mut wr = BufWriter::new(&mut buf);
    //         UNib32(i).write_forward(&mut wr).unwrap();
    //         let mut rd = BufReader::new(&buf);
    //         assert_eq!(UNib32::read_forward(&mut rd), Ok(UNib32(i)));
    //     }
    // }

    fn len_nibbles(num: u32) -> usize {
        match num {
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
            1_073_741_824..=4_294_967_295 => 11,
        }
    }

    #[test]
    fn len_nibbles_sanity() {
        for i in 0..=10 {
            let min = if i == 0 { 0 } else { 2u32.pow(i - 1) };
            let mid = min + 1;
            let max = 2u32.pow(i) - 1;
            assert_eq!(len_nibbles(min), UNib32(min).len_nibbles());
            assert_eq!(len_nibbles(mid), UNib32(mid).len_nibbles());
            assert_eq!(len_nibbles(max), UNib32(max).len_nibbles());
        }
    }

    // #[test]
    // fn len_nibbles_round_trip() {
    //     for i in 0..=u32::MAX {
    //         assert_eq!(len_nibbles(i), UNib32(i).len_nibbles());
    //     }
    // }
}
