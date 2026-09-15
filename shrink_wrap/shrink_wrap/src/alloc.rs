use crate::{
    BufReader, BufWriter, BufWriterOwned, DeserializeShrinkWrap, DeserializeShrinkWrapOwned,
    ElementSize, Error, SerializeShrinkWrap, SerializeShrinkWrapOwned,
};

impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Vec<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_rev_len(self.len())?;
        for item in self {
            wr.write(item)?;
        }
        Ok(())
    }
}

impl<T: SerializeShrinkWrapOwned> SerializeShrinkWrapOwned for Vec<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), Error> {
        wr.write_rev_len(self.len())?;
        for item in self {
            wr.write(item)?;
        }
        Ok(())
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Vec<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let elements_count = rd.read_rev_len()?;

        #[cfg(feature = "defmt-extended")]
        defmt::trace!("Vec element count: {}", elements_count);
        #[cfg(feature = "tracing-extended")]
        tracing::trace!("Vec element count: {}", elements_count);

        let mut items = vec![];
        for _ in 0..elements_count {
            let item = rd.read()?;
            items.push(item);
        }
        Ok(items)
    }
}

impl<T: DeserializeShrinkWrapOwned> DeserializeShrinkWrapOwned for Vec<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        let elements_count = rd.read_rev_len()?;

        #[cfg(feature = "defmt-extended")]
        defmt::trace!("Vec element count: {}", elements_count);
        #[cfg(feature = "tracing-extended")]
        tracing::trace!("Vec element count: {}", elements_count);

        let mut items = vec![];
        for _ in 0..elements_count {
            let item = rd.read_owned()?;
            items.push(item);
        }
        Ok(items)
    }
}

impl SerializeShrinkWrap for String {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_str(self.as_str())
    }
}

impl SerializeShrinkWrapOwned for String {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), Error> {
        wr.write_str(self.as_str())
    }
}

impl<'i> DeserializeShrinkWrap<'i> for String {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(String::from(rd.read_str()?))
    }
}

impl DeserializeShrinkWrapOwned for String {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        Ok(String::from(rd.read_str()?))
    }
}

impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Box<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        T::ser_shrink_wrap(self, wr)
    }
}

impl<T: SerializeShrinkWrapOwned> SerializeShrinkWrapOwned for Box<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), Error> {
        T::ser_shrink_wrap_owned(self, wr)
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Box<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let value = T::des_shrink_wrap(rd)?;
        Ok(Box::new(value))
    }
}

impl<T: DeserializeShrinkWrapOwned> DeserializeShrinkWrapOwned for Box<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        let value = T::des_shrink_wrap_owned(rd)?;
        Ok(Box::new(value))
    }
}
