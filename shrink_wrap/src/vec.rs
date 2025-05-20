use core::fmt::{Debug, Formatter};

use crate::traits::ElementSize;
use crate::{BufReader, BufWriter, DeserializeShrinkWrap, Error, SerializeShrinkWrap};

// pub enum Vec<'i, T, const S: u32, F> where F: Fn(usize) -> Option<T> {
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RefVec<'i, T> {
    // pub enum Vec<'i, T, const S: u32, I> where I: Iterator<Item=T> {
    Slice {
        slice: &'i [T],
        element_size: ElementSize,
    },
    // Iterator {
    //     it: I
    // },
    Buf {
        buf: BufReader<'i>,
        elements_count: u32,
        element_size: ElementSize,
    },
    // Gen {
    //     gen: F,
    //     len_elements: usize,
    //     // element_size: ElementSize,
    // },
}

impl<'i, T> RefVec<'i, T> {
    pub fn element_size(&self) -> ElementSize {
        match self {
            RefVec::Slice { element_size, .. } => *element_size,
            RefVec::Buf { element_size, .. } => *element_size,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            RefVec::Slice { slice, .. } => slice.len(),
            RefVec::Buf { elements_count, .. } => *elements_count as usize,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'i, T> RefVec<'i, T>
where
    T: DeserializeShrinkWrap<'i>,
    // F: Fn(usize) -> Option<T>
    // I: Iterator<Item=T>,
{
    pub fn iter(&self) -> RefVecIter<'i, T> {
        match self {
            RefVec::Slice { .. } => {
                unimplemented!("RefVec slice iter")
            }
            RefVec::Buf {
                buf,
                elements_count,
                element_size,
            } => RefVecIter::Buf {
                buf: *buf,
                elements_count: *elements_count,
                element_size: *element_size,
                pos: 0,
            },
        }
    }
}

// implementing separately because partial specialisation is not yet supported
impl<'i> RefVec<'i, u8> {
    pub fn new_byte_slice(slice: &'i [u8]) -> Self {
        RefVec::Slice {
            slice,
            element_size: ElementSize::Sized { size_bits: 8 },
        }
    }

    pub fn ser_shrink_wrap_vec_u8(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let len = self.len();
        let Ok(len_u16) = u16::try_from(len) else {
            return Err(Error::VecTooLong);
        };
        wr.write_u16_rev(len_u16)?;
        let len = self.len();
        match self {
            RefVec::Slice { slice, .. } => {
                wr.write_raw_slice(slice)?;
            }
            RefVec::Buf { buf, .. } => {
                let mut buf = *buf;
                let slice = buf.read_raw_slice(len)?;
                wr.write_raw_slice(slice)?;
            }
        }
        Ok(())
    }

    pub fn as_slice(&self) -> &[u8] {
        match self {
            RefVec::Slice { slice, .. } => slice,
            RefVec::Buf {
                buf,
                elements_count,
                ..
            } => {
                let mut buf = *buf;
                // RefVec::Buf is created during deserialization, at which point it is checked that there
                // are actually element_count bytes available, see DeserializeShrinkWrap below.
                buf.read_raw_slice(*elements_count as usize).unwrap_or(&[])
            }
        }
    }
}

impl<'i, T> SerializeShrinkWrap for RefVec<'i, T>
where
    T: SerializeShrinkWrap + DeserializeShrinkWrap<'i>,
{
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        if matches!(self.element_size(), ElementSize::Implied) {
            return Err(Error::ImpliedSizeInVec);
        }
        let is_unsized = matches!(self.element_size(), ElementSize::Unsized);
        match self {
            RefVec::Slice { slice, .. } => {
                let Ok(elements_count) = u16::try_from(slice.len()) else {
                    return Err(Error::VecTooLong);
                };
                wr.write_u16_rev(elements_count)?;
                for item in slice.iter() {
                    ser_item(wr, is_unsized, item)?;
                }
            }
            RefVec::Buf { elements_count, .. } => {
                let Ok(elements_count) = u16::try_from(*elements_count) else {
                    return Err(Error::VecTooLong);
                };
                wr.write_u16_rev(elements_count)?;
                for item in self.iter() {
                    let item = item?;
                    ser_item(wr, is_unsized, &item)?;
                }
            }
        }
        Ok(())
    }
}

fn ser_item<T: SerializeShrinkWrap>(
    wr: &mut BufWriter,
    is_unsized: bool,
    item: &T,
) -> Result<(), Error> {
    let unsized_start = wr.pos().0;
    let u16_rev_from = if is_unsized {
        wr.align_byte();
        Some(wr.u16_rev_pos())
    } else {
        None
    };
    wr.write(item)?;
    if let Some(u16_rev_from) = u16_rev_from {
        wr.encode_nib16_rev(u16_rev_from, wr.u16_rev_pos())?;
        let size = wr.pos().0 - unsized_start;
        let Ok(size) = u16::try_from(size) else {
            return Err(Error::ItemTooLong);
        };
        wr.write_u16_rev(size)?;
    }
    Ok(())
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for RefVec<'i, T> {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        element_size: ElementSize,
    ) -> Result<Self, Error> {
        let elements_count = rd.read_unib32_rev()?;

        #[cfg(feature = "defmt-extended")]
        defmt::trace!("Vec element count: {}", elements_count);
        #[cfg(feature = "tracing-extended")]
        tracing::trace!("Vec element count: {}", elements_count);

        // save BufReader state and read out elements to advance beyond Vec
        let buf = *rd;
        for _ in 0..elements_count {
            let _item: T = rd.read(element_size)?;
        }

        Ok(RefVec::Buf {
            buf,
            elements_count,
            element_size,
        })
    }
}

impl<'i> core::ops::Deref for RefVec<'i, u8> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

pub enum RefVecIter<'i, T> {
    Slice {
        slice: &'i [T],
        pos: usize,
    },
    Buf {
        buf: BufReader<'i>,
        elements_count: u32,
        element_size: ElementSize,
        pos: u32,
    },
    // Gen {
    //     gen: F,
    //     len_elements: usize,
    //     // element_size: ElementSize,
    // },
}

impl<'i, T: DeserializeShrinkWrap<'i>> Iterator for RefVecIter<'i, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            RefVecIter::Slice { .. } => {
                unimplemented!("RefVecIter::Slice")
            }
            RefVecIter::Buf {
                buf,
                elements_count,
                element_size,
                pos,
            } => {
                if *pos == *elements_count {
                    return None;
                }
                *pos += 1;
                let item = match element_size {
                    ElementSize::Implied => {
                        *pos = *elements_count;
                        return Some(Err(Error::ImpliedSizeInVec));
                    }
                    ElementSize::Unsized => {
                        let len = match buf.read_unib32_rev() {
                            Ok(len) => len,
                            Err(e) => {
                                return Some(Err(e));
                            }
                        };
                        // dbg!(len);
                        let mut buf = match buf.split(len as usize) {
                            Ok(buf) => buf,
                            Err(e) => {
                                return Some(Err(e));
                            }
                        };
                        buf.read(*element_size)
                    }
                    ElementSize::Sized { .. } => buf.read(*element_size),
                    ElementSize::UnsizedSelfDescribing => buf.read(*element_size),
                };
                Some(item)
            }
        }
    }
}

impl<'i, T: DeserializeShrinkWrap<'i> + Debug> Debug for RefVec<'i, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("[")?;
        let it = self.iter();
        let len = self.len();
        for (i, elem) in it.enumerate() {
            match elem {
                Ok(elem) => write!(f, "{elem:02X?}")?,
                Err(e) => write!(f, "{e:?}")?,
            }
            if i < len - 1 {
                f.write_str(", ")?;
            }
        }
        f.write_str("]")
    }
}

impl<'i> PartialEq for RefVec<'i, u8> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<'i> Eq for RefVec<'i, u8> {}

#[cfg(test)]
mod tests {
    use crate::traits::ElementSize;
    use crate::vec::RefVec;
    use crate::{BufReader, BufWriter, DeserializeShrinkWrap, Error, SerializeShrinkWrap};

    #[test]
    fn read_vec_sized() {
        let buf = [0xAB, 0xCD, 0x02];
        let mut rd = BufReader::new(&buf);
        let arr: RefVec<'_, u8> = rd.read(ElementSize::Sized { size_bits: 8 }).unwrap();
        let mut iter = arr.iter();
        assert_eq!(iter.next(), Some(Ok(0xAB)));
        assert_eq!(iter.next(), Some(Ok(0xCD)));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn write_vec_sized() {
        let mut buf = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf);
        let arr = RefVec::Slice {
            slice: &[0xABu8, 0xCD],
            element_size: ElementSize::Sized { size_bits: 8 },
        };
        wr.write(&arr).unwrap();
        assert_eq!(wr.finish(), Ok(&[0xAB, 0xCD, 0x02][..]));
    }

    #[test]
    fn read_vec_unsized() {
        let buf = [0xAB, 0x12, 0x34, 0x56, 0xCD, 0x78, 0x02, 0x42];
        let mut rd = BufReader::new(&buf);
        let arr: RefVec<'_, u8> = rd.read(ElementSize::Unsized).unwrap();
        let mut iter = arr.iter();
        assert_eq!(iter.next(), Some(Ok(0xAB)));
        assert_eq!(iter.next(), Some(Ok(0xCD)));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn write_vec_unsized() {
        let mut buf = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf);

        struct Evolved<'i> {
            byte: u8,
            additional_data: &'i [u8],
        }
        impl<'i> SerializeShrinkWrap for Evolved<'i> {
            fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                wr.write_u8(self.byte)?;
                wr.write_raw_slice(self.additional_data)
            }
        }
        impl<'i> DeserializeShrinkWrap<'i> for Evolved<'i> {
            fn des_shrink_wrap<'di>(
                _rd: &'di mut BufReader<'i>,
                _element_size: ElementSize,
            ) -> Result<Self, Error> {
                unimplemented!()
            }
        }

        let arr = RefVec::Slice {
            slice: &[
                Evolved {
                    byte: 0xAB,
                    additional_data: &[0x12, 0x34, 0x56],
                },
                Evolved {
                    byte: 0xCD,
                    additional_data: &[0x78],
                },
            ],
            element_size: ElementSize::Unsized,
        };
        wr.write(&arr).unwrap();
        assert_eq!(
            wr.finish(),
            Ok(&[0xAB, 0x12, 0x34, 0x56, 0xCD, 0x78, 0x02, 0x42][..])
        );
    }
}
