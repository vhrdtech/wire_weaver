use core::iter::FusedIterator;
use crate::serdes::{NibbleBuf, };
use crate::serdes::DeserializeVlu4;

/// Variable length array of u32 numbers based on vlu4 encoding without allocations.
#[derive(Copy, Clone, Debug)]
pub struct Vlu4U32Array<'i> {
    rdr: NibbleBuf<'i>,
    // number of vlu4 encoded numbers inside
    len: usize,
}

impl<'i> Vlu4U32Array<'i> {
    // pub fn new(mut rdr: NibbleBuf<'i>) -> Self {
    //     let len = rdr.get_vlu4_u32() as usize;
    //     Vlu4U32Array { rdr, len }
    // }

    pub fn iter(&self) -> Vlu4U32ArrayIter<'i> {
        Vlu4U32ArrayIter {
            array: self.clone(), pos: 0
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    //
    // /// Skip all elements of this array without reading them and return the rest of the input buffer
    // pub fn lookahead(&self) -> NibbleBuf<'i> {
    //     let mut rdr = self.rdr.clone();
    //     for _ in 0..self.len {
    //         rdr = NibbleBuf::lookahead_vlu4_u32(rdr);
    //     }
    //     rdr
    // }
}

impl<'i> DeserializeVlu4<'i> for Vlu4U32Array<'i> {
    type Error = crate::serdes::nibble_buf::Error;

    fn des_vlu4(rdr: & mut NibbleBuf<'i>) -> Result<Vlu4U32Array<'i>, Self::Error> {
        let len = rdr.get_vlu4_u32()? as usize;
        let rdr_before_elements = rdr.clone();
        for _ in 0..len {
            rdr.skip_vlu4_u32()?;
        }
        Ok(Vlu4U32Array {
            rdr: rdr_before_elements,
            len
        })
    }
}

impl<'i> IntoIterator for Vlu4U32Array<'i> {
    type Item = u32;
    type IntoIter = Vlu4U32ArrayIter<'i>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct Vlu4U32ArrayIter<'i> {
    array: Vlu4U32Array<'i>,
    pos: usize,
}

impl<'i> Iterator for Vlu4U32ArrayIter<'i> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.array.len {
            None
        } else {
            self.pos += 1;
            self.array.rdr.get_vlu4_u32().ok()
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.array.len, Some(self.array.len))
    }
}

impl<'i> FusedIterator for Vlu4U32ArrayIter<'i> {}

#[cfg(test)]
mod test {
    use crate::serdes::NibbleBuf;
    use super::Vlu4U32Array;

    #[test]
    fn vlu4_u32_array_iter() {
        let buf = [0x51, 0x23, 0x45, 0x77];
        let mut buf = NibbleBuf::new_all(&buf);

        let arr: Vlu4U32Array = buf.des_vlu4().unwrap();
        // use crate::serdes::vlu4::DeserializeVlu4;
        // let arr = Vlu4U32Array::des_vlu4(&mut buf);
        assert_eq!(buf.nibbles_pos(), 6);

        assert_eq!(arr.len(), 5);
        let mut iter = arr.into_iter();
        assert_eq!(iter.next(), Some(1));
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next(), Some(3));
        assert_eq!(iter.next(), Some(4));
        assert_eq!(iter.next(), Some(5));
        assert_eq!(iter.next(), None);
    }
}
