use core::fmt::{Debug, Display, Formatter};

use crate::{
    BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, Error,
    Nibble, SerializeShrinkWrap,
};

/// Variable length encoded u32 based on nibbles.
/// Each nibble carries 1 bit indicating whether there are more nibbles + 3 bits from the original number.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct UNib32(pub u32);

const ONE_MORE_NIBBLE: u8 = 0b1000;

impl UNib32 {
    pub const fn len_nibbles(&self) -> usize {
        // A loop rather than `bits.div_ceil(3)`: no division routine on CPUs without a divider (RV32I, Cortex-M0),
        // which is 170 bytes of code there, see docs/serdes/code_size.md.
        let mut val = self.0 >> 3;
        let mut nibbles = 1;
        while val != 0 {
            val >>= 3;
            nibbles += 1;
        }
        nibbles
    }

    pub(crate) fn write_forward(&self, wr: &mut BufWriter) -> Result<(), Error> {
        // the nibbles in runs of up to 7, a write each (1 for a value up to 2^21): the first nibble in the highest
        // bits, a "one more nibble" flag on all but the last of the value
        let mut val = self.0;
        if val < 8 {
            // one nibble, most values: the plain nibble write is cheaper than a run
            return wr.write_nib(Nibble::new_masked(val as u8));
        }
        loop {
            let (mut bits, mut count) = (0, 0);
            let more = loop {
                let more = val >> 3 != 0;
                bits = bits << 4 | val & 0b111 | (more as u32) << 3;
                val >>= 3;
                count += 1;
                if !more || count == 7 {
                    break more;
                }
            };
            wr.write_nibbles(count, bits)?;
            if !more {
                return Ok(());
            }
        }
    }

    pub(crate) fn write_reversed(&self, wr: &mut BufWriter) -> Result<(), Error> {
        // reversed unib is written left to right, but read from right to left: the nibble written first is the last
        // one read, so it is the one without the "one more nibble" flag
        let mut val = self.0;
        if val < 8 {
            return wr
                .write_nib(Nibble::new_masked(val as u8))
                .map_err(|_| Error::OutOfBoundsRevCompact);
        }
        let mut flag = 0;
        loop {
            let (mut bits, mut count) = (0, 0);
            loop {
                bits = bits << 4 | val & 0b111 | flag;
                flag = ONE_MORE_NIBBLE as u32;
                val >>= 3;
                count += 1;
                if val == 0 || count == 7 {
                    break;
                }
            }
            wr.write_nibbles(count, bits)
                .map_err(|_| Error::OutOfBoundsRevCompact)?;
            if val == 0 {
                return Ok(());
            }
        }
    }

    pub(crate) fn read_forward(rd: &mut BufReader) -> Result<Self, Error> {
        rd.take_unib32().map(UNib32)
    }

    pub(crate) fn read_reversed(rd: &mut BufReader) -> Result<Self, Error> {
        let mut num = 0;
        for i in 0..=10 {
            let nib = rd.read_u4_rev()?;
            if i == 10 {
                // 11th nibble should be the last for u32
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
    const PUSHES_LEN: bool = false;
    const MAX_SIZE: Option<crate::max_size::MaxSize> = Some(crate::max_size::MaxSize::nibbles(
        UNib32(u32::MAX).len_nibbles(),
    ));

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        self.write_forward(wr)
    }
}

#[cfg(feature = "std")]
impl crate::SerializeShrinkWrapOwned for UNib32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        wr.write_unib32(self.0)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for UNib32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        UNib32::read_forward(rd)
    }
}

impl DeserializeShrinkWrapOwned for UNib32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        UNib32::read_forward(rd)
    }
}

impl From<UNib32> for u32 {
    fn from(value: UNib32) -> Self {
        value.0
    }
}

impl From<u32> for UNib32 {
    fn from(num: u32) -> Self {
        UNib32(num)
    }
}

impl Debug for UNib32 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Display for UNib32 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Variable length encoded i32, zigzag over [UNib32]: `0`, `-1`, `1`, `-2`, `2`, .. map to `0`, `1`, `2`, `3`, `4`,
/// .., so small negative and positive values both take one nibble (SW-24). General purpose: the signed
/// counterpart of `UNib32`, not tied to any one field's meaning.
///
/// On the wire it is exactly a [UNib32] of the zigzag-encoded value: [to_zigzag](INib32::to_zigzag) /
/// [from_zigzag](INib32::from_zigzag) do the mapping, `UNib32`'s own (private) forward nibble writer and reader
/// do the nibbles, so a malformed read fails with [Error::MalformedUNib32], the same as a malformed `UNib32`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct INib32(pub i32);

/// Protobuf-style zigzag: the sign in the lowest bit, the magnitude above it, so a small negative or positive
/// number both end up a small unsigned one.
pub(crate) const fn zigzag32(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

pub(crate) const fn unzigzag32(z: u32) -> i32 {
    ((z >> 1) as i32) ^ -((z & 1) as i32)
}

impl INib32 {
    /// The zigzag-encoded value, as the [UNib32] written on the wire.
    pub const fn to_zigzag(self) -> UNib32 {
        UNib32(zigzag32(self.0))
    }

    /// The value a [UNib32] of `z` zigzag-decodes to.
    pub const fn from_zigzag(z: UNib32) -> Self {
        INib32(unzigzag32(z.0))
    }

    pub const fn len_nibbles(&self) -> usize {
        self.to_zigzag().len_nibbles()
    }

    pub(crate) fn write_forward(&self, wr: &mut BufWriter) -> Result<(), Error> {
        self.to_zigzag().write_forward(wr)
    }

    pub(crate) fn read_forward(rd: &mut BufReader) -> Result<Self, Error> {
        UNib32::read_forward(rd).map(Self::from_zigzag)
    }
}

impl SerializeShrinkWrap for INib32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;
    const PUSHES_LEN: bool = false;
    const MAX_SIZE: Option<crate::max_size::MaxSize> = Some(crate::max_size::MaxSize::nibbles(
        UNib32(u32::MAX).len_nibbles(),
    ));

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        self.write_forward(wr)
    }
}

#[cfg(feature = "std")]
impl crate::SerializeShrinkWrapOwned for INib32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        wr.write_inib32(self.0)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for INib32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        INib32::read_forward(rd)
    }
}

impl DeserializeShrinkWrapOwned for INib32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        INib32::read_forward(rd)
    }
}

impl From<INib32> for i32 {
    fn from(value: INib32) -> Self {
        value.0
    }
}

impl From<i32> for INib32 {
    fn from(num: i32) -> Self {
        INib32(num)
    }
}

impl Debug for INib32 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Display for INib32 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod test {
    use crate::nib32::{INib32, UNib32, unzigzag32, zigzag32};
    use crate::{
        BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, Error,
        SerializeShrinkWrap, SerializeShrinkWrapOwned,
    };

    #[test]
    fn u32_max_plus1() {
        // ignore bit 33
        let buf = [0xff, 0xff, 0xff, 0xff, 0xff, 0x70];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UNib32::read_forward(&mut rd), Ok(UNib32(u32::MAX)));
        assert_eq!(rd.read_nib_value(), Ok(0));
        assert_eq!(rd.bytes_left(), 0);
        assert_eq!(rd.nibbles_left(), 0);
    }

    #[test]
    fn u32_max_plus_nibble() {
        // more nibbles than expected for u32
        let buf = [0xff, 0xff, 0xff, 0xff, 0xff, 0xf0];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UNib32::read_forward(&mut rd), Err(Error::MalformedUNib32));
        assert_eq!(rd.nibbles_left(), 1);

        let buf = [0x0f, 0xff, 0xff, 0xff, 0xff, 0xff];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UNib32::read_reversed(&mut rd), Err(Error::MalformedUNib32));
        assert_eq!(rd.nibbles_left(), 1);
    }

    #[test]
    fn write_unib16_reversed() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_nib_masked(0).unwrap();
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
    fn test_reversed(num: usize) {
        const SIZE: usize = 8;
        let mut buf = [0u8; SIZE];
        let mut wr = BufWriter::new(&mut buf);
        // UNib32(num).write_reversed(&mut wr).unwrap();
        wr.write_rev_len(num).unwrap();
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
            // 1_073_741_824..=4_294_967_295 => 11,
            _ => 11, // make IDE happy
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

    #[test]
    fn traits() {
        let mut buf = [0; 64];
        let bytes = UNib32(123).to_ww_bytes(&mut buf).unwrap();
        let num = UNib32::from_ww_bytes(bytes).unwrap();
        assert_eq!(num, UNib32(123));
        let num = UNib32::from_ww_bytes_owned(bytes).unwrap();
        assert_eq!(num, UNib32(123));
    }

    #[test]
    fn zigzag_sanity_check() {
        // small values on both sides of zero map to small unsigned ones, one nibble each
        assert_eq!(zigzag32(0), 0);
        assert_eq!(zigzag32(-1), 1);
        assert_eq!(zigzag32(1), 2);
        assert_eq!(zigzag32(-2), 3);
        assert_eq!(zigzag32(2), 4);
        assert_eq!(zigzag32(i32::MAX), u32::MAX - 1);
        assert_eq!(zigzag32(i32::MIN), u32::MAX);
        for v in [0, -1, 1, -2, 2, i32::MIN, i32::MAX, -12345, 12345] {
            assert_eq!(unzigzag32(zigzag32(v)), v);
        }
    }

    #[inline]
    fn test_inib32_forward(num: i32, nib_count: usize) {
        const SIZE: usize = 8;
        let mut buf = [0u8; SIZE];
        let mut wr = BufWriter::new(&mut buf);
        INib32(num).write_forward(&mut wr).unwrap();
        assert_eq!(SIZE * 2 - wr.nibbles_left(), nib_count);
        let buf = wr.finish().unwrap();
        let mut rd = BufReader::new(buf);
        assert_eq!(INib32::read_forward(&mut rd), Ok(INib32(num)));
    }

    #[test]
    fn inib32_sanity_check() {
        test_inib32_forward(0, 1);
        test_inib32_forward(-1, 1);
        test_inib32_forward(1, 1);
        test_inib32_forward(-2, 1);
        test_inib32_forward(2, 1);
        test_inib32_forward(-4, 1); // zigzag(-4) = 7, still one nibble
        test_inib32_forward(4, 2); // zigzag(4) = 8, needs a second nibble
        test_inib32_forward(i32::MIN, UNib32(u32::MAX).len_nibbles());
        test_inib32_forward(i32::MAX, UNib32(u32::MAX - 1).len_nibbles());
    }

    #[test]
    fn inib32_malformed_is_malformed_unib32() {
        // on the wire INib32 is exactly a UNib32 of the zigzag value: a malformed read reports the same error
        let buf = [0xff, 0xff, 0xff, 0xff, 0xff, 0xf0];
        let mut rd = BufReader::new(&buf);
        assert_eq!(INib32::read_forward(&mut rd), Err(Error::MalformedUNib32));
    }

    #[test]
    fn buf_reader_writer_convenience_methods() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_inib32(-123).unwrap();
        let buf = wr.finish().unwrap();
        let mut rd = BufReader::new(buf);
        assert_eq!(rd.read_inib32(), Ok(-123));
    }

    #[test]
    fn inib32_traits() {
        let mut buf = [0; 64];
        let bytes = INib32(-123).to_ww_bytes(&mut buf).unwrap();
        let num = INib32::from_ww_bytes(bytes).unwrap();
        assert_eq!(num, INib32(-123));
        let num = INib32::from_ww_bytes_owned(bytes).unwrap();
        assert_eq!(num, INib32(-123));
    }

    proptest::proptest! {
        #[test]
        fn zigzag32_round_trip(num in proptest::prelude::any::<i32>()) {
            proptest::prop_assert_eq!(unzigzag32(zigzag32(num)), num);
        }

        #[test]
        fn inib32_round_trip(num in proptest::prelude::any::<i32>()) {
            let mut buf = [0u8; 8];
            let mut wr = BufWriter::new(&mut buf);
            INib32(num).write_forward(&mut wr).unwrap();
            let buf = wr.finish().unwrap();
            let mut rd = BufReader::new(buf);
            proptest::prop_assert_eq!(INib32::read_forward(&mut rd), Ok(INib32(num)));
        }

        #[test]
        fn inib32_owned_round_trip(num in proptest::prelude::any::<i32>()) {
            let bytes = INib32(num).to_ww_bytes_owned().unwrap();
            proptest::prop_assert_eq!(INib32::from_ww_bytes_owned(&bytes), Ok(INib32(num)));
        }
    }
}
