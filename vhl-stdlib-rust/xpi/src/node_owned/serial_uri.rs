use crate::xwfd;
use smallvec::SmallVec;
use std::fmt::{Display, Formatter};
use std::slice::Iter;
use vhl_stdlib::discrete::{U3, U4, U6};
use vhl_stdlib::serdes::vlu4::Vlu4Vec;

pub const URI_STACK_SEGMENTS: usize = 6;

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct UriOwned {
    pub(crate) segments: SmallVec<[u32; URI_STACK_SEGMENTS]>,
}

impl UriOwned {
    pub fn empty() -> Self {
        UriOwned {
            segments: SmallVec::new(),
        }
    }

    pub fn new(segments: &[u32]) -> Self {
        UriOwned {
            segments: segments.iter().copied().collect(),
        }
    }

    pub fn push(&mut self, segment: u32) {
        self.segments.push(segment);
    }

    pub fn parse<S: AsRef<str>>(_uri: S) -> Self {
        // TODO: Proper serial uri parser
        todo!()
    }

    pub fn iter(&self) -> Iter<u32> {
        self.segments.iter()
    }

    /// Determine most optimal xwfd::SerialUri and serialize it's kind into provided buffer.
    /// Remember this kind (by constructing xwfd::SerialUri) to not repeat the process later,
    /// when serializing actual data.
    pub(crate) fn ser_header_xwfd(
        &self,
    ) -> xwfd::SerialUri<smallvec::IntoIter<[u32; URI_STACK_SEGMENTS]>> {
        let mut iter = self.segments.iter();
        let s03 = (iter.next(), iter.next(), iter.next(), iter.next());
        use xwfd::SerialUri::*;
        match s03 {
            (Some(&s0), None, None, None) => {
                if s0 <= 15 {
                    OnePart4(unsafe { U4::new_unchecked(s0 as u8) })
                } else {
                    MultiPart(self.segments.clone().into_iter())
                }
            }
            (Some(&s0), Some(&s1), None, None) => {
                if s0 <= 15 && s1 <= 15 {
                    TwoPart44(unsafe { U4::new_unchecked(s0 as u8) }, unsafe {
                        U4::new_unchecked(s1 as u8)
                    })
                } else {
                    MultiPart(self.segments.clone().into_iter())
                }
            }
            (Some(&s0), Some(&s1), Some(&s2), None) => {
                if s0 <= 63 && s1 <= 63 && s2 <= 15 {
                    if s0 <= 15 && s1 <= 15 && s2 <= 15 {
                        ThreePart444(
                            unsafe { U4::new_unchecked(s0 as u8) },
                            unsafe { U4::new_unchecked(s1 as u8) },
                            unsafe { U4::new_unchecked(s2 as u8) },
                        )
                    } else if s1 <= 7 && s2 <= 7 {
                        ThreePart633(
                            unsafe { U6::new_unchecked(s0 as u8) },
                            unsafe { U3::new_unchecked(s1 as u8) },
                            unsafe { U3::new_unchecked(s2 as u8) },
                        )
                    } else {
                        ThreePart664(
                            unsafe { U6::new_unchecked(s0 as u8) },
                            unsafe { U6::new_unchecked(s1 as u8) },
                            unsafe { U4::new_unchecked(s2 as u8) },
                        )
                    }
                } else {
                    MultiPart(self.segments.clone().into_iter())
                }
            }
            (_, _, _, _) => MultiPart(self.segments.clone().into_iter()),
        }
    }
}

impl<'i> From<xwfd::SerialUri<Vlu4Vec<'i, u32>>> for UriOwned {
    fn from(uri: xwfd::SerialUri<Vlu4Vec<'i, u32>>) -> Self {
        UriOwned {
            segments: uri.iter().collect(),
        }
    }
}

// #[derive(Clone, Debug, Eq, PartialEq)]
// pub enum SerialUriSegment {
//     Serial { serial: u32 },
//     SerialIndex { serial: u32, by: u32 },
// }

impl Display for UriOwned {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        if f.alternate() {
            write!(f, "Uri(/")?;
        } else {
            write!(f, "/")?;
        }
        let mut uri_iter = self.segments.iter().peekable();
        while let Some(uri_part) = uri_iter.next() {
            write!(f, "{}", uri_part)?;
            if uri_iter.peek().is_some() {
                write!(f, "/")?;
            }
        }
        if f.alternate() {
            write!(f, ")")?;
        }
        Ok(())
    }
}
//
// #[derive(Clone)]
// pub struct SerialUriIter<'a> {
//     segments: Iter<'a, u32>,
//     pos: usize,
// }
//
// impl Iterator for SerialUriIter {
//     type Item = u32;
//
//     fn next(&mut self) -> Option<Self::Item> {
//         if self.pos < self.segments.len() {
//             let segment = self.segments[self.pos];
//             self.pos += 1;
//             Some(segment)
//         } else {
//             None
//         }
//     }
// }
