use crate::serdes::traits::SerializeVlu4;
use crate::serdes::vlu4::vlu32::Vlu32;
use crate::serdes::vlu4::{Vlu4Vec, Vlu4VecIter};
use crate::serdes::xpi_vlu4::error::XpiVlu4Error;
use crate::serdes::xpi_vlu4::{Uri, UriIter, UriMask, UriMaskIter};
use crate::serdes::DeserializeVlu4;
use crate::serdes::{NibbleBuf, NibbleBufMut};
use core::fmt::{Display, Formatter};

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
pub struct MultiUri<'i> {
    rdr: NibbleBuf<'i>,
    parts_count: usize,
}

impl<'i> MultiUri<'i> {
    /// Returns the amount of (Uri, UriMask) pairs
    pub fn len(&self) -> usize {
        self.parts_count
    }

    pub fn iter(&self) -> MultiUriIter {
        MultiUriIter {
            rdr: self.rdr.clone(),
            len: self.parts_count,
            pos: 0,
        }
    }

    pub fn flat_iter(&self) -> MultiUriFlatIter {
        let mut rdr_clone = self.rdr.clone();
        let uri_arr: Vlu4Vec<Vlu32> = rdr_clone.des_vlu4().unwrap_or(Vlu4Vec::<Vlu32>::empty());
        let mask: UriMask = rdr_clone
            .des_vlu4()
            .unwrap_or(UriMask::ByIndices(Vlu4Vec::<Vlu32>::empty()));
        MultiUriFlatIter::MultiUri {
            rdr: rdr_clone,
            len: self.parts_count,
            pos: 1,
            uri_iter: uri_arr.iter(),
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
    type Item = (Uri<'i>, UriMask<'i>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.len {
            return None;
        }
        self.pos += 1;

        Some((
            Uri::MultiPart(self.rdr.des_vlu4().ok()?),
            self.rdr.des_vlu4().ok()?,
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

pub enum MultiUriFlatIter<'i> {
    OneUri(Option<UriIter<'i>>),
    MultiUri {
        rdr: NibbleBuf<'i>,
        len: usize,
        pos: usize,
        uri_iter: Vlu4VecIter<'i, Vlu32>,
        mask_iter: UriMaskIter<'i>,
    },
}

impl<'i> Iterator for MultiUriFlatIter<'i> {
    type Item = UriIter<'i>;

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
                    Some(m) => Some(UriIter::ArrIterChain {
                        arr_iter: uri_iter.clone(),
                        last: Some(m),
                    }),
                    None => {
                        if *pos == *len {
                            *pos += 1;
                            return None;
                        }
                        *pos += 1;

                        let uri_arr: Vlu4Vec<Vlu32> = rdr.des_vlu4().ok()?;
                        let mask: UriMask = rdr.des_vlu4().ok()?;
                        *uri_iter = uri_arr.iter();
                        *mask_iter = mask.iter();

                        Some(UriIter::ArrIterChain {
                            arr_iter: uri_iter.clone(),
                            last: mask_iter.next(),
                        })
                    }
                }
            }
        }
    }
}

impl<'i> DeserializeVlu4<'i> for MultiUri<'i> {
    type Error = XpiVlu4Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let parts_count = rdr.get_vlu4_u32()? as usize;
        let mut rdr_before_elements = rdr.clone();
        for _ in 0..parts_count {
            // TODO: implement skip_vlu4() to not read data
            let _uri_len: Uri = rdr.des_vlu4()?;
            let _mask: UriMask = rdr.des_vlu4()?;
        }
        rdr_before_elements.shrink_to_pos_of(rdr)?;
        Ok(MultiUri {
            rdr: rdr_before_elements,
            parts_count,
        })
    }
}

impl<'i> SerializeVlu4 for MultiUri<'i> {
    type Error = XpiVlu4Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        wgr.put_vlu4_u32(self.parts_count as u32)?;
        for (uri, mask) in self.iter() {
            wgr.put(&uri)?;
            wgr.put(&mask)?;
        }
        Ok(())
    }

    fn len_nibbles(&self) -> usize {
        self.rdr.nibbles_left()
    }
}

impl<'i> Display for MultiUri<'i> {
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
    use crate::serdes::vlu4::vlu32::Vlu32;
    use crate::serdes::xpi_vlu4::multi_uri::MultiUri;
    use crate::serdes::xpi_vlu4::UriMask;
    use crate::serdes::NibbleBuf;
    use std::format;

    #[test]
    fn one_pair_mask_u16() {
        let buf = [0x13, 0x12, 0x31, 0xab, 0xcd];
        let mut rdr = NibbleBuf::new_all(&buf);
        let multi_uri: MultiUri = rdr.des_vlu4().unwrap();
        let mut multi_uri_iter = multi_uri.iter();
        let (uri, mask) = multi_uri_iter.next().unwrap();
        assert!(multi_uri_iter.next().is_none());

        let mut uri_iter = uri.iter();
        assert_eq!(uri_iter.next(), Some(1));
        assert_eq!(uri_iter.next(), Some(2));
        assert_eq!(uri_iter.next(), Some(3));
        assert_eq!(uri_iter.next(), None);

        assert!(matches!(mask, UriMask::ByBitfield16(0xabcd)));
    }

    #[test]
    fn one_pair_mask_indices() {
        let buf = [0x12, 0x12, 0x52, 0x35];
        let mut rdr = NibbleBuf::new_all(&buf);
        let multi_uri: MultiUri = rdr.des_vlu4().unwrap();
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
            assert_eq!(indices_iter.next(), Some(Vlu32(3)));
            assert_eq!(indices_iter.next(), Some(Vlu32(5)));
            assert_eq!(indices_iter.next(), None);
        }
    }

    #[test]
    fn two_pairs_mask_all() {
        let buf = [0x22, 0x12, 0x63, 0x25, 0x66, 0x70];
        let mut rdr = NibbleBuf::new_all(&buf);
        let multi_uri: MultiUri = rdr.des_vlu4().unwrap();
        let mut multi_uri_iter = multi_uri.iter();

        let (uri0, mask0) = multi_uri_iter.next().unwrap();
        let (uri1, mask1) = multi_uri_iter.next().unwrap();
        assert!(multi_uri_iter.next().is_none());

        let mut uri_iter = uri0.iter();
        assert_eq!(uri_iter.next(), Some(1));
        assert_eq!(uri_iter.next(), Some(2));
        assert_eq!(uri_iter.next(), None);

        assert!(matches!(mask0, UriMask::All(Vlu32(3))));

        let mut uri_iter = uri1.iter();
        assert_eq!(uri_iter.next(), Some(5));
        assert_eq!(uri_iter.next(), Some(6));
        assert_eq!(uri_iter.next(), None);

        assert!(matches!(mask1, UriMask::All(Vlu32(7))));
    }

    #[test]
    fn flat_iter() {
        let buf = [0x22, 0x12, 0x63, 0x25, 0x66, 0x20];
        let mut rdr = NibbleBuf::new_all(&buf);
        let multi_uri: MultiUri = rdr.des_vlu4().unwrap();
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
        let multi_uri: MultiUri = NibbleBuf::new_all(&[0x10, 0x52, 0x34]).des_vlu4().unwrap();
        let mut iter = multi_uri.flat_iter();
        assert_eq!(format!("{}", iter.next().unwrap()), "/3");
        assert_eq!(format!("{}", iter.next().unwrap()), "/4");
        assert!(iter.next().is_none());
    }

    #[test]
    fn two_at_root_separate() {
        let multi_uri: MultiUri = NibbleBuf::new_all(&[0x20, 0x51, 0x50, 0x51, 0x50])
            .des_vlu4()
            .unwrap();
        let mut iter = multi_uri.flat_iter();
        assert_eq!(format!("{}", iter.next().unwrap()), "/5");
        assert_eq!(format!("{}", iter.next().unwrap()), "/5");
        assert!(iter.next().is_none());
    }
}
