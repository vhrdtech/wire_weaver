use crate::serdes::bit_buf;
use crate::serdes::traits::SerializeBits;
use crate::serdes::{BitBuf, DeserializeBits, DeserializeVlu4, NibbleBuf};

#[macro_export]
macro_rules! max_bound_number {
    ($type_name: ident, $base_type: ty, $max: literal, $fmt: literal) => {
        #[derive(Copy, Clone, Eq, PartialEq, Debug)]
        pub struct $type_name($base_type);
        impl $type_name {
            pub const fn new(x: $base_type) -> Option<$type_name> {
                if x <= $max {
                    Some($type_name(x))
                } else {
                    None
                }
            }

            pub unsafe fn new_unchecked(x: $base_type) -> $type_name {
                $type_name(x)
            }

            pub const fn inner(&self) -> $base_type {
                self.0
            }
        }

        impl core::fmt::Display for $type_name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                if f.alternate() {
                    write!(f, "{}", self.0)
                } else {
                    write!(f, $fmt, self.0)
                }
            }
        }
    };

    ($type_name: ident, $bit_count: literal, $base_type: ty, $max: literal, $fmt: literal, $ser: ident, $des: ident) => {
        max_bound_number!($type_name, $base_type, $max, $fmt);

        impl SerializeBits for $type_name {
            type Error = bit_buf::Error;

            fn ser_bits(&self, wgr: &mut bit_buf::BitBufMut) -> Result<(), Self::Error> {
                wgr.$ser($bit_count, self.0)
            }
        }

        impl<'i> DeserializeBits<'i> for $type_name {
            type Error = bit_buf::Error;

            fn des_bits<'di>(rdr: &'di mut bit_buf::BitBuf<'i>) -> Result<Self, Self::Error> {
                Ok($type_name(rdr.$des($bit_count)?))
            }
        }
    };
}
pub use max_bound_number;

// 2 bit unsigned integer
max_bound_number!(U2, 2, u8, 3, "U2:{}", put_up_to_8, get_up_to_8);
// 3 bit unsigned integer
max_bound_number!(U3, 3, u8, 7, "U3:{}", put_up_to_8, get_up_to_8);

// 4 bit unsigned integer
max_bound_number!(U4, 4, u8, 15, "U4:{}", put_up_to_8, get_up_to_8);
impl<'i> DeserializeVlu4<'i> for U4 {
    type Error = crate::serdes::nibble_buf::Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        Ok(U4(rdr.get_nibble()?))
    }
}

// 6 bit unsigned integer
max_bound_number!(U6, 6, u8, 63, "U6:{}", put_up_to_8, get_up_to_8);

/// 7 bit unsigned integer shifted +1 == range 1..=128
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct U7Sp1(u8);
impl U7Sp1 {
    pub const fn new(from: u8) -> Option<Self> {
        if from >= 1 && from <= 128 {
            Some(U7Sp1(from - 1))
        } else {
            None
        }
    }

    pub fn to_u8(&self) -> u8 {
        self.0 + 1
    }
}

impl<'i> DeserializeBits<'i> for U7Sp1 {
    type Error = crate::serdes::bit_buf::Error;

    fn des_bits<'di>(rdr: &'di mut BitBuf<'i>) -> Result<Self, Self::Error> {
        let bits_6_0 = rdr.get_up_to_8(7)?;
        Ok(U7Sp1(bits_6_0))
    }
}

// /// 2 bit unsigned integer shifted +1 == range 1..=4
// #[derive(Copy, Clone, Debug, PartialEq, Eq)]
// pub struct U2Sp1(u8);
// impl U2Sp1 {
//     pub const fn new(from: u8) -> Option<Self> {
//         if from >= 1 && from <= 4 {
//             Some(U2Sp1(from - 1))
//         } else {
//             None
//         }
//     }
//
//     pub fn to_u8(&self) -> u8 {
//         self.0 + 1
//     }
// }
//
// impl<'i> DeserializeBits<'i> for U2Sp1 {
//     type Error = bit_buf::Error;
//
//     fn des_bits<'di>(rdr: &'di mut BitBuf<'i>) -> Result<Self, Self::Error> {
//         let bits_1_0 = rdr.get_up_to_8(2)?;
//         Ok(U2Sp1(bits_1_0))
//     }
// }
//
// impl SerializeBits for U2Sp1 {
//     type Error = bit_buf::Error;
//
//     fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
//         wgr.put_up_to_8(2, self.0)
//     }
// }
