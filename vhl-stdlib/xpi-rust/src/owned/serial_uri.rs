use vhl_stdlib_nostd::serdes::BitBufMut;
use crate::owned::convert_error::ConvertError;
use crate::xwfd;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialUri {
    pub segments: Vec<u32>,
}

impl SerialUri {
    /// Determine most optimal xwfd::SerialUri and serialize it's kind into provided buffer.
    /// Remember this kind to not repeat the process later, when serializing actual data.
    pub(crate) fn ser_header_xwfd(
        &self,
        bwr: &mut BitBufMut,
    ) -> Result<xwfd::SerialUriDiscriminant, ConvertError> {
        let mut iter = self.segments.iter();
        let s03 = (iter.next(), iter.next(), iter.next(), iter.next());
        use xwfd::SerialUriDiscriminant::*;
        let uri_kind = match s03 {
            (Some(s0), None, None, None) => {
                if *s0 <= 15 {
                    OnePart4
                } else {
                    MultiPart
                }
            }
            (Some(s0), Some(s1), None, None) => {
                if *s0 <= 15 && *s1 <= 15 {
                    TwoPart44
                } else {
                    MultiPart
                }
            }
            (Some(s0), Some(s1), Some(s2), None) => {
                if *s0 <= 63 {
                    if *s1 <= 63 && *s2 <= 15 {
                        ThreePart664
                    } else if *s1 <= 7 && *s2 <= 7 {
                        ThreePart633
                    } else if *s0 <= 15 && *s1 <= 15 && *s2 <= 15 {
                        ThreePart444
                    } else {
                        MultiPart
                    }
                } else {
                    MultiPart
                }
            }
            (_, _, _, _) => MultiPart,
        };
        bwr.put_up_to_8(3, uri_kind as u8)?;
        Ok(uri_kind)
    }
}

// #[derive(Clone, Debug, Eq, PartialEq)]
// pub enum SerialUriSegment {
//     Serial { serial: u32 },
//     SerialIndex { serial: u32, by: u32 },
// }

