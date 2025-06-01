use crate::{BufReader, BufWriter, Error};

pub trait SerializeShrinkWrap {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error>;
}

#[derive(Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ElementSize {
    Implied,
    /// Element size is unknown and stored at the back of the buffer.
    Unsized,
    /// Element size is known and not stored in a buffer.
    Sized {
        size_bits: usize,
    },
    /// Elements size is unknown, but deserializer is able to differ them apart (e.g. LEB or NIB16).
    /// Element size is not stored as with Sized.
    UnsizedSelfDescribing,
}
//
// impl ElementSize {
//     pub fn most_constraining(&self, other: ElementSize) -> ElementSize {
//         if self == ElementSize::Unsized || other == ElementSize::Unsized {
//             return ElementSize::Unsized
//         }
//
//     }
// }

pub trait DeserializeShrinkWrap<'i>: Sized {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        element_size: ElementSize,
    ) -> Result<Self, Error>;
}

macro_rules! impl_serialize {
    ($ty:ty, $write_fn:ident) => {
        impl SerializeShrinkWrap for $ty {
            fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                wr.$write_fn(*self)
            }
        }
    };
}
impl_serialize!(bool, write_bool);
impl_serialize!(u8, write_u8);
impl_serialize!(u16, write_u16);
impl_serialize!(u32, write_u32);
impl_serialize!(u64, write_u64);
impl_serialize!(u128, write_u128);
impl_serialize!(i8, write_i8);
impl_serialize!(i16, write_i16);
impl_serialize!(i32, write_i32);
impl_serialize!(i64, write_i64);
impl_serialize!(i128, write_i128);
impl_serialize!(f32, write_f32);
impl_serialize!(f64, write_f64);

macro_rules! impl_deserialize {
    ($ty:ty, $read_fn:ident) => {
        impl<'i> DeserializeShrinkWrap<'i> for $ty {
            fn des_shrink_wrap<'di>(
                rd: &'di mut BufReader<'i>,
                _element_size: ElementSize,
            ) -> Result<Self, Error> {
                rd.$read_fn()
            }
        }
    };
}
impl_deserialize!(bool, read_bool);
impl_deserialize!(u8, read_u8);
impl_deserialize!(u16, read_u16);
impl_deserialize!(u32, read_u32);
impl_deserialize!(u64, read_u64);
impl_deserialize!(u128, read_u128);
impl_deserialize!(i8, read_i8);
impl_deserialize!(i16, read_i16);
impl_deserialize!(i32, read_i32);
impl_deserialize!(i64, read_i64);
impl_deserialize!(i128, read_i128);
impl_deserialize!(f32, read_f32);
impl_deserialize!(f64, read_f64);

impl<'i> SerializeShrinkWrap for &'i str {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_raw_str(self)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for &'i str {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        _element_size: ElementSize,
    ) -> Result<Self, Error> {
        rd.read_raw_str()
    }
}

// Won't work with reordered bool's and u4's
// Custom implementation is provided, which also allows to place is_ok and is_some flags manually.
// impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Option<T> {
//     fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
//         match self {
//             Some(val) => {
//                 wr.write_bool(true)?;
//                 wr.write(val)
//             }
//             None => wr.write_bool(false),
//         }
//     }
// }
//
// impl<T: SerializeShrinkWrap, E: SerializeShrinkWrap> SerializeShrinkWrap for Result<T, E> {
//     fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
//         match self {
//             Ok(val) => {
//                 wr.write_bool(true)?;
//                 wr.write(val)
//             }
//             Err(err_code) => {
//                 wr.write_bool(false)?;
//                 wr.write(err_code)
//             }
//         }
//     }
// }
//
// impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Option<T> {
//     fn des_shrink_wrap<'di>(
//         rd: &'di mut BufReader<'i>,
//         element_size: ElementSize,
//     ) -> Result<Self, Error> {
//         let is_some = rd.read_bool()?;
//         if is_some {
//             Ok(Some(rd.read(element_size)?))
//         } else {
//             Ok(None)
//         }
//     }
// }
//
// impl<'i, T: DeserializeShrinkWrap<'i>, E: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i>
//     for Result<T, E>
// {
//     fn des_shrink_wrap<'di>(
//         rd: &'di mut BufReader<'i>,
//         element_size: ElementSize,
//     ) -> Result<Self, Error> {
//         let is_ok = rd.read_bool()?;
//         if is_ok {
//             Ok(Ok(rd.read(element_size)?))
//         } else {
//             Ok(Err(rd.read(element_size)?))
//         }
//     }
// }
