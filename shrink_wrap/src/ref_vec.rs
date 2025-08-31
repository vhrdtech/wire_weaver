use core::fmt::{Debug, Formatter};

use crate::traits::ElementSize;
use crate::{BufReader, BufWriter, DeserializeShrinkWrap, Error, SerializeShrinkWrap};

#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RefVec<'i, T> {
    Slice {
        slice: &'i [T],
    },
    Buf {
        buf: BufReader<'i>,
        elements_count: u32,
    },
    // Iterator { ?
    //     it: I
    // },
    // Gen { ?
    //     gen: F,
    //     len_elements: usize,
    //     // element_size: ElementSize,
    // },
}

impl<T> RefVec<'_, T> {
    pub const fn new() -> Self {
        Self::Slice { slice: &[] }
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

impl<T> Default for RefVec<'_, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'i, T> RefVec<'i, T>
where
    T: DeserializeShrinkWrap<'i>,
{
    pub fn iter(&self) -> RefVecIter<'i, T> {
        match self {
            RefVec::Slice { slice, .. } => RefVecIter::Slice { slice, pos: 0 },
            RefVec::Buf {
                buf,
                elements_count,
            } => RefVecIter::Buf {
                buf: *buf,
                elements_count: *elements_count,
                pos: 0,
            },
        }
    }
}

// implementing separately because partial specialization is not yet supported
impl<'i> RefVec<'i, u8> {
    pub fn new_bytes(slice: &'i [u8]) -> Self {
        RefVec::Slice { slice }
    }

    pub fn ser_shrink_wrap_vec_u8(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let len = self.len();
        let Ok(len_u16) = u16::try_from(len) else {
            return Err(Error::VecTooLong);
        };
        // len == size in bytes when serialized, so this works
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

impl<'i> RefVec<'i, &'i str> {
    pub fn new_str_slice(str_slice: &'i [&'i str]) -> Self {
        RefVec::Slice { slice: str_slice }
    }
}

impl<'i, T> SerializeShrinkWrap for RefVec<'i, T>
where
    T: SerializeShrinkWrap + DeserializeShrinkWrap<'i> + Clone,
{
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            RefVec::Slice { slice, .. } => {
                let Ok(elements_count) = u16::try_from(slice.len()) else {
                    return Err(Error::VecTooLong);
                };
                wr.write_u16_rev(elements_count)?;
                for item in slice.iter() {
                    wr.write(item)?;
                }
            }
            RefVec::Buf { elements_count, .. } => {
                let Ok(elements_count) = u16::try_from(*elements_count) else {
                    return Err(Error::VecTooLong);
                };
                wr.write_u16_rev(elements_count)?;
                for item in self.iter() {
                    let item = item?;
                    wr.write(&item)?;
                }
            }
        }
        Ok(())
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for RefVec<'i, T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let elements_count = rd.read_unib32_rev()?;

        #[cfg(feature = "defmt-extended")]
        defmt::trace!("Vec element count: {}", elements_count);
        #[cfg(feature = "tracing-extended")]
        tracing::trace!("Vec element count: {}", elements_count);

        // save BufReader state and read out elements to advance beyond Vec
        let buf = *rd;
        for _ in 0..elements_count {
            let _item: T = rd.read()?;
        }

        Ok(RefVec::Buf {
            buf,
            elements_count,
        })
    }
}

impl core::ops::Deref for RefVec<'_, u8> {
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
        pos: u32,
    },
    // Gen {
    //     gen: F,
    //     len_elements: usize,
    //     // element_size: ElementSize,
    // },
}

impl<'i, T: DeserializeShrinkWrap<'i> + Clone> Iterator for RefVecIter<'i, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            RefVecIter::Slice { slice, pos } => {
                if *pos >= slice.len() {
                    return None;
                }
                let idx = *pos;
                *pos += 1;
                Some(Ok(slice[idx].clone())) // Clone requirement in multiple places only because of this line
            }
            RefVecIter::Buf {
                buf,
                elements_count,
                pos,
            } => {
                if *pos >= *elements_count {
                    return None;
                }
                *pos += 1;
                let item = buf.read();
                Some(item)
            }
        }
    }
}

impl<'i, T: DeserializeShrinkWrap<'i> + Debug + Clone> Debug for RefVec<'i, T> {
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

// impl<'i> PartialEq for RefVec<'i, u8> {
//     fn eq(&self, other: &Self) -> bool {
//         self.as_slice() == other.as_slice()
//     }
// }
//
// impl<'i> Eq for RefVec<'i, u8> {}

impl<'i, T: DeserializeShrinkWrap<'i> + PartialEq + Clone> PartialEq for RefVec<'i, T> {
    fn eq(&self, other: &Self) -> bool {
        let mut other = other.iter();
        for x in self.iter() {
            let Ok(x) = x else {
                return false;
            };
            let Some(y) = other.next() else {
                return false;
            };
            let Ok(y) = y else {
                return false;
            };
            if x != y {
                return false;
            }
        }
        true
    }
}

impl<'i, T: DeserializeShrinkWrap<'i> + Eq + Clone> Eq for RefVec<'i, T> {}

#[cfg(test)]
mod tests {
    use crate::ref_vec::RefVec;
    use crate::traits::ElementSize;
    use crate::un::U7;
    use crate::{BufReader, BufWriter, DeserializeShrinkWrap, Error, SerializeShrinkWrap};
    use hex_literal::hex;

    #[test]
    fn read_vec_sized() {
        let buf = [0xAB, 0xCD, 0x02];
        let mut rd = BufReader::new(&buf);
        let arr: RefVec<'_, u8> = rd.read().unwrap();
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
        };
        wr.write(&arr).unwrap();
        assert_eq!(wr.finish(), Ok(&[0xAB, 0xCD, 0x02][..]));
    }

    #[test]
    fn read_vec_unsized() {
        #[derive(Clone, PartialEq, Debug)]
        struct Old {
            byte: u8,
        }
        impl<'i> DeserializeShrinkWrap<'i> for Old {
            const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

            fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
                Ok(Old { byte: rd.read()? })
            }
        }

        let buf = [0xAB, 0x12, 0x34, 0x56, 0xCD, 0x78, 0x02, 0x42];
        let mut rd = BufReader::new(&buf);
        let arr: RefVec<'_, Old> = rd.read().unwrap();
        let mut iter = arr.iter();
        assert_eq!(iter.next(), Some(Ok(Old { byte: 0xAB })));
        assert_eq!(iter.next(), Some(Ok(Old { byte: 0xCD })));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn write_vec_unsized() {
        let mut buf = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf);

        #[derive(Clone)]
        struct Evolved<'i> {
            byte: u8,
            additional_data: &'i [u8],
        }
        impl SerializeShrinkWrap for Evolved<'_> {
            const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

            fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                wr.write_u8(self.byte)?;
                wr.write_raw_slice(self.additional_data)
            }
        }
        impl<'i> DeserializeShrinkWrap<'i> for Evolved<'i> {
            const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

            fn des_shrink_wrap<'di>(_rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
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
        };
        wr.write(&arr).unwrap();
        assert_eq!(
            wr.finish(),
            Ok(&[0xAB, 0x12, 0x34, 0x56, 0xCD, 0x78, 0x02, 0x42][..])
        );
    }

    #[test]
    fn string_sanity() {
        // &str and String are Unsized
        const _: () = assert!(
            matches!(
                <Vec<&str> as SerializeShrinkWrap>::ELEMENT_SIZE,
                ElementSize::UnsizedFinalStructure
            ),
            "Vec<Unsized> must be UnsizedFinalStructure"
        );
        const _: () = assert!(
            matches!(
                <Vec<String> as SerializeShrinkWrap>::ELEMENT_SIZE,
                ElementSize::UnsizedFinalStructure
            ),
            "Vec<Unsized> must be UnsizedFinalStructure"
        );
    }

    #[test]
    fn vec_vec_string() {
        let arr = vec![vec!["a", "bc"], vec!["def", "ghij"], vec!["klmno"]];
        let mut buf = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&arr).unwrap();
        let buf = wr.finish_and_take().unwrap();
        // println!("{:02X?}", buf);
        assert_eq!(
            buf,
            hex!(
                "616263"
                "6465666768696A"
                "6B6C6D6E6F"
                "0 5 1 4 3 2 2 1 2 3"
            )
        );

        let mut rd = BufReader::new(buf);
        let arr_des: Vec<Vec<String>> = rd.read().unwrap();
        assert_eq!(arr, arr_des);

        // deserialize without alloc
        let mut rd = BufReader::new(buf);
        let arr_des: RefVec<'_, RefVec<'_, &str>> = rd.read().unwrap();
        let mut iter = arr_des.iter();
        let mut iter0 = iter.next().unwrap().unwrap().iter();
        assert_eq!(iter0.next(), Some(Ok("a")));
        assert_eq!(iter0.next(), Some(Ok("bc")));
        assert_eq!(iter0.next(), None);
        let mut iter1 = iter.next().unwrap().unwrap().iter();
        assert_eq!(iter1.next(), Some(Ok("def")));
        assert_eq!(iter1.next(), Some(Ok("ghij")));
        assert_eq!(iter1.next(), None);
        let mut iter2 = iter.next().unwrap().unwrap().iter();
        assert_eq!(iter2.next(), Some(Ok("klmno")));
        assert_eq!(iter2.next(), None);

        // serialize without alloc
        let arr_ref = RefVec::Slice {
            slice: &[
                RefVec::Slice {
                    slice: &["a", "bc"],
                },
                RefVec::Slice {
                    slice: &["def", "ghij"],
                },
                RefVec::Slice { slice: &["klmno"] },
            ],
        };
        let mut buf2 = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf2);
        wr.write(&arr_ref).unwrap();
        let buf_ref = wr.finish_and_take().unwrap();
        assert_eq!(buf, buf_ref);
    }

    #[test]
    fn vec_u7() {
        let arr = vec![
            Some(U7::new(1).unwrap()),
            Some(U7::new(2).unwrap()),
            Some(U7::new(3).unwrap()),
            None,
        ];
        let mut buf = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&arr).unwrap();
        let buf = wr.finish_and_take().unwrap();
        println!("{:02X?}", buf);
        assert_eq!(buf, hex!("81 82 83 04"));
    }
}
