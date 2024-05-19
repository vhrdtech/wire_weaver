use crate::{BufReader, BufWriter, Error};

pub trait SerializeShrinkWrap {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error>;
}

pub trait DeserializeShrinkWrap<'i>: Sized {
    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error>;
}

impl SerializeShrinkWrap for bool {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_bool(*self)
    }
}

impl SerializeShrinkWrap for u8 {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_u8(*self)
    }
}

// Won't work with reordered bool's and u4's
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

impl<'i> DeserializeShrinkWrap<'i> for u8 {
    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        rd.read_u8()
    }
}

// impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Option<T> {
//     fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
//         let is_some = rd.read_bool()?;
//         if is_some {
//             Ok(Some(rd.read()?))
//         } else {
//             Ok(None)
//         }
//     }
// }
// impl<'i, T: DeserializeShrinkWrap<'i>, E: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Result<T, E> {
//     fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
//         let is_ok = rd.read_bool()?;
//         if is_ok {
//             Ok(Ok(rd.read()?))
//         } else {
//             Ok(Err(rd.read()?))
//         }
//     }
// }
