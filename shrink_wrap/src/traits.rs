use crate::vec::write_item;
use crate::{BufReader, BufWriter, Error};
use paste::paste;

pub trait SerializeShrinkWrap {
    const ELEMENT_SIZE: ElementSize;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error>;
}

pub trait DeserializeShrinkWrap<'i>: Sized {
    const ELEMENT_SIZE: ElementSize;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error>;
}

#[derive(Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ElementSize {
    /// Element size is unknown and stored at the back of the buffer.
    Unsized,
    /// Element size is known and not stored in a buffer.
    Sized { size_bits: usize },
    /// Element's size is unknown, but deserializer is able to differ them apart (e.g., LEB or NIB16).
    /// Element size is not stored as with Sized.
    UnsizedSelfDescribing,
    // When deserializing root enum or struct
    // Implied,
}

impl ElementSize {
    pub const fn most_constraining(&self, other: ElementSize) -> ElementSize {
        // if self == &ElementSize::Unsized || other == ElementSize::Unsized {
        //     ElementSize::Unsized
        // } else if self == &ElementSize::UnsizedSelfDescribing || other == ElementSize::UnsizedSelfDescribing {
        //     ElementSize::UnsizedSelfDescribing
        // } else {
        //
        // }
        match (self, other) {
            (ElementSize::Unsized, _) => ElementSize::Unsized,
            (_, ElementSize::Unsized) => ElementSize::Unsized,
            (ElementSize::UnsizedSelfDescribing, _) => ElementSize::UnsizedSelfDescribing,
            (_, ElementSize::UnsizedSelfDescribing) => ElementSize::UnsizedSelfDescribing,
            (
                ElementSize::Sized { size_bits: size_a },
                ElementSize::Sized { size_bits: size_b },
            ) => {
                if *size_a >= size_b {
                    ElementSize::Sized { size_bits: *size_a }
                } else {
                    ElementSize::Sized { size_bits: size_b }
                }
            }
        }
    }
}

impl SerializeShrinkWrap for bool {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 1 };

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_bool(*self)
    }
}

macro_rules! impl_serialize {
    ($sign:ident, $bits:literal) => {
        paste! {
            impl SerializeShrinkWrap for [<$sign $bits>] {
                const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: $bits };

                fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                    wr.[<write_ $sign $bits>](*self)
                }
            }
        }
    };
}
impl_serialize!(u, 8);
impl_serialize!(u, 16);
impl_serialize!(u, 32);
impl_serialize!(u, 64);
impl_serialize!(u, 128);
impl_serialize!(i, 8);
impl_serialize!(i, 16);
impl_serialize!(i, 32);
impl_serialize!(i, 64);
impl_serialize!(i, 128);
impl_serialize!(f, 32);
impl_serialize!(f, 64);

impl<'i> DeserializeShrinkWrap<'i> for bool {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 1 };

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        rd.read_bool()
    }
}

macro_rules! impl_deserialize {
    ($sign:ident, $bits:literal) => {
        paste! {
            impl<'i> DeserializeShrinkWrap<'i> for [<$sign $bits>] {
                const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: $bits };

                fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
                    rd.[<read_ $sign $bits>]()
                }
            }
        }
    };
}
impl_deserialize!(u, 8);
impl_deserialize!(u, 16);
impl_deserialize!(u, 32);
impl_deserialize!(u, 64);
impl_deserialize!(u, 128);
impl_deserialize!(i, 8);
impl_deserialize!(i, 16);
impl_deserialize!(i, 32);
impl_deserialize!(i, 64);
impl_deserialize!(i, 128);
impl_deserialize!(f, 32);
impl_deserialize!(f, 64);

impl SerializeShrinkWrap for &'_ str {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_raw_str(self)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for &'i str {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        rd.read_raw_str()
    }
}

impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Option<T> {
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            Some(val) => {
                wr.write_bool(true)?;
                write_item(wr, val)
            }
            None => wr.write_bool(false),
        }
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Option<T> {
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE;
    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let is_some = rd.read_bool()?;
        if is_some {
            if T::ELEMENT_SIZE == ElementSize::Unsized {
                let size = rd.read_unib32_rev()?;
                let mut rd_split = rd.split(size as usize)?;
                Ok(Some(rd_split.read()?))
            } else {
                Ok(Some(rd.read()?))
            }
        } else {
            Ok(None)
        }
    }
}

impl<T: SerializeShrinkWrap, E: SerializeShrinkWrap> SerializeShrinkWrap for Result<T, E> {
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE.most_constraining(E::ELEMENT_SIZE);

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            Ok(val) => {
                wr.write_bool(true)?;
                write_item(wr, val)
            }
            Err(err) => {
                wr.write_bool(false)?;
                write_item(wr, err)
            }
        }
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>, E: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i>
    for Result<T, E>
{
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE.most_constraining(E::ELEMENT_SIZE);

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let is_ok = rd.read_bool()?;
        if is_ok {
            if T::ELEMENT_SIZE == ElementSize::Unsized {
                let size = rd.read_unib32_rev()?;
                let mut rd_split = rd.split(size as usize)?;
                Ok(Ok(rd_split.read()?))
            } else {
                Ok(Ok(rd.read()?))
            }
        } else if E::ELEMENT_SIZE == ElementSize::Unsized {
            let size = rd.read_unib32_rev()?;
            let mut rd_split = rd.split(size as usize)?;
            Ok(Err(rd_split.read()?))
        } else {
            Ok(Err(rd.read()?))
        }
    }
}
