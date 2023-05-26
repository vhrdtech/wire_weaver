use super::{SerialUri, SerialUriIter, UriMask, UriMaskIter};
use crate::error::XpiError;
use core::fmt::{Display, Formatter};
use vhl_stdlib::serdes::nibble_buf;
use vhl_stdlib::serdes::{
    vlu4::{Vlu4Vec, Vlu4VecIter},
    DeserializeVlu4, NibbleBuf, NibbleBufMut, SerDesSize, SerializeVlu4,
};

/// Allows to select any combination of resources in order to perform read/write or stream
/// operations on them all at once. Operations are performed sequentially in order of the resources
/// serial numbers, depth first. Responses to read requests or stream published values are arranged
/// in arbitrary order, that is deemed optimal at a time, all with proper uris attached, so it's possible
/// to distinguish them. In response to one request, one or many responses may arrive.
/// Maximum packets sizes, publishing and observing rates, maximum jitter is taken into account when
/// grouping responses together.
///
/// Examples:
/// (/a, bitfield: 0b110), (/b, bitfield: 0b011) selects /a/2, /a/3, /b/x, /b/y
/// (/b, bitfield: 0b100) select /b/z/u and /b/z/v
/// (/b/z, indexes: 1) selects /b/z/v
#[derive(Copy, Clone, Debug)]
pub struct SerialMultiUri<'i> {
    rdr: NibbleBuf<'i>,
    parts_count: usize,
}

impl<'i> SerialMultiUri<'i> {
    /// Returns the amount of (Uri, UriMask) pairs
    pub fn len(&self) -> usize {
        self.parts_count
    }

    pub fn is_empty(&self) -> bool {
        self.parts_count == 0
    }

    pub fn iter(&self) -> MultiUriIter {
        MultiUriIter {
            rdr: self.rdr,
            len: self.parts_count,
            pos: 0,
        }
    }

    pub fn flat_iter(&self) -> MultiUriFlatIter {
        let mut rdr_clone = self.rdr;
        let uri_iter: Vlu4VecIter<u32> = rdr_clone
            .des_vlu4()
            .unwrap_or_else(|_| Vlu4Vec::<u32>::empty())
            .into_iter();
        let mask: UriMask<Vlu4Vec<u32>> = rdr_clone
            .des_vlu4()
            .unwrap_or_else(|_| UriMask::ByIndices(Vlu4Vec::<u32>::empty()));
        MultiUriFlatIter::MultiUri {
            rdr: rdr_clone,
            len: self.parts_count,
            pos: 1,
            uri_iter,
            mask_iter: mask.iter(),
        }
    }
}

pub struct MultiUriIter<'i> {
    rdr: NibbleBuf<'i>,
    len: usize,
    pos: usize,
}

impl<'i> Iterator for MultiUriIter<'i> {
    type Item = (SerialUri<Vlu4Vec<'i, u32>>, UriMask<Vlu4Vec<'i, u32>>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.len {
            return None;
        }
        self.pos += 1;

        let arr: Vlu4Vec<u32> = self.rdr.des_vlu4().ok()?;
        Some((SerialUri::MultiPart(arr), self.rdr.des_vlu4().ok()?))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

pub enum MultiUriFlatIter<'i> {
    OneUri(Option<SerialUriIter<Vlu4VecIter<'i, u32>>>),
    MultiUri {
        rdr: NibbleBuf<'i>,
        len: usize,
        pos: usize,
        uri_iter: Vlu4VecIter<'i, u32>,
        mask_iter: UriMaskIter<Vlu4VecIter<'i, u32>>,
    },
}

impl<'i> Iterator for MultiUriFlatIter<'i> {
    // must yield iterators, otherwise allocator would be needed
    type Item = SerialUriIter<Vlu4VecIter<'i, u32>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            MultiUriFlatIter::OneUri(iter) => iter.take(),
            MultiUriFlatIter::MultiUri {
                rdr,
                len,
                pos,
                uri_iter,
                mask_iter,
            } => {
                if *pos > *len {
                    return None;
                }
                match mask_iter.next() {
                    Some(m) => Some(SerialUriIter::ArrIterChain {
                        arr_iter: uri_iter.clone(),
                        last: Some(m),
                    }),
                    None => {
                        if *pos == *len {
                            *pos += 1;
                            return None;
                        }
                        *pos += 1;

                        let uri_arr: Vlu4Vec<u32> = rdr.des_vlu4().ok()?;
                        let mask: UriMask<Vlu4Vec<u32>> = rdr.des_vlu4().ok()?;
                        *uri_iter = uri_arr.into_iter();
                        *mask_iter = mask.iter();

                        Some(SerialUriIter::ArrIterChain {
                            arr_iter: uri_iter.clone(),
                            last: mask_iter.next(),
                        })
                    }
                }
            }
        }
    }
}

impl<'i> DeserializeVlu4<'i> for SerialMultiUri<'i> {
    type Error = XpiError;

    fn des_vlu4<'di>(nrd: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let parts_count = nrd.get_vlu32n()? as usize;
        let mut rdr_before_elements = *nrd;
        for _ in 0..parts_count {
            // TODO: implement skip_vlu4() to not read data
            let _uri_arr_len: Vlu4Vec<u32> = nrd.des_vlu4()?;
            // let _uri_len: SerialUri<Vlu4VecIter<u32>> = rdr.des_vlu4()?;
            let _mask: UriMask<Vlu4Vec<u32>> = nrd.des_vlu4()?;
        }
        rdr_before_elements.shrink_to_pos_of(nrd)?;
        Ok(SerialMultiUri {
            rdr: rdr_before_elements,
            parts_count,
        })
    }
}

impl<'i> SerializeVlu4 for SerialMultiUri<'i> {
    type Error = nibble_buf::Error;

    fn ser_vlu4(&self, nwr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        nwr.put_vlu32n(self.parts_count as u32)?;
        for (uri, mask) in self.iter() {
            nwr.put(&uri)?;
            nwr.put(&mask)?;
        }
        Ok(())
    }

    fn len_nibbles(&self) -> SerDesSize {
        SerDesSize::Sized(self.rdr.nibbles_left())
    }
}

impl<'i> Display for SerialMultiUri<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "MultiUri(")?;
        let iter = self.iter();
        let len = iter.size_hint().0;
        for (i, (uri, mask)) in iter.enumerate() {
            if uri.iter().size_hint().0 != 0 {
                write!(f, "{}", uri)?;
            }
            write!(f, "/{:#}", mask)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        write!(f, ")")
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use crate::xwfd::multi_uri::SerialMultiUri;
    use crate::xwfd::UriMask;
    use std::format;
    use vhl_stdlib::serdes::vlu4::vlu32n::Vlu32N;
    use vhl_stdlib::serdes::NibbleBuf;

    #[test]
    fn one_pair_mask_u16() {
        let buf = [0x13, 0x12, 0x31, 0x80, 0x88];
        let mut rdr = NibbleBuf::new_all(&buf);
        let multi_uri: SerialMultiUri = rdr.des_vlu4().unwrap();
        let mut multi_uri_iter = multi_uri.iter();
        let (uri, mask) = multi_uri_iter.next().unwrap();
        assert!(multi_uri_iter.next().is_none());

        let mut uri_iter = uri.iter();
        assert_eq!(uri_iter.next(), Some(1));
        assert_eq!(uri_iter.next(), Some(2));
        assert_eq!(uri_iter.next(), Some(3));
        assert_eq!(uri_iter.next(), None);

        assert!(matches!(mask, UriMask::ByBitfield16(0x8088)));
    }

    #[test]
    fn one_pair_mask_indices() {
        let buf = [0x12, 0x12, 0x52, 0x35];
        let mut rdr = NibbleBuf::new_all(&buf);
        let multi_uri: SerialMultiUri = rdr.des_vlu4().unwrap();
        let mut multi_uri_iter = multi_uri.iter();
        let (uri, mask) = multi_uri_iter.next().unwrap();
        assert!(multi_uri_iter.next().is_none());

        let mut uri_iter = uri.iter();
        assert_eq!(uri_iter.next(), Some(1));
        assert_eq!(uri_iter.next(), Some(2));
        assert_eq!(uri_iter.next(), None);

        assert!(matches!(mask, UriMask::ByIndices(_)));
        if let UriMask::ByIndices(indices) = mask {
            let mut indices_iter = indices.iter();
            assert_eq!(indices_iter.next(), Some(3));
            assert_eq!(indices_iter.next(), Some(5));
            assert_eq!(indices_iter.next(), None);
        }
    }

    #[test]
    fn two_pairs_mask_all() {
        let buf = [0x22, 0x12, 0x63, 0x25, 0x66, 0x70];
        let mut rdr = NibbleBuf::new_all(&buf);
        let multi_uri: SerialMultiUri = rdr.des_vlu4().unwrap();
        let mut multi_uri_iter = multi_uri.iter();

        let (uri0, mask0) = multi_uri_iter.next().unwrap();
        let (uri1, mask1) = multi_uri_iter.next().unwrap();
        assert!(multi_uri_iter.next().is_none());

        let mut uri_iter = uri0.iter();
        assert_eq!(uri_iter.next(), Some(1));
        assert_eq!(uri_iter.next(), Some(2));
        assert_eq!(uri_iter.next(), None);

        assert!(matches!(mask0, UriMask::All(Vlu32N(3))));

        let mut uri_iter = uri1.iter();
        assert_eq!(uri_iter.next(), Some(5));
        assert_eq!(uri_iter.next(), Some(6));
        assert_eq!(uri_iter.next(), None);

        assert!(matches!(mask1, UriMask::All(Vlu32N(7))));
    }

    #[test]
    fn flat_iter() {
        let buf = [0x22, 0x12, 0x63, 0x25, 0x66, 0x20];
        let mut rdr = NibbleBuf::new_all(&buf);
        let multi_uri: SerialMultiUri = rdr.des_vlu4().unwrap();
        let mut flat_iter = multi_uri.flat_iter();
        assert_eq!(format!("{}", flat_iter.next().unwrap()), "/1/2/0");
        assert_eq!(format!("{}", flat_iter.next().unwrap()), "/1/2/1");
        assert_eq!(format!("{}", flat_iter.next().unwrap()), "/1/2/2");
        assert_eq!(format!("{}", flat_iter.next().unwrap()), "/5/6/0");
        assert_eq!(format!("{}", flat_iter.next().unwrap()), "/5/6/1");
        assert!(flat_iter.next().is_none());
    }

    #[test]
    fn at_root_level() {
        let multi_uri: SerialMultiUri = NibbleBuf::new_all(&[0x10, 0x52, 0x34]).des_vlu4().unwrap();
        let mut iter = multi_uri.flat_iter();
        assert_eq!(format!("{}", iter.next().unwrap()), "/3");
        assert_eq!(format!("{}", iter.next().unwrap()), "/4");
        assert!(iter.next().is_none());
    }

    #[test]
    fn two_at_root_separate() {
        let multi_uri: SerialMultiUri = NibbleBuf::new_all(&[0x20, 0x51, 0x50, 0x51, 0x50])
            .des_vlu4()
            .unwrap();
        let mut iter = multi_uri.flat_iter();
        assert_eq!(format!("{}", iter.next().unwrap()), "/5");
        assert_eq!(format!("{}", iter.next().unwrap()), "/5");
        assert!(iter.next().is_none());
    }
}
