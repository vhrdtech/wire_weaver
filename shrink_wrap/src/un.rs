macro_rules! write_unx {
    ($fn_name:ident, $base_ty:ty, $max_bit_count:literal) => {
        #[doc = "Write up to "]
        #[doc = stringify!($max_bit_count)]
        #[doc = " bits from "]
        #[doc = stringify!($base_ty)]
        #[doc = " number."]
        pub fn $fn_name(&mut self, bit_count: u8, value: $base_ty) -> Result<(), Error> {
            if bit_count > $max_bit_count {
                return Err(Error::InvalidBitCount);
            }

            let mut bits_left = bit_count;
            while bits_left > 0 {
                if (self.bytes_left() == 0) && self.bit_idx == 7 {
                    return Err(Error::OutOfBounds);
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
    };
}
pub(crate) use write_unx;

macro_rules! read_unx {
    ($fn_name:ident, $base_ty:ty, $max_bit_count:literal) => {
        #[doc = "Read up to "]
        #[doc = stringify!($max_bit_count)]
        #[doc = " bits into "]
        #[doc = stringify!($base_ty)]
        #[doc = " number."]
        pub fn $fn_name(&mut self, bit_count: u8) -> Result<$base_ty, Error> {
            if bit_count > $max_bit_count {
                return Err(Error::InvalidBitCount);
            }

            let mut result: $base_ty = 0;
            let mut bits_left = bit_count;

            while bits_left > 0 {
                if (self.bytes_left() == 0) && self.bit_idx == 7 {
                    return Err(Error::OutOfBounds);
                }

                let bits_to_read = bits_left.min(self.bit_idx + 1);
                let mask = ((1 as u8) << bits_to_read) - 1;
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
    };
}
pub(crate) use read_unx;
