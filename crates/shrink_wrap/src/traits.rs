use crate::{BufReader, BufWriter, Error};

pub trait SerializeShrinkWrap {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error>;
}

#[derive(Copy, Clone)]
pub enum ElementSize {
    Implied,
    /// Element size is unknown and stored at the back of the buffer.
    Unsized,
    /// Element size is known and not stored in a buffer.
    Sized {
        size_bytes: usize,
    },
    /// Elements size is unknown, but deserializer is able to differ them apart (e.g. LEB or VLU16N).
    /// Element size is not stored as with Sized.
    UnsizedSelfDescribing,
}

pub trait DeserializeShrinkWrap<'i>: Sized {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        element_size: ElementSize,
    ) -> Result<Self, Error>;
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

impl<'i> DeserializeShrinkWrap<'i> for u8 {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        _element_size: ElementSize,
    ) -> Result<Self, Error> {
        rd.read_u8()
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Option<T> {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        element_size: ElementSize,
    ) -> Result<Self, Error> {
        let is_some = rd.read_bool()?;
        if is_some {
            Ok(Some(rd.read(element_size)?))
        } else {
            Ok(None)
        }
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>, E: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i>
    for Result<T, E>
{
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        element_size: ElementSize,
    ) -> Result<Self, Error> {
        let is_ok = rd.read_bool()?;
        if is_ok {
            Ok(Ok(rd.read(element_size)?))
        } else {
            Ok(Err(rd.read(element_size)?))
        }
    }
}
