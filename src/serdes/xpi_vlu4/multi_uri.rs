use core::fmt::{Display, Formatter};
use crate::serdes::NibbleBuf;
use crate::serdes::DeserializeVlu4;
use crate::serdes::xpi_vlu4::{Uri, UriMask};
use crate::serdes::xpi_vlu4::error::XpiVlu4Error;

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
    len: usize,
}

impl<'i> MultiUri<'i> {
    /// Returns the amount of (Uri, UriMask) pairs
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn iter(&self) -> MultiUriIter {
        MultiUriIter {
            rdr: self.rdr.clone(),
            len: self.len,
            pos: 0,
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

        Some((Uri::MultiPart(self.rdr.des_vlu4().ok()?), self.rdr.des_vlu4().ok()?))
    }
}

impl<'i> DeserializeVlu4<'i> for MultiUri<'i> {
    type Error = XpiVlu4Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let len = rdr.get_vlu4_u32()? as usize;
        let rdr_before_elements = rdr.clone();
        for _ in 0..len {
            let _uri_len: Uri = rdr.des_vlu4()?;
            let _mask: UriMask = rdr.des_vlu4()?;
        }
        Ok(MultiUri {
            rdr: rdr_before_elements,
            len,
        })
    }
}

impl<'i> Display for MultiUri<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "MultiUri(")?;
        for (uri, mask) in self.iter() {
            write!(f, "{} {}", uri, mask)?;
        }
        write!(f, ")")
    }
}

#[cfg(test)]
mod test {
    use crate::serdes::NibbleBuf;
    use crate::serdes::xpi_vlu4::multi_uri::MultiUri;
    use crate::serdes::xpi_vlu4::UriMask;

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
            assert_eq!(indices_iter.next(), Some(3));
            assert_eq!(indices_iter.next(), Some(5));
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

        assert!(matches!(mask0, UriMask::All(3)));

        let mut uri_iter = uri1.iter();
        assert_eq!(uri_iter.next(), Some(5));
        assert_eq!(uri_iter.next(), Some(6));
        assert_eq!(uri_iter.next(), None);

        assert!(matches!(mask1, UriMask::All(7)));
    }
}