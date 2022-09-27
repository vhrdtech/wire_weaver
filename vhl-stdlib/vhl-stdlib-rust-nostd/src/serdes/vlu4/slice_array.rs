use core::fmt::{Debug, Display, Formatter};
use core::iter::FusedIterator;
use crate::serdes::{NibbleBuf, NibbleBufMut};
use crate::serdes::buf::BufMut;
use crate::serdes::DeserializeVlu4;
use crate::serdes::traits::{SerializeBytes, SerializeVlu4};
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

    fn len_nibbles(&self) -> usize {
        todo!()
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

/// Allows to create a [Vlu4SliceArray] with unknown amount of slices with unknown lengths in place,
/// without making copies or data relocations.
///
/// Create an instance through [NibbleBufMut::put_slice_array()]
pub struct Vlu4SliceArrayBuilder<'i> {
    pub(crate) wgr: NibbleBufMut<'i>,
    pub(crate) idx_before: usize,
    pub(crate) is_at_byte_boundary_before: bool,

    pub(crate) stride_len: u8,
    pub(crate) stride_len_idx_nibbles: usize,
    pub(crate) slices_written: usize,
}

impl<'i> Vlu4SliceArrayBuilder<'i> {
    /// Write a slice and create correct [Vlu4SliceArray] layout at the same time.
    pub fn put_slice(&mut self, slice: &[u8]) -> Result<(), crate::serdes::nibble_buf::Error> {
        self.start_putting_slice(slice.len())?;
        self.wgr.put_slice(slice)?;
        self.finish_putting_slice()
    }

    fn start_putting_slice(&mut self, len: usize) -> Result<(), crate::serdes::nibble_buf::Error> {
        if self.stride_len == 0 {
            self.stride_len_idx_nibbles = self.wgr.nibbles_pos();
            self.wgr.put_nibble(0)?;
        }

        self.wgr.put_vlu4_u32(len as u32)?;
        self.wgr.align_to_byte()
    }

    fn finish_putting_slice(&mut self) -> Result<(), crate::serdes::nibble_buf::Error> {
        self.stride_len += 1;
        self.slices_written += 1;

        if self.stride_len == 14 {
            self.wgr.replace_nibble(self.stride_len_idx_nibbles, 0xf)?;
            self.stride_len = 0;
        } else {
            self.wgr.replace_nibble(self.stride_len_idx_nibbles, self.stride_len)?;
        }
        Ok(())
    }

    /// Serialize any type that implements SerializeBytes as a slice into this buffer.
    pub fn put_bytes<E, T: SerializeBytes<Error = E>>(&mut self, t: &T) -> Result<(), E>
        where E: From<crate::serdes::nibble_buf::Error>,
    {
        self.start_putting_slice(t.len_bytes())?;
        let mut wgr = BufMut::new(
            &mut self.wgr.buf[self.wgr.idx .. self.wgr.idx + t.len_bytes()]
        );
        wgr.put(t)?;
        let (_, pos) = wgr.finish();
        if pos != t.len_bytes() {
            return Err(crate::serdes::nibble_buf::Error::InvalidByteSizeEstimate.into());
        }
        self.wgr.idx += t.len_bytes();
        self.finish_putting_slice()?;
        Ok(())
    }

    /// Get a mutable slice of requested length inside a closure. Slice is created in exactly the
    /// right spot, while adhering to the layout of Vlu4SliceArray.
    ///
    /// Example:
    /// ```
    /// use vhl_stdlib::serdes::NibbleBufMut;
    /// use vhl_stdlib::serdes::nibble_buf::Error as NibbleBufError;
    /// use vhl_stdlib::serdes::vlu4::Vlu4SliceArray;
    ///
    /// #[derive(Debug)]
    /// enum MyError {
    ///     NibbleBufError(NibbleBufError),
    /// }
    /// impl From<NibbleBufError> for MyError {
    /// fn from(e: NibbleBufError) -> Self {
    ///         MyError::NibbleBufError(e)
    ///     }
    /// }
    ///
    ///  let mut args_set = [0u8; 128];
    ///  let args_set: Vlu4SliceArray = {
    ///      let wgr = NibbleBufMut::new_all(&mut args_set);
    ///      let mut wgr = wgr.put_slice_array();
    ///      wgr.put_exact::<MyError, _>(8, |slice| {
    ///          // write 8 bytes into slice with the help of BufMut, NibbleBufMut, BitBufMut or others.
    ///          Ok(())
    ///      }).unwrap();
    ///      wgr.finish_as_slice_array().unwrap()
    ///  };
    /// ```
    pub fn put_exact<E, F>(&mut self, len: usize, f: F) -> Result<(), E> where
        F: Fn(&mut [u8]) -> Result<(), E>,
        E: From<crate::serdes::nibble_buf::Error>
    {
        self.start_putting_slice(len)?;
        f(&mut self.wgr.buf[self.wgr.idx .. self.wgr.idx + len])?;
        self.wgr.idx += len;
        self.finish_putting_slice()?;
        Ok(())
    }

    /// Get a mutable slice of requested length inside a closure. Slice is created in exactly the
    /// right spot, while adhering to the layout of Vlu4SliceArray.
    /// If closure returns an error, restore state as it was before calling this functions.
    pub fn put_exact_or_rewind<E, F>(&mut self, len: usize, f: F)
        -> Result<Result<(), E>, crate::serdes::nibble_buf::Error> where
        F: Fn(&mut [u8]) -> Result<(), E>,
        E: From<crate::serdes::nibble_buf::Error>
    {
        let state = self.wgr.save_state();
        let stride_len_idx_nibbles_before = self.stride_len_idx_nibbles;
        self.start_putting_slice(len)?;
        match f(&mut self.wgr.buf[self.wgr.idx .. self.wgr.idx + len]) {
            Ok(_) => {
                self.wgr.idx += len;
                self.finish_putting_slice()?;
                Ok(Ok(()))
            }
            Err(e) => {
                self.wgr.restore_state(state)?;
                self.stride_len_idx_nibbles = stride_len_idx_nibbles_before;
                return Ok(Err(e))
            }
        }
    }

    pub fn slices_written(&self) -> usize {
        self.slices_written
    }

    /// Finish writing slices and get original NibbleBufMut back to continue writing to it.
    /// If no slices were provided, one 0 nibble is written to indicate an empty array.
    pub fn finish(mut self) -> Result<NibbleBufMut<'i>, crate::serdes::nibble_buf::Error> {
        if self.slices_written == 0 {
            self.wgr.put_nibble(0)?;
        }
        Ok(self.wgr)
    }

    /// Finish writing slices ang get Vlu4SliceArray right away, without deserialization.
    /// If no slices were provided, one 0 nibble is written to indicate an empty array.
    pub fn finish_as_slice_array(mut self) -> Result<Vlu4SliceArray<'i>, crate::serdes::nibble_buf::Error> {
        if self.slices_written == 0 {
            self.wgr.put_nibble(0)?;
        }
        let len_nibbles = (self.wgr.idx - self.idx_before) * 2;
        Ok(Vlu4SliceArray {
            rdr: NibbleBuf {
                buf: &self.wgr.buf[self.idx_before .. self.wgr.idx],
                len_nibbles,
                idx: 0,
                is_at_byte_boundary: self.is_at_byte_boundary_before
            },
            total_len: self.slices_written
        })
    }
}

#[cfg(test)]
mod test {
    // extern crate std;
    // use std::println;
    use hex_literal::hex;
    use crate::serdes::{NibbleBuf, NibbleBufMut};
    use crate::serdes::buf::BufMut;
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

    #[test]
    fn slice_array_builder_len_3() {
        let mut buf = [0u8; 256];
        let wgr = NibbleBufMut::new_all(&mut buf);
        let mut wgr = wgr.put_slice_array();
        wgr.put_slice(&[1, 2, 3]).unwrap();
        wgr.put_slice(&[4, 5, 6]).unwrap();
        wgr.put_slice(&[7, 8, 9]).unwrap();
        assert_eq!(wgr.slices_written(), 3);
        let wgr = wgr.finish().unwrap();
        assert_eq!(wgr.nibbles_pos(), 24);
        let (buf, len, _) = wgr.finish();
        assert_eq!(&buf[0..len], hex!("33 01 02 03 30 04 05 06 30 07 08 09"));
    }

    #[test]
    fn slice_array_builder_finish_as_slice_array_unaligned() {
        let mut buf = [0u8; 32];
        let mut wgr = NibbleBufMut::new_all(&mut buf);
        wgr.put_u8(0xaa).unwrap();
        wgr.put_nibble(0xb).unwrap();

        let mut wgr = wgr.put_slice_array();
        assert_eq!(wgr.wgr.nibbles_pos(), 3);
        wgr.put_slice(&[1, 2, 3]).unwrap();
        wgr.put_slice(&[4, 5, 6]).unwrap();
        wgr.put_slice(&[7, 8, 9]).unwrap();
        assert_eq!(wgr.slices_written(), 3);
        assert_eq!(&wgr.wgr.buf[0..14], hex!("aa b3 30 01 02 03 30 04 05 06 30 07 08 09"));
        assert_eq!(wgr.wgr.nibbles_pos(), 28);

        let slice_array = wgr.finish_as_slice_array().unwrap();
        assert_eq!(slice_array.total_len, 3);
        assert_eq!(slice_array.rdr.buf[0], 0xb3); // should start from correct position, not the start
        assert_eq!(slice_array.rdr.nibbles_left(), 25);
        let mut iter = slice_array.iter();
        assert_eq!(iter.next(), Some(&[1, 2, 3][..]));
        assert_eq!(iter.next(), Some(&[4, 5, 6][..]));
        assert_eq!(iter.next(), Some(&[7, 8, 9][..]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn slice_array_builder_finish_as_slice_array_aligned() {
        let mut buf = [0u8; 32];
        let mut wgr = NibbleBufMut::new_all(&mut buf);
        wgr.put_u8(0xaa).unwrap();

        let mut wgr = wgr.put_slice_array();
        assert_eq!(wgr.wgr.nibbles_pos(), 2);
        wgr.put_slice(&[1, 2, 3]).unwrap();
        wgr.put_slice(&[4, 5, 6]).unwrap();
        wgr.put_slice(&[7, 8, 9]).unwrap();
        assert_eq!(wgr.slices_written(), 3);
        assert_eq!(&wgr.wgr.buf[0..13], hex!("aa 33 01 02 03 30 04 05 06 30 07 08 09"));
        assert_eq!(wgr.wgr.nibbles_pos(), 26);

        let slice_array = wgr.finish_as_slice_array().unwrap();
        assert_eq!(slice_array.total_len, 3);
        assert_eq!(slice_array.rdr.buf[0], 0x33); // should start from correct position, not the start
        assert_eq!(slice_array.rdr.nibbles_left(), 24);
        let mut iter = slice_array.iter();
        assert_eq!(iter.next(), Some(&[1, 2, 3][..]));
        assert_eq!(iter.next(), Some(&[4, 5, 6][..]));
        assert_eq!(iter.next(), Some(&[7, 8, 9][..]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn slice_array_builder_len_20() {
        let mut buf = [0u8; 256];
        let wgr = NibbleBufMut::new_all(&mut buf);
        let mut wgr = wgr.put_slice_array();
        for i in 0..20u8 {
            wgr.put_slice(&[i, i + 1, i + 2]).unwrap();
        }
        assert_eq!(wgr.slices_written(), 20);
        let wgr = wgr.finish().unwrap();

        let (buf, pos, is_at_byte_boundary) = wgr.finish();
        let len_nibbles = if is_at_byte_boundary {
            pos * 2
        } else {
            pos * 2 + 1
        };
        let mut rdr = NibbleBuf::new(buf, len_nibbles).unwrap();
        let slices: Vlu4SliceArray = rdr.des_vlu4().unwrap();
        assert_eq!(slices.len(), 20);
        let mut slices_iter = slices.iter();
        for i in 0..20u8 {
            let slice = slices_iter.next().unwrap();
            assert_eq!(slice.len(), 3);
            assert_eq!(slice, &[i, i + 1, i + 2]);
        }
    }

    use crate::serdes::nibble_buf::Error as NibbleBufError;
    use crate::serdes::buf::Error as BufError;

    #[derive(Debug, PartialEq, Eq)]
    enum MyError {
        NibbleBufError(NibbleBufError),
        BufError(BufError),
        Fake,
    }

    impl From<NibbleBufError> for MyError {
        fn from(e: NibbleBufError) -> Self {
            MyError::NibbleBufError(e)
        }
    }

    impl From<BufError> for MyError {
        fn from(e: BufError) -> Self {
            MyError::BufError(e)
        }
    }

    #[test]
    fn put_exact() {
        let mut args_set = [0u8; 128];
        let args_set = {
            let wgr = NibbleBufMut::new_all(&mut args_set);
            let mut wgr = wgr.put_slice_array();
            wgr.put_exact::<MyError, _>(4, |slice| {
                let mut wgr = BufMut::new(slice);
                wgr.put_u16_le(0x1234)?;
                wgr.put_u16_le(0x5678)?;
                Ok(())
            }).unwrap();
            assert_eq!(&wgr.wgr.buf[0..5], hex!("14 34 12 78 56"));
            wgr.finish_as_slice_array().unwrap()
        };
        assert_eq!(args_set.total_len, 1);
        assert_eq!(args_set.rdr.nibbles_pos(), 0);
        assert_eq!(args_set.rdr.nibbles_left(), 10);
        let mut iter = args_set.iter();
        assert_eq!(iter.next(), Some(&[0x34, 0x12, 0x78, 0x56][..]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn put_exact_or_rewind_ok() {
        let mut args_set = [0u8; 128];
        let args_set = {
            let wgr = NibbleBufMut::new_all(&mut args_set);
            let mut wgr = wgr.put_slice_array();
            wgr.put_exact_or_rewind::<MyError, _>(4, |slice| {
                let mut wgr = BufMut::new(slice);
                wgr.put_u16_le(0x1234)?;
                wgr.put_u16_le(0x5678)?;
                Ok(())
            }).unwrap();
            assert_eq!(&wgr.wgr.buf[0..5], hex!("14 34 12 78 56"));
            wgr.finish_as_slice_array().unwrap()
        };
        assert_eq!(args_set.total_len, 1);
        assert_eq!(args_set.rdr.nibbles_pos(), 0);
        assert_eq!(args_set.rdr.nibbles_left(), 10);
        let mut iter = args_set.iter();
        assert_eq!(iter.next(), Some(&[0x34, 0x12, 0x78, 0x56][..]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn put_exact_or_rewind_err() {
        let mut args_set = [0u8; 128];
        let args_set = {
            let wgr = NibbleBufMut::new_all(&mut args_set);
            let mut wgr = wgr.put_slice_array();
            let r = wgr.put_exact_or_rewind::<MyError, _>(4, |slice| {
                let mut wgr = BufMut::new(slice);
                wgr.put_u16_le(0x1234)?;
                wgr.put_u16_le(0x5678)?;
                Err(MyError::Fake)
            }).unwrap();
            assert_eq!(wgr.slices_written, 0);
            assert_eq!(r, Err(MyError::Fake));
            wgr.finish_as_slice_array().unwrap()
        };
        assert_eq!(args_set.total_len, 0);
    }
}