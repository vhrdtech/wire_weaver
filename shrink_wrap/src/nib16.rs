use core::fmt::{Debug, Formatter};

use crate::{BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};

/// Variable length encoded u16 based on nibbles.
/// Each nibbles carries 1 bit indicating whether there are more nibbles + 3 bits from the original number.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Nib16(pub u16);

impl Nib16 {
    pub(crate) fn len_nibbles(&self) -> usize {
        match self.0 {
            0..=7 => 1,
            8..=63 => 2,
            64..=511 => 3,
            512..=4095 => 4,
            4096..=32_767 => 5,
            _ => 6,
        }
    }

    pub(crate) fn write_forward(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let mut val = self.0 as u32;
        let shift_n = (self.len_nibbles() - 1) * 3;
        for i in 0..self.len_nibbles() {
            let nib = ((val >> shift_n) & 0b111) as u8;
            if i != self.len_nibbles() - 1 {
                wr.write_u4(nib | 0b1000)?;
            } else {
                wr.write_u4(nib)?;
            }
            val <<= 3;
        }
        Ok(())
    }

    pub(crate) fn write_reversed(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let mut val = self.0;
        for i in 0..self.len_nibbles() {
            let nib = (val & 0b111) as u8;
            if (i == self.len_nibbles() - 1) && self.len_nibbles() > 1 {
                wr.write_u4(nib | 0b1000)
                    .map_err(|_| Error::OutOfBoundsRevCompact)?;
            } else {
                wr.write_u4(nib).map_err(|_| Error::OutOfBoundsRevCompact)?;
            }
            val >>= 3;
        }
        Ok(())
    }

    pub(crate) fn read_forward(rd: &mut BufReader) -> Result<Self, Error> {
        let mut num = 0;
        for i in 0..=5 {
            let nib = rd.read_u4()?;
            if i == 5 {
                // 6th nibble should be the last for u16
                if nib & 0b1000 != 0 {
                    return Err(Error::MalformedVlu16N);
                }
            }
            num |= nib as u16 & 0b111;
            if nib & 0b1000 == 0 {
                break;
            }
            num <<= 3;
        }
        Ok(Nib16(num))
    }

    pub(crate) fn read_reversed(rd: &mut BufReader) -> Result<Self, Error> {
        let mut num = 0;
        for i in 0..=5 {
            let nib = rd.read_u4_rev()?;
            if i == 5 {
                // 6th nibble should be the last for u16
                if nib & 0b1000 != 0 {
                    return Err(Error::MalformedVlu16N);
                }
            }
            num |= nib as u16 & 0b111;
            if nib & 0b1000 == 0 {
                break;
            }
            num <<= 3;
        }
        Ok(Nib16(num))
    }
}

impl SerializeShrinkWrap for Nib16 {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        self.write_forward(wr)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for Nib16 {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        _element_size: ElementSize,
    ) -> Result<Self, Error> {
        Nib16::read_forward(rd)
    }
}

impl Debug for Nib16 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod test {
    // extern crate std;

    use crate::nib16::Nib16;
    use crate::{BufReader, BufWriter};

    // #[test]
    // fn read_vlu4_u32_single_nibble() {
    //     let buf = [0b0111_0010, 0b0000_0001];
    //     let mut rdr = BufReader::new(&buf);
    //     assert_eq!(rdr.read_vlu16n(), Ok(7));
    //     assert_eq!(rdr.read_vlu16n(), Ok(2));
    //     assert_eq!(rdr.read_vlu16n(), Ok(0));
    //     assert_eq!(rdr.read_vlu16n(), Ok(1));
    //     assert!(rdr.is_at_end());
    // }
    //
    // #[test]
    // fn read_vlu4_u32_multi_nibble() {
    //     let buf = [0b1111_0111, 0b1001_0000, 0b1000_0111];
    //     let mut rdr = BufReader::new(&buf);
    //     assert_eq!(rdr.get_vlu32n(), Ok(63));
    //     assert_eq!(rdr.nibbles_pos(), 2);
    //     assert_eq!(rdr.get_vlu32n(), Ok(0b001000));
    //     assert_eq!(rdr.nibbles_pos(), 4);
    //     assert_eq!(rdr.get_vlu32n(), Ok(0b111));
    //     assert!(rdr.is_at_end());
    // }
    //
    // #[test]
    // fn read_vlu4_u32_max() {
    //     let buf = [0b1011_1111, 0xff, 0xff, 0xff, 0xff, 0x70];
    //     let mut rdr = BufReader::new(&buf);
    //     assert_eq!(rdr.get_vlu32n(), Ok(u32::MAX));
    //     assert_eq!(rdr.get_nibble(), Ok(0));
    //     assert!(rdr.is_at_end());
    // }
    //
    // #[test]
    // fn read_vlu4_u32_max_plus1() {
    //     // ignore bit 33
    //     let buf = [0b1111_1111, 0xff, 0xff, 0xff, 0xff, 0x70];
    //     let mut rdr = BufReader::new(&buf);
    //     assert_eq!(rdr.get_vlu32n(), Ok(u32::MAX));
    //     assert_eq!(rdr.get_nibble(), Ok(0));
    //     assert!(rdr.is_at_end());
    // }
    //
    // #[test]
    // fn read_vlu4_u32_max_plus_nibble() {
    //     // more nibbles than expected for u32
    //     let buf = [0xff, 0xff, 0xff, 0xff, 0xff, 0xf0];
    //     let mut rdr = BufReader::new(&buf);
    //     assert_eq!(rdr.get_vlu32n(), Err(Error::MalformedVlu32N));
    //     assert_eq!(rdr.nibbles_left(), 1);
    // }

    #[test]
    fn write_vlu16n_forward() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        Nib16(0b1000).write_forward(&mut wr).unwrap();
        Nib16(0b101010).write_forward(&mut wr).unwrap();
        Nib16(5).write_forward(&mut wr).unwrap();
        assert_eq!(
            wr.finish().unwrap(),
            &[0b1001_0000, 0b1101_0010, 0b0101_0000]
        );
    }

    #[test]
    fn write_vlu16n_reversed() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        Nib16(0b1000).write_reversed(&mut wr).unwrap();
        Nib16(0b101010).write_reversed(&mut wr).unwrap();
        Nib16(5).write_reversed(&mut wr).unwrap();
        assert_eq!(
            wr.finish().unwrap(),
            &[0b0000_1001, 0b0010_1101, 0b0101_0000]
        );
    }

    #[test]
    fn read_vlu16n_reversed() {
        // Note that we would read 0 from the back even if it wasn't actually written.
        // This edge case is handled by writing one 0 nibble before writing reversed Vlu16N in BufWriter::finish().
        let buf = [0b0000_1001, 0b0010_1101, 0b0101_0000];
        let mut rd = BufReader::new(&buf);
        assert_eq!(Nib16::read_reversed(&mut rd).unwrap().0, 0);
        assert_eq!(Nib16::read_reversed(&mut rd).unwrap().0, 5);
        assert_eq!(Nib16::read_reversed(&mut rd).unwrap().0, 42);
        assert_eq!(Nib16::read_reversed(&mut rd).unwrap().0, 8);
    }

    #[test]
    fn round_trip_nib16_rev() {
        let mut buf = [0; 5];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u16_rev(5).unwrap();
        wr.write_u16_rev(42).unwrap();
        let buf = wr.finish().unwrap();

        let mut rd = BufReader::new(buf);
        assert_eq!(rd.read_nib16_rev().unwrap(), 5);
        assert_eq!(rd.read_nib16_rev().unwrap(), 42);
    }

    #[test]
    fn round_trip_vlu16n() {
        let mut buf = [0u8; 3];
        for i in 0..=u16::MAX {
            let mut wr = BufWriter::new(&mut buf);
            Nib16(i).write_forward(&mut wr).unwrap();
            let mut rd = BufReader::new(&buf);
            assert_eq!(Nib16::read_forward(&mut rd), Ok(Nib16(i)));
        }
    }
}
