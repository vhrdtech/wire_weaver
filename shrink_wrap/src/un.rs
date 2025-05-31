use crate::{BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};
use paste::paste;

macro_rules! un {
    ($bits:literal, $base_bits:literal) => {
        paste! {
            #[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
            #[cfg_attr(feature = "defmt", derive(defmt::Format))]
            #[doc = $bits "-bit unsigned number, backed by u" $base_bits ", serialized as " $bits " bits"]
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
                pub const fn max() -> Self {
                    Self([<2 _ u $base_bits>].pow($bits) - 1)
                }

                #[inline(always)]
                pub const fn value(&self) -> [<u $base_bits>] {
                    self.0
                }
            }

            impl SerializeShrinkWrap for [<U $bits>] {
                fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                    wr.[<write_un $base_bits>]($bits, self.0)
                }
            }

            impl<'i> DeserializeShrinkWrap<'i> for [<U $bits>] {
                fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>, _s: ElementSize) -> Result<Self, Error> {
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

unx!(1, 2, 3, 4, 5, 6, 7 / 8);
unx!(9, 10, 11, 12, 13, 14, 15 / 16);
unx!(17, 18, 19, 20, 21, 22, 23, 24 / 32);
unx!(25, 26, 27, 28, 29, 30, 31 / 32);
unx!(33, 34, 35, 36, 37, 38, 39, 40 / 64);
unx!(41, 42, 43, 44, 45, 46, 47, 48 / 64);
unx!(49, 50, 51, 52, 53, 54, 55, 56 / 64);
unx!(57, 58, 59, 60, 61, 62, 63 / 64);

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
            #[doc = $bits "-bit signed number, backed by i" $base_bits ", serialized as " $bits " bits"]
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
                    Self(-[<2 _ i $base_bits>].pow($bits - 1))
                }

                #[inline(always)]
                pub const fn zero() -> Self {
                    Self(0)
                }

                #[inline(always)]
                pub const fn max() -> Self {
                    Self([<2 _ i $base_bits>].pow($bits - 1) - 1)
                }

                #[inline(always)]
                pub const fn value(&self) -> [<i $base_bits>] {
                    self.0
                }
            }

            impl SerializeShrinkWrap for [<I $bits>] {
                fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                    wr.[<write_un $base_bits>]($bits, self.0 as [<u $base_bits>])
                }
            }

            impl<'i> DeserializeShrinkWrap<'i> for [<I $bits>] {
                fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>, _s: ElementSize) -> Result<Self, Error> {
                    let val = rd.[<read_un $base_bits>]($bits)?;
                    let is_negative = val & (1 << ($bits - 1)) != 0;
                    if is_negative {
                        const SIGN_EXTEND: [<u $base_bits>] = [<u $base_bits>]::MAX << $bits;
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

inx!(2, 3, 4, 5, 6, 7 / 8);
inx!(9, 10, 11, 12, 13, 14, 15 / 16);
inx!(17, 18, 19, 20, 21, 22, 23, 24 / 32);
inx!(25, 26, 27, 28, 29, 30, 31 / 32);
inx!(33, 34, 35, 36, 37, 38, 39, 40 / 64);
inx!(41, 42, 43, 44, 45, 46, 47, 48 / 64);
inx!(49, 50, 51, 52, 53, 54, 55, 56 / 64);
inx!(57, 58, 59, 60, 61, 62, 63 / 64);
