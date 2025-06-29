use crate::{BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};
use paste::paste;

macro_rules! un {
    ($bits:literal, $base_bits:literal) => {
        paste! {
            #[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
            #[cfg_attr(feature = "defmt", derive(defmt::Format))]
            #[doc = $bits "-bit unsigned number, backed by u" $base_bits ", serialized as " $bits " bits with alignemnt of 1 bit."]
            pub struct [<U $bits>]([<u $base_bits>]);
            impl [<U $bits>] {
                #[inline(always)]
                pub const fn new(x: [<u $base_bits>]) -> Option<Self> {
                    if x <= Self::max().0 {
                        Some(Self(x))
                    } else {
                        None
                    }
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
                    // avoid overflow (2^(n-1) - 1) + 2^(n-1)
                    Self(([<2 _ u $base_bits>].pow($bits - 1) - 1) + [<2 _ u $base_bits>].pow($bits - 1))
                }

                #[inline(always)]
                pub const fn value(&self) -> [<u $base_bits>] {
                    self.0
                }
            }

            impl SerializeShrinkWrap for [<U $bits>] {
                const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: $bits };

                fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                    wr.[<write_un $base_bits>]($bits, self.0)
                }
            }

            impl<'i> DeserializeShrinkWrap<'i> for [<U $bits>] {
                const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: $bits };

                fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
                    Ok([<U $bits>](rd.[<read_un $base_bits>]($bits)?))
                }
            }
        }
    };
}

macro_rules! unx {
    ($($bits:literal),* / $base_bits:literal) => {
        $(un!($bits, $base_bits);)*
    };
}

unx!(1, 2, 3, 4, 5, 6, 7, 8 / 8);
unx!(9, 10, 11, 12, 13, 14, 15, 16 / 16);
unx!(17, 18, 19, 20, 21, 22, 23, 24 / 32);
unx!(25, 26, 27, 28, 29, 30, 31, 32 / 32);
unx!(33, 34, 35, 36, 37, 38, 39, 40 / 64);
unx!(41, 42, 43, 44, 45, 46, 47, 48 / 64);
unx!(49, 50, 51, 52, 53, 54, 55, 56 / 64);
unx!(57, 58, 59, 60, 61, 62, 63, 64 / 64);

macro_rules! write_unx {
    ($fn_name:ident, $base_ty:ty, $max_bit_count:literal) => {
        paste::paste! {
            #[doc = "Write up to " $max_bit_count " bits from " $base_ty " number without alignment."]
            pub fn $fn_name(&mut self, bit_count: u8, value: $base_ty) -> Result<(), Error> {
                if bit_count > $max_bit_count {
                    return Err(Error::InvalidBitCount);
                }

                let mut bits_left = bit_count;
                while bits_left > 0 {
                    if (self.bytes_left() == 0) && self.bit_idx == 7 {
                        return Err(Error::OutOfBoundsWriteUN(UNib32(bit_count as u32)));
                    }

                    let bits_to_write = bits_left.min(self.bit_idx + 1);
                    let mask = ((1 as $base_ty) << bits_to_write) - 1;
                    let bits = ((value >> (bits_left - bits_to_write)) & mask) as u8;

                    self.buf[self.byte_idx] &= !(mask as u8) << (self.bit_idx + 1 - bits_to_write);
                    self.buf[self.byte_idx] |= bits << (self.bit_idx + 1 - bits_to_write);

                    self.bit_idx = self.bit_idx.wrapping_sub(bits_to_write);
                    if self.bit_idx == 255 {
                        // Wrapping around after saturating_sub(1) from 0 .. 8 from 7
                        self.bit_idx = 7;
                        self.byte_idx += 1;
                    }

                    bits_left -= bits_to_write;
                }

                Ok(())
            }
        }
    };
}
pub(crate) use write_unx;

macro_rules! read_unx {
    ($fn_name:ident, $base_ty:ty, $max_bit_count:literal) => {
        paste::paste! {
            #[doc = "Read up to " $max_bit_count " bits (without alignment) into " $base_ty " number."]
            pub fn $fn_name(&mut self, bit_count: u8) -> Result<$base_ty, Error> {
                if bit_count > $max_bit_count {
                    return Err(Error::InvalidBitCount);
                }

                let mut result: $base_ty = 0;
                let mut bits_left = bit_count;

                while bits_left > 0 {
                    if (self.bytes_left() == 0) && (self.bits_in_byte_left() == 0) {
                        return Err(Error::OutOfBoundsReadUN(UNib32(bit_count as u32)));
                    }

                    let bits_to_read = bits_left.min(self.bit_idx + 1);
                    let mask = ((1u16 << bits_to_read) - 1) as u8;
                    let shift = self.bit_idx + 1 - bits_to_read;
                    let bits = (self.buf[self.byte_idx] >> shift) & mask;

                    result = (result << bits_to_read) | (bits as $base_ty);

                    self.bit_idx = self.bit_idx.wrapping_sub(bits_to_read);
                    if self.bit_idx == 255 {
                        // Wrapping around after saturating_sub(1) from 0 .. 8 from 7
                        self.bit_idx = 7;
                        self.byte_idx += 1;
                    }

                    bits_left -= bits_to_read;
                }

                Ok(result)
            }
        }
    };
}
pub(crate) use read_unx;

macro_rules! signed_un {
    ($bits:literal, $base_bits:literal) => {
        paste! {
            #[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
            #[cfg_attr(feature = "defmt", derive(defmt::Format))]
            #[doc = $bits "-bit signed number, backed by i" $base_bits ", serialized as " $bits " bits with alignment of 1 bit."]
            pub struct [<I $bits>]([<i $base_bits>]);
            impl [<I $bits>] {
                #[inline(always)]
                pub const fn new(x: [<i $base_bits>]) -> Option<Self> {
                    if x >= Self::min().0 && x <= Self::max().0 {
                        Some(Self(x))
                    } else {
                        None
                    }
                }

                #[inline(always)]
                pub const fn min() -> Self {
                    // avoid overflow -2^(n-2) - 2^(n-2)
                    Self(-[<2 _ i $base_bits>].pow($bits - 2) - [<2 _ i $base_bits>].pow($bits - 2))
                }

                #[inline(always)]
                pub const fn minus_one() -> Self {
                    Self(-1)
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
                    // avoid overflow (2^(n-2) - 1) + 2^(n-2)
                    Self(([<2 _ i $base_bits>].pow($bits - 2) - 1) + [<2 _ i $base_bits>].pow($bits - 2))
                }

                #[inline(always)]
                pub const fn value(&self) -> [<i $base_bits>] {
                    self.0
                }
            }

            impl SerializeShrinkWrap for [<I $bits>] {
                const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: $bits };

                fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                    wr.[<write_un $base_bits>]($bits, self.0 as [<u $base_bits>])
                }
            }

            impl<'i> DeserializeShrinkWrap<'i> for [<I $bits>] {
                const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: $bits };

                fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
                    let val = rd.[<read_un $base_bits>]($bits)?;
                    let is_negative = val & (1 << ($bits - 1)) != 0;
                    if is_negative {
                        const SIGN_EXTEND_M1: [<u $base_bits>] = [<u $base_bits>]::MAX << ($bits - 1);
                        const SIGN_EXTEND: [<u $base_bits>] = SIGN_EXTEND_M1 << 1;
                        Ok(Self((val | SIGN_EXTEND) as [<i $base_bits>]))
                    } else {
                        Ok(Self(val as [<i $base_bits>]))
                    }
                }
            }
        }
    };
}

macro_rules! inx {
    ($($bits:literal),* / $base_bits:literal) => {
        $(signed_un!($bits, $base_bits);)*
    };
}

inx!(2, 3, 4, 5, 6, 7, 8 / 8);
inx!(9, 10, 11, 12, 13, 14, 15 / 16);
inx!(17, 18, 19, 20, 21, 22, 23, 24 / 32);
inx!(25, 26, 27, 28, 29, 30, 31, 32 / 32);
inx!(33, 34, 35, 36, 37, 38, 39, 40 / 64);
inx!(41, 42, 43, 44, 45, 46, 47, 48 / 64);
inx!(49, 50, 51, 52, 53, 54, 55, 56 / 64);
inx!(57, 58, 59, 60, 61, 62, 63, 64 / 64);

/// N-bit unsigned number, unlike Ux and Ix which are const size, here N is serialized as well and not known beforehand.
/// Serialized as `3 + 3(N<=8) or 4(N<=16) or 5(N<=32) or 6(N<=64) + N bits`.
/// Serialized layout: discriminant(U3), bit_count(U3, U4, U5 or U6), value (N-bits).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum UN {
    UN8 { bit_count: U3, value: u8 },
    UN16 { bit_count: U4, value: u16 },
    UN32 { bit_count: U5, value: u32 },
    UN64 { bit_count: U6, value: u64 },
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum IN {
    IN8 { bit_count: U3, value: i8 },
    IN16 { bit_count: U4, value: i16 },
    IN32 { bit_count: U5, value: i32 },
    IN64 { bit_count: U6, value: i64 },
}

impl SerializeShrinkWrap for UN {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap(&self, _wr: &mut BufWriter) -> Result<(), Error> {
        todo!()
    }
}

impl<'i> DeserializeShrinkWrap<'i> for UN {
    const ELEMENT_SIZE: ElementSize = <UN as SerializeShrinkWrap>::ELEMENT_SIZE;

    fn des_shrink_wrap<'di>(_rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        todo!()
    }
}

impl SerializeShrinkWrap for IN {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap(&self, _wr: &mut BufWriter) -> Result<(), Error> {
        todo!()
    }
}

impl<'i> DeserializeShrinkWrap<'i> for IN {
    const ELEMENT_SIZE: ElementSize = <UN as SerializeShrinkWrap>::ELEMENT_SIZE;

    fn des_shrink_wrap<'di>(_rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanity_check() {
        assert_eq!(U7::max().0, 127);
        assert_eq!(I7::max().0, 63);
        assert_eq!(I7::min().0, -64);
    }
}
