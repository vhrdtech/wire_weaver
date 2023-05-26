use crate::owned::UriOwned;
use crate::xwfd;
use crate::xwfd::{UriMask, UriMaskIter};
use core::fmt::{Display, Formatter};
use core::slice::Iter;
use smallvec::SmallVec;
use vhl_stdlib::serdes::vlu4::Vlu4Vec;

type UriMaskArr = SmallVec<[u32; 4]>;
type UriMaskOwned = UriMask<UriMaskArr>;
type UriMaskIterOwned = UriMaskIter<<UriMaskArr as IntoIterator>::IntoIter>;

#[derive(Clone, Debug)]
pub struct MultiUriOwned {
    pub(crate) pairs: SmallVec<[(UriOwned, UriMaskOwned); 2]>,
}

impl MultiUriOwned {
    pub fn new() -> Self {
        MultiUriOwned {
            pairs: SmallVec::new(),
        }
    }

    pub fn push(&mut self, uri_seed: UriOwned, uri_mask: UriMaskOwned) {
        self.pairs.push((uri_seed, uri_mask));
    }

    pub fn flat_iter(&self) -> MultiUriFlatIter {
        MultiUriFlatIter::MultiUri {
            pairs_iter: self.pairs.iter(),
            current: None,
        }
    }
}

impl Default for MultiUriOwned {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for MultiUriOwned {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "MultiUri()")
    }
}

pub enum MultiUriFlatIter<'a> {
    OneUri(Option<UriOwned>),
    MultiUri {
        pairs_iter: Iter<'a, (UriOwned, UriMaskOwned)>,
        current: Option<(UriOwned, UriMaskIterOwned)>,
    },
}

impl<'a> Iterator for MultiUriFlatIter<'a> {
    type Item = UriOwned;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            MultiUriFlatIter::OneUri(uri) => {
                return uri.take();
            }
            MultiUriFlatIter::MultiUri {
                pairs_iter,
                current,
            } => {
                if let Some((uri_seed, mask_iter)) = current {
                    if let Some(segment) = mask_iter.next() {
                        let mut uri = uri_seed.clone();
                        uri.push(segment);
                        return Some(uri);
                    }
                }
                match pairs_iter.next() {
                    // if mask_iter is exhausted or self.current is None
                    Some((uri_seed, uri_mask)) => {
                        let uri_seed = uri_seed.clone();
                        let mask_iter = uri_mask.iter();
                        // update and call next() recursively (once) down below to avoid duplicated code
                        *current = Some((uri_seed, mask_iter));
                    }
                    None => {
                        // nothing to yield anymore
                        *current = None;
                        return None;
                    }
                }
            }
        }
        let next = self.next();
        match next {
            Some(next) => Some(next),
            None => {
                *self = MultiUriFlatIter::OneUri(None); // nothing to yield anymore, short circuit
                None
            }
        }
    }
}

impl<'i> From<UriMask<Vlu4Vec<'i, u32>>> for UriMaskOwned {
    fn from(mask: UriMask<Vlu4Vec<'i, u32>>) -> Self {
        match mask {
            UriMask::ByBitfield8(bits) => UriMaskOwned::ByBitfield8(bits),
            UriMask::ByBitfield16(bits) => UriMaskOwned::ByBitfield16(bits),
            UriMask::ByBitfield32(bits) => UriMaskOwned::ByBitfield32(bits),
            UriMask::ByIndices(arr) => {
                let mut arr_owned = UriMaskArr::new();
                for segment in arr {
                    arr_owned.push(segment);
                }
                UriMaskOwned::ByIndices(arr_owned)
            }
            UriMask::All(max) => UriMaskOwned::All(max),
        }
    }
}

impl<'i> From<xwfd::SerialMultiUri<'i>> for MultiUriOwned {
    fn from(multi_uri: xwfd::SerialMultiUri<'i>) -> Self {
        MultiUriOwned {
            pairs: multi_uri
                .iter()
                .map(|(uri, mask)| (uri.into(), mask.into()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use super::*;
    use vhl_stdlib::serdes::NibbleBuf;

    #[test]
    fn one_pair_mask_u16() {
        let mut multi_uri = MultiUriOwned::new();
        multi_uri.push(
            UriOwned::new(&[1, 2, 3]),
            UriMask::ByBitfield16(0b1000_0000_1000_1000),
        );
        let mut multi_uri_iter = multi_uri.flat_iter();
        assert_eq!(multi_uri_iter.next(), Some(UriOwned::new(&[1, 2, 3, 0])));
        assert_eq!(multi_uri_iter.next(), Some(UriOwned::new(&[1, 2, 3, 8])));
        assert_eq!(multi_uri_iter.next(), Some(UriOwned::new(&[1, 2, 3, 12])));
        assert_eq!(multi_uri_iter.next(), None);
    }

    #[test]
    fn convert_xwfd_to_owned() {
        let buf = [0x13, 0x12, 0x31, 0x80, 0x88];
        let mut rdr = NibbleBuf::new_all(&buf);
        let multi_uri: xwfd::SerialMultiUri = rdr.des_vlu4().unwrap();
        let multi_uri: MultiUriOwned = multi_uri.into();

        let mut multi_uri_iter = multi_uri.flat_iter();
        assert_eq!(multi_uri_iter.next(), Some(UriOwned::new(&[1, 2, 3, 0])));
        assert_eq!(multi_uri_iter.next(), Some(UriOwned::new(&[1, 2, 3, 8])));
        assert_eq!(multi_uri_iter.next(), Some(UriOwned::new(&[1, 2, 3, 12])));
        assert_eq!(multi_uri_iter.next(), None);
    }
}
