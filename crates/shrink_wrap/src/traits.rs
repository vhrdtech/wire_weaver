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

impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Option<T> {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            Some(val) => {
                wr.write_bool(true)?;
                wr.write(val)
            }
            None => wr.write_bool(false),
        }
    }
}

impl<T: SerializeShrinkWrap, E: SerializeShrinkWrap> SerializeShrinkWrap for Result<T, E> {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            Ok(val) => {
                wr.write_bool(true)?;
                wr.write(val)
            }
            Err(err_code) => {
                wr.write_bool(false)?;
                wr.write(err_code)
            }
        }
    }
}
