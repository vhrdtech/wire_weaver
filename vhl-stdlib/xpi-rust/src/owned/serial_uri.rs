use std::vec::IntoIter;
use vhl_stdlib::discrete::{U3, U4, U6};
use crate::owned::convert_error::ConvertError;
use crate::xwfd;
use vhl_stdlib::serdes::BitBufMut;
use vhl_stdlib::serdes::vlu4::Vlu32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialUri {
    pub segments: Vec<Vlu32>,
}

impl SerialUri {
    pub fn new<S: AsRef<str>>(_uri: S) -> Self {
        SerialUri {
            segments: vec![Vlu32(5)]
        }
    }

    /// Determine most optimal xwfd::SerialUri and serialize it's kind into provided buffer.
    /// Remember this kind to not repeat the process later, when serializing actual data.
    pub(crate) fn ser_header_xwfd(
        &self,
        bwr: &mut BitBufMut,
    ) -> Result<xwfd::SerialUri<IntoIter<Vlu32>>, ConvertError> {
        let mut iter = self.segments.iter();
        let s03 = (iter.next(), iter.next(), iter.next(), iter.next());
        use xwfd::SerialUri::*;
        let uri = match s03 {
            (Some(s0), None, None, None) => {
                if s0.0 <= 15 {
                    OnePart4(unsafe { U4::new_unchecked(s0.0 as u8) })
                } else {
                    MultiPart(self.segments.clone().into_iter())
                }
            }
            (Some(s0), Some(s1), None, None) => {
                if s0.0 <= 15 && s1.0 <= 15 {
                    TwoPart44(unsafe { U4::new_unchecked(s0.0 as u8) }, unsafe { U4::new_unchecked(s1.0 as u8) })
                } else {
                    MultiPart(self.segments.clone().into_iter())
                }
            }
            (Some(s0), Some(s1), Some(s2), None) => {
                if s0.0 <= 63 {
                    if s1.0 <= 63 && s2.0 <= 15 {
                        ThreePart664(unsafe { U6::new_unchecked(s0.0 as u8) }, unsafe { U6::new_unchecked(s1.0 as u8) }, unsafe { U4::new_unchecked(s2.0 as u8) })
                    } else if s1.0 <= 7 && s2.0 <= 7 {
                        ThreePart633(unsafe { U6::new_unchecked(s0.0 as u8) }, unsafe { U3::new_unchecked(s1.0 as u8) }, unsafe { U3::new_unchecked(s2.0 as u8) })
                    } else if s0.0 <= 15 && s1.0 <= 15 && s2.0 <= 15 {
                        ThreePart444(unsafe { U4::new_unchecked(s0.0 as u8) }, unsafe { U4::new_unchecked(s1.0 as u8) }, unsafe { U4::new_unchecked(s2.0 as u8) })
                    } else {
                        MultiPart(self.segments.clone().into_iter())
                    }
                } else {
                    MultiPart(self.segments.clone().into_iter())
                }
            }
            (_, _, _, _) => MultiPart(self.segments.clone().into_iter()),
        };
        bwr.put_up_to_8(3, uri.discriminant() as u8)?;
        Ok(uri)
    }
    //
    // pub(crate) fn ser_body_xwfd(
    //     &self,
    //     nwr: &mut NibbleBufMut,
    //     uri_kind: xwfd::SerialUriDiscriminant,
    // ) -> Result<(), ConvertError> {
    //
    //     Ok(())
    // }
}

// #[derive(Clone, Debug, Eq, PartialEq)]
// pub enum SerialUriSegment {
//     Serial { serial: u32 },
//     SerialIndex { serial: u32, by: u32 },
// }
