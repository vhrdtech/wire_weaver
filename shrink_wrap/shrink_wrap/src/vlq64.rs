use core::fmt::{Debug, Display, Formatter};

use crate::{
    BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, Error,
    SerializeShrinkWrap,
};

/// Variable length encoded u64 based on bytes (VLQ, big endian).
/// Each byte carries 1 bit (MSB) indicating whether there are more bytes + 7 bits from the original number,
/// most significant group first. Takes from 1 to 10 bytes, alignment of 1 byte is used.
///
/// Same encoding as [UVlq32](crate::UVlq32), just wide enough for a full `u64` (10 groups of 7 bits cover 70 bits,
/// so every `u64` value fits with room for non-canonical leading `0x80` padding groups, same as `UVlq32`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct UVlq64(pub u64);

const ONE_MORE_BYTE: u8 = 0b1000_0000;
const MAX_LEN: usize = 10;

impl UVlq64 {
    pub fn len_bytes(&self) -> usize {
        if self.0 == 0 {
            1
        } else {
            ((64 - self.0.leading_zeros()) as usize).div_ceil(7)
        }
    }

    /// Encode into the maximum length, right-justified: canonical encoding is in `bytes[start..]`, while `bytes`
    /// as a whole is the same number padded with empty `0x80` groups in front, which is also valid to read.
    pub(crate) fn encode_right_justified(&self) -> ([u8; MAX_LEN], usize) {
        let mut bytes = [ONE_MORE_BYTE; MAX_LEN];
        let len = self.len_bytes();
        for i in 0..len {
            let byte = ((self.0 >> (i * 7)) & 0x7f) as u8;
            bytes[MAX_LEN - 1 - i] = if i > 0 { byte | ONE_MORE_BYTE } else { byte };
        }
        (bytes, MAX_LEN - len)
    }

    pub(crate) fn write_forward(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let (bytes, start) = self.encode_right_justified();
        wr.write_raw_slice(&bytes[start..])
    }

    pub(crate) fn read_forward(rd: &mut BufReader) -> Result<Self, Error> {
        let mut num: u64 = 0;
        for _ in 0..MAX_LEN {
            let byte = rd.read_u8()?;
            if num > (u64::MAX >> 7) {
                // shifting would lose the most significant bits
                return Err(Error::MalformedUVlq64);
            }
            num = (num << 7) | (byte & 0x7f) as u64;
            if byte & ONE_MORE_BYTE == 0 {
                return Ok(UVlq64(num));
            }
        }
        // 10th byte should be the last for u64
        Err(Error::MalformedUVlq64)
    }
}

impl SerializeShrinkWrap for UVlq64 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        self.write_forward(wr)
    }
}

#[cfg(feature = "std")]
impl crate::SerializeShrinkWrapOwned for UVlq64 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        wr.write_uvlq64(self.0)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for UVlq64 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        UVlq64::read_forward(rd)
    }
}

impl DeserializeShrinkWrapOwned for UVlq64 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        UVlq64::read_forward(rd)
    }
}

impl From<UVlq64> for u64 {
    fn from(value: UVlq64) -> Self {
        value.0
    }
}

impl From<u64> for UVlq64 {
    fn from(num: u64) -> Self {
        UVlq64(num)
    }
}

impl Debug for UVlq64 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Display for UVlq64 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod test {
    use crate::vlq64::UVlq64;
    use crate::{
        BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, Error,
        SerializeShrinkWrap,
    };

    #[inline]
    fn test_forward(num: u64, repr: &[u8]) {
        const SIZE: usize = 16;
        let mut buf = [0u8; SIZE];
        let mut wr = BufWriter::new(&mut buf);
        UVlq64(num).write_forward(&mut wr).unwrap();
        assert_eq!(SIZE - wr.bytes_left(), UVlq64(num).len_bytes());
        let buf = wr.finish().unwrap();
        assert_eq!(buf, repr);
        let mut rd = BufReader::new(buf);
        assert_eq!(UVlq64::read_forward(&mut rd), Ok(UVlq64(num)));
        assert_eq!(rd.bytes_left(), 0);
    }

    #[test]
    fn uvlq64_sanity_check() {
        test_forward(0, &[0x00]);
        test_forward(0x7f, &[0x7f]);
        test_forward(0x80, &[0x81, 0x00]);
        test_forward(0x2000, &[0xc0, 0x00]);
        test_forward(0x3fff, &[0xff, 0x7f]);
        test_forward(0x4000, &[0x81, 0x80, 0x00]);
        test_forward(u32::MAX as u64, &[0x8f, 0xff, 0xff, 0xff, 0x7f]);
        test_forward(
            u64::MAX,
            &[0x81, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f],
        );
    }

    #[test]
    fn len_bytes_sanity() {
        for (bits, len) in [
            (0, 1),
            (7, 1),
            (8, 2),
            (14, 2),
            (15, 3),
            (28, 4),
            (29, 5),
            (32, 5),
            (56, 8),
            (63, 9),
            (64, 10),
        ] {
            let max = if bits == 64 {
                u64::MAX
            } else {
                (1u64 << bits) - 1
            };
            assert_eq!(UVlq64(max).len_bytes(), len, "bits: {bits}");
        }
    }

    #[test]
    fn non_canonical_leading_zero_groups() {
        let buf = [0x80, 0x80, 0x01];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UVlq64::read_forward(&mut rd), Ok(UVlq64(1)));
    }

    #[test]
    fn u64_overflow() {
        // 65 bits
        let buf = [0x83, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UVlq64::read_forward(&mut rd), Err(Error::MalformedUVlq64));
    }

    #[test]
    fn more_bytes_than_expected() {
        let buf = [
            0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01,
        ];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UVlq64::read_forward(&mut rd), Err(Error::MalformedUVlq64));
        assert_eq!(rd.bytes_left(), 1);
    }

    #[test]
    fn out_of_bounds() {
        let buf = [0x81, 0x80];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UVlq64::read_forward(&mut rd), Err(Error::OutOfBoundsReadU8));
    }

    #[test]
    fn byte_aligned() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_bool(true).unwrap();
        wr.write_uvlq64(0x80).unwrap();
        let bytes = wr.finish().unwrap();
        assert_eq!(bytes, &[0x80, 0x81, 0x00]);
        let mut rd = BufReader::new(bytes);
        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_uvlq64(), Ok(0x80));
    }

    #[test]
    fn traits() {
        let mut buf = [0; 64];
        let bytes = UVlq64(123_456).to_ww_bytes(&mut buf).unwrap();
        let num = UVlq64::from_ww_bytes(bytes).unwrap();
        assert_eq!(num, UVlq64(123_456));
        let num = UVlq64::from_ww_bytes_owned(bytes).unwrap();
        assert_eq!(num, UVlq64(123_456));

        assert_eq!(UVlq64(0).to_ww_bytes(&mut buf).unwrap(), &[0x00]);
        assert_eq!(UVlq64(127).to_ww_bytes(&mut buf).unwrap(), &[0x7f]);
        assert_eq!(UVlq64(128).to_ww_bytes(&mut buf).unwrap(), &[0x81, 0x00]);
        assert_eq!(
            UVlq64(u32::MAX as u64).to_ww_bytes(&mut buf).unwrap(),
            &[0x8f, 0xff, 0xff, 0xff, 0x7f]
        );
        assert_eq!(
            UVlq64(u64::MAX).to_ww_bytes(&mut buf).unwrap(),
            &[0x81, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn owned_writer_matches() {
        for num in [
            0,
            1,
            0x7f,
            0x80,
            0x3fff,
            0x4000,
            0xfff_ffff,
            0x1000_0000,
            u32::MAX as u64,
            u64::MAX,
        ] {
            let mut buf = [0u8; 16];
            let mut wr = BufWriter::new(&mut buf);
            wr.write_bool(true).unwrap();
            wr.write_uvlq64(num).unwrap();
            let expected = wr.finish().unwrap().to_vec();

            let mut wro = crate::BufWriterOwned::new();
            wro.write_bool(true).unwrap();
            wro.write_uvlq64(num).unwrap();
            assert_eq!(wro.finish().unwrap(), expected, "num: {num}");
        }
    }
}
