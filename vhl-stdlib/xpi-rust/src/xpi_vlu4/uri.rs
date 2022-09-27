use vhl_stdlib_nostd::discrete::{U3, U4, U6};
use vhl_stdlib_nostd::serdes::traits::SerializeVlu4;
use vhl_stdlib_nostd::serdes::vlu4::vlu32::Vlu32;
use vhl_stdlib_nostd::serdes::vlu4::{Vlu4Vec, Vlu4VecIter};
use crate::xpi_vlu4::error::XpiVlu4Error;
use vhl_stdlib_nostd::serdes::{DeserializeVlu4, NibbleBuf, NibbleBufMut, SerDesSize};
use core::fmt::{Debug, Display, Formatter};
use core::iter::FusedIterator;

/// Sequence of numbers uniquely identifying one of the resources.
/// If there is a group in the uri with not numerical index - it must be mapped into numbers.
#[derive(Copy, Clone)]
pub enum SerialUri<'i> {
    /// Points to one of the root resources /i, i <= 15; takes 1 nibble
    OnePart4(U4),

    /// Points to one of the root child resources /i/j, i,j <= 15; takes 2 nibbles
    TwoPart44(U4, U4),

    /// Points into third level of the resource tree /i/j/k, i,j,k <= 15; takes 3 nibbles
    ThreePart444(U4, U4, U4),

    /// Points into third level of the resource tree /i/j/k, i <= 63, j,k <= 7; takes 3 nibbles
    ThreePart633(U6, U3, U3),

    /// Points into third level of the resource tree /i/j/k, i,j <= 63, k <= 15; takes 4 nibbles
    ThreePart664(U6, U6, U4),

    /// Point to any resource in the resources tree, any numbers up to u32::MAX; variable size
    MultiPart(Vlu4Vec<'i, Vlu32>),
}

impl<'i> SerialUri<'i> {
    pub fn iter(&self) -> UriIter<'i> {
        match self {
            SerialUri::OnePart4(a) => UriIter::UpToThree {
                parts: [a.inner(), 0, 0],
                len: 1,
                pos: 0,
            },
            SerialUri::TwoPart44(a, b) => UriIter::UpToThree {
                parts: [a.inner(), b.inner(), 0],
                len: 2,
                pos: 0,
            },
            SerialUri::ThreePart444(a, b, c) => UriIter::UpToThree {
                parts: [a.inner(), b.inner(), c.inner()],
                len: 3,
                pos: 0,
            },
            SerialUri::ThreePart633(a, b, c) => UriIter::UpToThree {
                parts: [a.inner(), b.inner(), c.inner()],
                len: 3,
                pos: 0,
            },
            SerialUri::ThreePart664(a, b, c) => UriIter::UpToThree {
                parts: [a.inner(), b.inner(), c.inner()],
                len: 3,
                pos: 0,
            },
            SerialUri::MultiPart(arr) => UriIter::ArrIter(arr.iter()),
        }
    }
}

impl<'i> DeserializeVlu4<'i> for SerialUri<'i> {
    type Error = XpiVlu4Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        Ok(SerialUri::MultiPart(rdr.des_vlu4()?))
    }
}

impl<'i> SerializeVlu4 for SerialUri<'i> {
    type Error = XpiVlu4Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        match self {
            SerialUri::OnePart4(a) => {
                wgr.put_nibble(a.inner())?;
            }
            SerialUri::TwoPart44(a, b) => {
                wgr.put_nibble(a.inner())?;
                wgr.put_nibble(b.inner())?;
            }
            SerialUri::ThreePart444(a, b, c) => {
                wgr.put_nibble(a.inner())?;
                wgr.put_nibble(b.inner())?;
                wgr.put_nibble(c.inner())?;
            }
            SerialUri::ThreePart633(a, b, c) => {
                wgr.as_bit_buf::<_, XpiVlu4Error>(|wgr| {
                    wgr.put_up_to_8(6, a.inner())?;
                    wgr.put_up_to_8(3, b.inner())?;
                    wgr.put_up_to_8(3, c.inner())?;
                    Ok(())
                })?;
            }
            SerialUri::ThreePart664(a, b, c) => {
                wgr.as_bit_buf::<_, XpiVlu4Error>(|wgr| {
                    wgr.put_up_to_8(6, a.inner())?;
                    wgr.put_up_to_8(6, b.inner())?;
                    wgr.put_up_to_8(4, c.inner())?;
                    Ok(())
                })?;
            }
            SerialUri::MultiPart(arr) => {
                wgr.put(arr)?;
            }
        }
        Ok(())
    }

    fn len_nibbles(&self) -> SerDesSize {
        let nibbles = match self {
            SerialUri::OnePart4(_) => 1,
            SerialUri::TwoPart44(_, _) => 2,
            SerialUri::ThreePart444(_, _, _) => 3,
            SerialUri::ThreePart633(_, _, _) => 3,
            SerialUri::ThreePart664(_, _, _) => 4,
            SerialUri::MultiPart(arr) => {
                return arr.len_nibbles();
            },
        };
        SerDesSize::Sized(nibbles)
    }
}

impl<'i> IntoIterator for SerialUri<'i> {
    type Item = u32;
    type IntoIter = UriIter<'i>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Clone)]
pub enum UriIter<'i> {
    UpToThree {
        parts: [u8; 3],
        len: u8,
        pos: u8,
    },
    ArrIter(Vlu4VecIter<'i, Vlu32>),
    ArrIterChain {
        arr_iter: Vlu4VecIter<'i, Vlu32>,
        last: Option<u32>,
    },
}

impl<'i> Iterator for UriIter<'i> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            UriIter::UpToThree { parts, len, pos } => {
                if pos < len {
                    *pos += 1;
                    Some(parts[(*pos - 1) as usize] as u32)
                } else {
                    None
                }
            }
            UriIter::ArrIter(arr_iter) => arr_iter.next().map(|x| x.0),
            UriIter::ArrIterChain { arr_iter, last } => match arr_iter.next() {
                Some(p) => Some(p.0),
                None => match *last {
                    Some(p) => {
                        *last = None;
                        Some(p)
                    }
                    None => None,
                },
            },
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            UriIter::UpToThree { len, .. } => (*len as usize, Some(*len as usize)),
            UriIter::ArrIter(arr_iter) => arr_iter.size_hint(),
            UriIter::ArrIterChain { arr_iter, .. } => {
                let size = arr_iter.size_hint().0;
                (size + 1, Some(size + 1))
            }
        }
    }
}

impl<'i> FusedIterator for UriIter<'i> {}

impl<'i> Display for UriIter<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let mut uri_iter = self.clone().peekable();
        if f.alternate() {
            write!(f, "Uri(/")?;
        } else {
            write!(f, "/")?;
        }
        while let Some(uri_part) = uri_iter.next() {
            write!(f, "{}", uri_part)?;
            if uri_iter.peek().is_some() {
                write!(f, "/")?;
            }
        }
        if f.alternate() {
            write!(f, ")")
        } else {
            write!(f, "")
        }
    }
}

impl<'i> Display for SerialUri<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        if f.alternate() {
            write!(f, "{:#}", self.iter())
        } else {
            write!(f, "{}", self.iter())
        }
    }
}

impl<'i> Debug for SerialUri<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self)
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use vhl_stdlib_nostd::discrete::{U3, U4, U6};
    use std::format;

    use super::SerialUri;
    use vhl_stdlib_nostd::serdes::vlu4::vlu32::Vlu32;
    use vhl_stdlib_nostd::serdes::vlu4::Vlu4Vec;
    use vhl_stdlib_nostd::serdes::NibbleBuf;

    #[test]
    fn one_part_uri_iter() {
        let uri = SerialUri::OnePart4(U4::new(1).unwrap());
        let mut uri_iter = uri.iter();
        assert_eq!(uri_iter.next(), Some(1));
        assert_eq!(uri_iter.next(), None);
    }

    #[test]
    fn two_part_uri_iter() {
        let uri = SerialUri::TwoPart44(U4::new(1).unwrap(), U4::new(2).unwrap());
        let mut uri_iter = uri.iter();
        assert_eq!(uri_iter.next(), Some(1));
        assert_eq!(uri_iter.next(), Some(2));
        assert_eq!(uri_iter.next(), None);
    }

    #[test]
    fn three_part_uri_iter() {
        let uri = SerialUri::ThreePart633(
            U6::new(35).unwrap(),
            U3::new(4).unwrap(),
            U3::new(3).unwrap(),
        );
        let mut uri_iter = uri.iter();
        assert_eq!(uri_iter.next(), Some(35));
        assert_eq!(uri_iter.next(), Some(4));
        assert_eq!(uri_iter.next(), Some(3));
        assert_eq!(uri_iter.next(), None);
    }

    #[test]
    fn multi_part_uri_iter() {
        let buf = [0x51, 0x23, 0x45];
        let mut buf = NibbleBuf::new_all(&buf);
        let arr: Vlu4Vec<Vlu32> = buf.des_vlu4().unwrap();
        let uri = SerialUri::MultiPart(arr);
        let mut uri_iter = uri.iter();
        assert_eq!(uri_iter.next(), Some(1));
        assert_eq!(uri_iter.next(), Some(2));
        assert_eq!(uri_iter.next(), Some(3));
        assert_eq!(uri_iter.next(), Some(4));
        assert_eq!(uri_iter.next(), Some(5));
        assert_eq!(uri_iter.next(), None);
    }

    #[test]
    fn uri_display() {
        let buf = [0x51, 0x23, 0x45];
        let mut buf = NibbleBuf::new_all(&buf);
        let arr: Vlu4Vec<Vlu32> = buf.des_vlu4().unwrap();
        let uri = SerialUri::MultiPart(arr);
        assert_eq!(format!("{:#}", uri), "Uri(/1/2/3/4/5)");
        assert_eq!(format!("{}", uri), "/1/2/3/4/5");
    }
}
