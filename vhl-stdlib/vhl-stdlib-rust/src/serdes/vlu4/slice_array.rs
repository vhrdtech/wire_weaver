use core::fmt::{Debug, Display, Formatter};
use core::iter::FusedIterator;
use crate::serdes::{NibbleBuf, NibbleBufMut};
use crate::serdes::DeserializeVlu4;
use crate::serdes::traits::SerializeVlu4;
use crate::serdes::xpi_vlu4::error::XpiVlu4Error;

/// Variable size array of u8 slices (each aligned to byte boundary).
/// Optimised for ease of writing in place - slice amount is written as 4 bits, with 0b1111 meaning
/// that there are more than 15 slices.
/// 4 bit slice count ~ (vlu4 slice len ~ padding? ~ u8 slice data)+ ~ (self)*
#[derive(Copy, Clone)]
pub struct Vlu4SliceArray<'i> {
    rdr: NibbleBuf<'i>,
    // total number of [u8] slices serialized
    total_len: usize,
}

impl<'i> Vlu4SliceArray<'i> {
    pub fn iter(&self) -> Vlu4SliceArrayIter {
        let mut rdr_clone = self.rdr.clone();
        // NOTE: unwrap_or: should not happen, checked in DeserializeVlu4
        let mut stride_len = rdr_clone.get_nibble().unwrap_or(0) as usize;
        let is_last_stride = if stride_len <= 14 {
            true
        } else {
            stride_len -= 1;
            false
        };
        Vlu4SliceArrayIter {
            total_len: self.total_len,
            rdr: rdr_clone,
            stride_len,
            pos: 0,
            is_last_stride,
        }
    }

    pub fn len(&self) -> usize {
        self.total_len
    }
}

// impl<'i> IntoIterator for Vlu4SliceArray<'i> {
//     type Item = &'i [u8];
//     type IntoIter = Vlu4SliceArrayIter<'i>;
//
//     fn into_iter(self) -> Self::IntoIter {
//         self.iter()
//     }
// }

impl<'i> Display for Vlu4SliceArray<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let iter = self.iter();
        write!(f, "Vlu4SliceArray[{}]( ", self.total_len)?;
        for s in iter {
            write!(f, "{}:{:2x?} ", s.len(), s)?;
        }
        write!(f, ")")
    }
}

impl<'i> Debug for Vlu4SliceArray<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self)
    }
}

pub struct Vlu4SliceArrayIter<'i> {
    total_len: usize,
    rdr: NibbleBuf<'i>,
    stride_len: usize,
    pos: usize,
    is_last_stride: bool,
}

impl<'i> Iterator for Vlu4SliceArrayIter<'i> {
    type Item = &'i [u8];

    fn next(&mut self) -> Option<&'i [u8]> {
        if self.pos >= self.stride_len && self.is_last_stride {
            None
        } else {
            if self.pos >= self.stride_len {
                self.pos = 0;
                self.stride_len = self.rdr.get_nibble().unwrap_or(0) as usize;
                self.is_last_stride = if self.stride_len == 0 {
                    self.is_last_stride = true;
                    return None;
                } else if self.stride_len <= 14 {
                    true
                } else {
                    self.stride_len -= 1;
                    false
                };
            }
            self.pos += 1;
            let slice_len = self.rdr
                .get_vlu4_u32()
                .or_else(|e| {
                    self.pos = self.stride_len; // stop reading corrupt data, shouldn't be possible
                    self.is_last_stride = true;
                    Err(e)
                }).ok()?;
            match self.rdr.align_to_byte() {
                Ok(()) => {},
                Err(_) => {
                    self.pos = self.stride_len;
                    self.is_last_stride = true;
                    return None;
                }
            }
            Some(self.rdr.get_slice(slice_len as usize).ok()?)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.total_len, Some(self.total_len))
    }
}

impl<'i> FusedIterator for Vlu4SliceArrayIter<'i> {}

impl<'i> SerializeVlu4 for Vlu4SliceArray<'i> {
    type Error = XpiVlu4Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        let mut slices_left = self.total_len;
        let mut slices_iter = self.iter();
        while slices_left > 0 {
            let stride_len = if slices_left <= 14 {
                wgr.put_nibble(slices_left as u8)?;
                slices_left
            } else {
                wgr.put_nibble(0xf)?;
                14
            };
            slices_left -= stride_len;
            for _ in 0..stride_len {
                let slice = slices_iter.next().ok_or_else(|| XpiVlu4Error::Vlu4SliceArray)?;
                wgr.put_vlu4_u32(slice.len() as u32)?;
                wgr.align_to_byte()?;
                wgr.put_slice(slice)?;
            }
        }
        Ok(())
    }
}

impl<'i> DeserializeVlu4<'i> for Vlu4SliceArray<'i> {
    type Error = XpiVlu4Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let mut rdr_clone = rdr.clone();

        let mut total_len = 0;
        loop {
            // allow stride of len 15 followed by 0 for now, but do not create on purpose
            let mut len = rdr.get_nibble()? as usize;
            let is_last_stride = if len <= 14 {
                true
            } else {
                len -= 1;
                false
            };
            total_len += len;
            for _ in 0..len {
                let slice_len = rdr.get_vlu4_u32()? as usize;
                rdr.align_to_byte()?;
                rdr.skip(slice_len * 2)?;
            }
            if is_last_stride {
                break;
            }
        }
        rdr_clone.shrink_to_pos_of(rdr)?;

        Ok(Vlu4SliceArray {
            rdr: rdr_clone,
            total_len
        })
    }
}

#[cfg(test)]
mod test {
    use hex_literal::hex;
    use crate::serdes::{NibbleBuf, NibbleBufMut};
    use crate::serdes::vlu4::Vlu4SliceArray;

    #[test]
    fn aligned_start() {
        let input_buf = hex!("33 ab cd ef 20 ed cb 20 ab cd /* slices end */ 11 22");
        let mut buf = NibbleBuf::new_all(&input_buf);

        let slices: Vlu4SliceArray = buf.des_vlu4().unwrap();
        let mut iter = slices.iter();
        assert_eq!(iter.next(), Some(&input_buf[1..=3]));
        assert_eq!(iter.next(), Some(&input_buf[5..=6]));
        assert_eq!(iter.next(), Some(&input_buf[8..=9]));
        assert_eq!(iter.next(), None);

        assert_eq!(buf.get_u8(), Ok(0x11));
    }

    #[test]
    fn unaligned_start() {
        let input_buf = hex!("12 20 ab cd 20 ef fe 11");
        let mut buf = NibbleBuf::new_all(&input_buf);

        assert_eq!(buf.get_nibble(), Ok(1));

        let slices: Vlu4SliceArray = buf.des_vlu4().unwrap();
        let mut iter = slices.iter();
        assert_eq!(iter.next(), Some(&input_buf[2..=3]));
        assert_eq!(iter.next(), Some(&input_buf[5..=6]));
        assert_eq!(iter.next(), None);

        assert_eq!(buf.get_u8(), Ok(0x11));
    }

    #[test]
    fn round_trip() {
        let input_buf = hex!("22 ab cd 20 ef fe /* slices end */ aa bb");
        let mut buf = NibbleBuf::new_all(&input_buf);
        let slices: Vlu4SliceArray = buf.des_vlu4().unwrap();
        assert_eq!(slices.total_len, 2);
        assert_eq!(slices.rdr.nibbles_left(), 12);

        let mut output_buf = [0u8; 6];
        let mut wgr = NibbleBufMut::new_all(&mut output_buf);
        wgr.put(slices).unwrap();
        let (output_buf, _, is_at_byte_boundary) = wgr.finish();
        assert_eq!(output_buf, &[0x22, 0xab, 0xcd, 0x20, 0xef, 0xfe]);
        assert_eq!(is_at_byte_boundary, true);
    }

    #[test]
    fn round_trip_unaligned() {
        let input_buf = hex!("22 ab cd 20 ef fe /* slices end */ aa bb");
        let mut buf = NibbleBuf::new_all(&input_buf);
        let slices: Vlu4SliceArray = buf.des_vlu4().unwrap();
        assert_eq!(slices.total_len, 2);
        assert_eq!(slices.rdr.nibbles_left(), 12);

        let mut output_buf = [0u8; 7];
        let mut wgr = NibbleBufMut::new_all(&mut output_buf);
        wgr.put_nibble(0x7).unwrap();
        wgr.put(slices).unwrap();
        let (output_buf, _, is_at_byte_boundary) = wgr.finish();
        assert_eq!(output_buf, hex!("72 20 ab cd 20 ef fe"));
        assert_eq!(is_at_byte_boundary, true);
    }
}