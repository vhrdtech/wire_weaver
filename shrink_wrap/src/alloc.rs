use crate::{BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};

impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Vec<T> {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let Ok(len_u16) = u16::try_from(self.len()) else {
            return Err(Error::VecTooLong);
        };
        wr.write_u16_rev(len_u16)?;
        for item in self {
            wr.write(item)?;
        }
        Ok(())
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Vec<T> {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        element_size: ElementSize,
    ) -> Result<Self, Error> {
        let elements_count = rd.read_unib32_rev()?;

        #[cfg(feature = "tracing-extended")]
        tracing::trace!("Vec element count: {}", elements_count);

        let mut items = vec![];
        for _ in 0..elements_count {
            let item = rd.read(element_size)?;
            items.push(item);
        }
        Ok(items)
    }
}

impl SerializeShrinkWrap for String {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_raw_str(self.as_str())
    }
}

impl<'i> DeserializeShrinkWrap<'i> for String {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        _element_size: ElementSize,
    ) -> Result<Self, Error> {
        Ok(String::from(rd.read_raw_str()?))
    }
}
