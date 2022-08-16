use core::fmt::{Debug, Display, Formatter};
use core::iter::FusedIterator;
use crate::serdes::NibbleBuf;
use crate::serdes::DeserializeVlu4;

/// Variable size array of u8 slices, aligned to byte boundary.
///
/// 4 bit padding is inserted and skipped if needed before the slices data start.
#[derive(Copy, Clone)]
pub struct Vlu4SliceArray<'i> {
    rdr_lengths: NibbleBuf<'i>,
    rdr_slices: NibbleBuf<'i>,
    // number of [u8] slices serialized
    len: usize,
}

impl<'i> Vlu4SliceArray<'i> {
    pub fn iter(&self) -> Vlu4SliceArrayIter {
        Vlu4SliceArrayIter {
            array: self.clone(), pos: 0
        }
    }

    pub fn len(&self) -> usize {
        self.len
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
        write!(f, "Vlu4SliceArray[{}]( ", self.len)?;
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
    array: Vlu4SliceArray<'i>,
    pos: usize,
}

impl<'i> Iterator for Vlu4SliceArrayIter<'i> {
    type Item = &'i [u8];

    fn next(&mut self) -> Option<&'i [u8]> {
        if self.pos >= self.array.len {
            None
        } else {
            self.pos += 1;
            let slice_len = self.array.rdr_lengths
                .get_vlu4_u32()
                .or_else(|e| {
                    self.pos = self.array.len; // stop reading corrupt data
                    Err(e)
                }).ok()?;
            Some(self.array.rdr_slices.get_slice(slice_len as usize).ok()?)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.array.len, Some(self.array.len))
    }
}

impl<'i> FusedIterator for Vlu4SliceArrayIter<'i> {}

impl<'i> DeserializeVlu4<'i> for Vlu4SliceArray<'i> {
    type Error = crate::serdes::nibble_buf::Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let len = rdr.get_vlu4_u32()? as usize;
        let rdr_before_lengths = rdr.clone();
        for _ in 0..len {
            let _slice_len = rdr.get_vlu4_u32()? as usize;
        }
        if !rdr.is_at_byte_boundary() {
            let _padding = rdr.get_nibble()?;
        }
        let rdr_before_slices = rdr.clone();
        let mut rdr_before_lengths_clone = rdr_before_lengths.clone();
        for _ in 0..len {
            let slice_len = rdr_before_lengths_clone.get_vlu4_u32()? as usize;
            let _slice = rdr.get_slice(slice_len)?;
        }

        Ok(Vlu4SliceArray {
            rdr_lengths: rdr_before_lengths,
            rdr_slices: rdr_before_slices,
            len
        })
    }
}

#[cfg(test)]
mod test {
    use hex_literal::hex;
    use crate::serdes::NibbleBuf;
    use crate::serdes::vlu4::Vlu4SliceArray;

    #[test]
    fn aligned_start() {
        let input_buf = hex!("32 32 ab cd ef ed cb ab cd /* slices end */ 11 22");
        let mut buf = NibbleBuf::new_all(&input_buf);

        let slices: Vlu4SliceArray = buf.des_vlu4().unwrap();
        let mut iter = slices.iter();
        assert_eq!(iter.next(), Some(&input_buf[2..=3]));
        assert_eq!(iter.next(), Some(&input_buf[4..=6]));
        assert_eq!(iter.next(), Some(&input_buf[7..=8]));
        assert_eq!(iter.next(), None);

        assert_eq!(buf.get_u8(), Ok(0x11));
    }

    #[test]
    fn unaligned_start() {
        let input_buf = hex!("12 22 ab cd ef fe 11");
        let mut buf = NibbleBuf::new_all(&input_buf);

        assert_eq!(buf.get_nibble(), Ok(1));

        let slices: Vlu4SliceArray = buf.des_vlu4().unwrap();
        let mut iter = slices.iter();
        assert_eq!(iter.next(), Some(&input_buf[2..=3]));
        assert_eq!(iter.next(), Some(&input_buf[4..=5]));
        assert_eq!(iter.next(), None);

        assert_eq!(buf.get_u8(), Ok(0x11));
    }
}