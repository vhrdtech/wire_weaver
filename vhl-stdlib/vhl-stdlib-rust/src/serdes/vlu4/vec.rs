use crate::serdes::nibble_buf::Error as NibbleBufError;
use crate::serdes::traits::SerializeVlu4;
use crate::serdes::vlu4::vlu32::Vlu32;
use crate::serdes::DeserializeVlu4;
use crate::serdes::{NibbleBuf, NibbleBufMut};
use core::fmt::{Debug, Display, Formatter};
use core::iter::FusedIterator;
use core::marker::PhantomData;

/// Variable size array of u8 slices (each aligned to byte boundary).
/// Optimised for ease of writing in place - slice amount is written as 4 bits, with 0b1111 meaning
/// that there are more than 15 slices.
/// 4 bit slice count ~ (vlu4 slice len ~ padding? ~ u8 slice data)+ ~ (self)*
#[derive(Copy, Clone)]
pub struct Vlu4Vec<'i, T> {
    rdr: NibbleBuf<'i>,
    // total number of [u8] slices serialized
    total_len: usize,
    _phantom: PhantomData<T>,
}

impl<'i, T: DeserializeVlu4<'i>> Vlu4Vec<'i, T> {
    pub fn empty() -> Self {
        Vlu4Vec {
            rdr: NibbleBuf::new_all(&[]),
            total_len: 0,
            _phantom: PhantomData,
        }
    }

    pub fn iter(&self) -> Vlu4VecIter<'i, T> {
        let mut rdr_clone = self.rdr.clone();
        // NOTE: unwrap_or: should not happen, checked in DeserializeVlu4
        let mut stride_len = rdr_clone.get_nibble().unwrap_or(0) as usize;
        let is_last_stride = if stride_len <= 14 {
            true
        } else {
            stride_len -= 1;
            false
        };
        Vlu4VecIter {
            total_len: self.total_len,
            rdr: rdr_clone,
            stride_len,
            pos: 0,
            is_last_stride,
            _phantom: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.total_len
    }
}

impl<'i, T: DeserializeVlu4<'i>> IntoIterator for Vlu4Vec<'i, T> {
    type Item = T;
    type IntoIter = Vlu4VecIter<'i, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'i, T: DeserializeVlu4<'i> + Display> Display for Vlu4Vec<'i, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let iter = self.iter();
        write!(f, "Vlu4Vec[{}](", self.total_len)?;
        let len = iter.size_hint().0;
        for (i, t) in iter.enumerate() {
            write!(f, "{}", t)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        write!(f, ")")
    }
}

impl<'i, T: DeserializeVlu4<'i> + Debug> Debug for Vlu4Vec<'i, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let iter = self.iter();
        write!(f, "Vlu4Vec[{}](", self.total_len)?;
        let len = iter.size_hint().0;
        for (i, t) in iter.enumerate() {
            write!(f, "{:?}", t)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        write!(f, ")")
    }
}

#[derive(Clone)]
pub struct Vlu4VecIter<'i, T> {
    total_len: usize,
    rdr: NibbleBuf<'i>,
    stride_len: usize,
    pos: usize,
    is_last_stride: bool,
    _phantom: PhantomData<T>,
}

impl<'i, T: DeserializeVlu4<'i>> Iterator for Vlu4VecIter<'i, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
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

            match T::des_vlu4(&mut self.rdr) {
                Ok(t) => Some(t),
                Err(_) => {
                    // stop reading corrupt data, shouldn't happen because during deserialization
                    // data is checked to be correct
                    self.pos = self.stride_len;
                    self.is_last_stride = true;
                    None
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.total_len, Some(self.total_len))
    }
}

impl<'i, T: DeserializeVlu4<'i>> FusedIterator for Vlu4VecIter<'i, T> {}

impl<'i, T, E> SerializeVlu4 for Vlu4Vec<'i, T>
where
    T: SerializeVlu4<Error = E> + DeserializeVlu4<'i, Error = E>,
    E: From<NibbleBufError>,
{
    type Error = E;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        let mut elements_left = self.total_len;
        let mut elements_iter = self.iter();
        if elements_left == 0 {
            wgr.put_nibble(0)?;
        }
        while elements_left > 0 {
            let stride_len = if elements_left <= 14 {
                wgr.put_nibble(elements_left as u8)?;
                elements_left
            } else {
                wgr.put_nibble(0xf)?;
                14
            };
            elements_left -= stride_len;
            for _ in 0..stride_len {
                let element = elements_iter
                    .next()
                    .ok_or_else(|| NibbleBufError::Vlu4Vec)?;
                wgr.put(&element)?;
            }
        }
        Ok(())
    }

    fn len_nibbles(&self) -> usize {
        todo!()
    }
}

impl<'i, T: DeserializeVlu4<'i, Error = E>, E> DeserializeVlu4<'i> for Vlu4Vec<'i, T>
where
    E: From<NibbleBufError>,
{
    type Error = E;

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
                T::des_vlu4(rdr)?; // TODO: replace with check_and_skip_vlu4()
            }
            if is_last_stride {
                break;
            }
        }
        rdr_clone.shrink_to_pos_of(rdr)?;

        Ok(Vlu4Vec {
            rdr: rdr_clone,
            total_len,
            _phantom: PhantomData,
        })
    }
}

/// Allows to create a [Vlu4Vec] with unknown amount of elements with unknown lengths in place,
/// without allocations or making excessive copies.
///
/// Create an instance by calling [Vlu4VecBuilder::new()] or through [NibbleBufMut::put_vec()]
pub struct Vlu4VecBuilder<'i, T> {
    pub(crate) wgr: NibbleBufMut<'i>,
    pub(crate) idx_before: usize,
    pub(crate) is_at_byte_boundary_before: bool,

    pub(crate) stride_len: u8,
    pub(crate) stride_len_idx_nibbles: usize,
    pub(crate) slices_written: usize,

    pub(crate) _phantom: PhantomData<T>,
}

impl<'i, T> Vlu4VecBuilder<'i, T> {
    pub fn new(buf: &'i mut [u8]) -> Self {
        Vlu4VecBuilder {
            wgr: NibbleBufMut::new_all(buf),
            idx_before: 0,
            is_at_byte_boundary_before: true,
            stride_len: 0,
            stride_len_idx_nibbles: 0,
            slices_written: 0,
            _phantom: PhantomData,
        }
    }

    pub fn put<E>(&mut self, element: T) -> Result<(), E>
    where
        T: SerializeVlu4<Error = E>,
        E: From<NibbleBufError>,
    {
        let len = element.len_nibbles();
        self.start_putting_element()?;
        let pos_before = self.wgr.nibbles_pos();
        self.wgr.put(&element)?;
        if self.wgr.nibbles_pos() - pos_before != len {
            return Err(NibbleBufError::InvalidByteSizeEstimate.into());
        }
        self.finish_putting_element()?;
        Ok(())
    }

    fn start_putting_element(&mut self) -> Result<(), NibbleBufError> {
        if self.stride_len == 0 {
            self.stride_len_idx_nibbles = self.wgr.nibbles_pos();
            self.wgr.put_nibble(0)?;
        }
        Ok(())
    }

    fn put_len_bytes_and_align(&mut self, len_bytes: usize) -> Result<(), NibbleBufError> {
        if len_bytes == 0 {
            self.wgr.put_nibble(0)?;
            return Ok(());
        }
        self.wgr.put(&Vlu32(len_bytes as u32))?;
        self.wgr.align_to_byte()?;
        Ok(())
    }

    fn finish_putting_element(&mut self) -> Result<(), NibbleBufError> {
        self.stride_len += 1;
        self.slices_written += 1;

        if self.stride_len == 14 {
            self.wgr.replace_nibble(self.stride_len_idx_nibbles, 0xf)?;
            self.stride_len = 0;
        }
        Ok(())
    }

    pub fn slices_written(&self) -> usize {
        self.slices_written
    }

    /// Finish writing slices and get original NibbleBufMut back to continue writing to it.
    /// If no slices were provided, one 0 nibble is written to indicate an empty array.
    pub fn finish(mut self) -> Result<NibbleBufMut<'i>, NibbleBufError> {
        if self.slices_written == 0 {
            self.wgr.put_nibble(0)?;
        } else {
            self.wgr
                .replace_nibble(self.stride_len_idx_nibbles, self.stride_len)?;
        }
        Ok(self.wgr)
    }

    /// Finish writing elements ang get Vlu4Vec right away, without deserialization.
    /// If no slices were provided, one 0 nibble is written to indicate an empty array.
    pub fn finish_as_vec(mut self) -> Result<Vlu4Vec<'i, T>, NibbleBufError> {
        if self.slices_written == 0 {
            self.wgr.put_nibble(0)?;
        } else {
            self.wgr
                .replace_nibble(self.stride_len_idx_nibbles, self.stride_len)?;
        }
        let len_nibbles = self.wgr.nibbles_pos() - self.idx_before * 2;
        let last_idx = if self.wgr.is_at_byte_boundary {
            self.wgr.idx - 1
        } else {
            self.wgr.idx
        };
        Ok(Vlu4Vec {
            rdr: NibbleBuf {
                buf: &self.wgr.buf[self.idx_before..=last_idx],
                len_nibbles,
                idx: 0,
                is_at_byte_boundary: self.is_at_byte_boundary_before,
            },
            total_len: self.slices_written,
            _phantom: PhantomData,
        })
    }
}

/// Implementation of Vlu4VecBuilder for u8 slices, all methods ensure byte alignment.
impl<'i> Vlu4VecBuilder<'i, &'i [u8]> {
    /// Get a mutable, aligned u8 slice of requested length inside a closure.
    /// Slice is created in exactly the right spot, while adhering to the layout of Vlu4Vec.
    ///
    /// Example:
    /// ```
    /// use vhl_stdlib::serdes::NibbleBufMut;
    /// use vhl_stdlib::serdes::nibble_buf::Error as NibbleBufError;
    /// use vhl_stdlib::serdes::vlu4::{Vlu4Vec, Vlu4VecBuilder};
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
    ///  let args_set: Vlu4Vec<&[u8]> = {
    ///      let mut arb = Vlu4VecBuilder::new(&mut args_set);
    ///      arb.put_aligned_with::<MyError, _>(8, |slice| {
    ///          // write 8 bytes into slice with the help of BufMut, NibbleBufMut, BitBufMut or others.
    ///          Ok(())
    ///      }).unwrap();
    ///      arb.finish_as_vec().unwrap()
    ///  };
    /// ```
    pub fn put_aligned_with<SE, F>(&mut self, len_bytes: usize, f: F) -> Result<(), SE>
    where
        F: Fn(&mut [u8]) -> Result<(), SE>,
        SE: From<NibbleBufError>,
    {
        self.start_putting_element()?;
        self.put_len_bytes_and_align(len_bytes)?;
        f(&mut self.wgr.buf[self.wgr.idx..self.wgr.idx + len_bytes])?;
        self.wgr.idx += len_bytes;
        self.finish_putting_element()?;
        Ok(())
    }

    /// Put u8 slice into Vlu4Vec. Padding is added if necessary.
    pub fn put_aligned(&mut self, slice: &[u8]) -> Result<(), NibbleBufError> {
        self.start_putting_element()?;
        self.put_len_bytes_and_align(slice.len())?;
        if slice.len() != 0 {
            self.wgr.put_slice(slice)?;
        }
        self.finish_putting_element()?;
        Ok(())
    }
}

impl<'i, E, SE> Vlu4VecBuilder<'i, Result<&'i [u8], E>>
where
    E: SerializeVlu4<Error = SE>,
    SE: From<NibbleBufError>,
{
    pub fn put_result_with_slice(&mut self, result: Result<&'i [u8], E>) -> Result<(), SE> {
        self.start_putting_element()?;
        match result {
            Ok(slice) => {
                self.wgr.put_nibble(0)?;
                self.put_len_bytes_and_align(slice.len())?;
                if slice.len() != 0 {
                    self.wgr.put_slice(slice)?;
                }
                self.finish_putting_element()?;
                Ok(())
            }
            Err(e) => {
                self.wgr.put(&e)?;
                self.finish_putting_element()?;
                Ok(())
            }
        }
    }

    /// Get a mutable slice of requested length inside a closure. Put it as Ok(&[u8]) if f returns
    /// Ok(()) or as Err(E) otherwise.
    ///
    /// Slice is created in exactly the right spot, while adhering to the layout of Vlu4Vec.
    pub fn put_result_with_slice_from<F>(&mut self, len_bytes: usize, f: F) -> Result<(), SE>
    where
        F: Fn(&mut [u8]) -> Result<(), E>,
    {
        self.start_putting_element()?;
        let state = self.wgr.save_state();
        let stride_len_idx_nibbles_before = self.stride_len_idx_nibbles;
        self.wgr.put_nibble(0)?;

        self.put_len_bytes_and_align(len_bytes)?;
        match f(&mut self.wgr.buf[self.wgr.idx..self.wgr.idx + len_bytes]) {
            Ok(_) => {
                self.wgr.idx += len_bytes;
                self.finish_putting_element()?;
                Ok(())
            }
            Err(e) => {
                self.wgr.restore_state(state)?;
                self.stride_len_idx_nibbles = stride_len_idx_nibbles_before;
                self.wgr.put(&e)?;
                self.finish_putting_element()?;
                return Ok(());
            }
        }
    }
}

impl<'i> DeserializeVlu4<'i> for &'i [u8] {
    type Error = NibbleBufError;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let len = rdr.get_vlu4_u32()? as usize;
        if len == 0 {
            return Ok(&[]);
        }
        rdr.align_to_byte()?;
        Ok(rdr.get_slice(len)?)
    }
}

impl<'i> SerializeVlu4 for &'i [u8] {
    type Error = NibbleBufError;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        if self.len() == 0 {
            wgr.put_nibble(0)?;
            return Ok(());
        }
        wgr.put(&Vlu32(self.len() as u32))?;
        wgr.align_to_byte()?;
        wgr.put_slice(self)?;
        Ok(())
    }

    fn len_nibbles(&self) -> usize {
        // no way to return correct len, because it depends on buffer state which is unknown here
        panic!("<&[u8] as SerializeVlu4>::len_nibbles() is incorrect to call");
    }
}

impl<'i, T, E> DeserializeVlu4<'i> for Result<T, E>
where
    T: DeserializeVlu4<'i, Error = NibbleBufError>,
    E: From<u32>,
{
    type Error = NibbleBufError;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let code = rdr.get_vlu4_u32()?;
        if code == 0 {
            Ok(Ok(T::des_vlu4(rdr)?))
        } else {
            Ok(Err(E::from(code)))
        }
    }
}

impl<'i, T, E, SE> SerializeVlu4 for Result<T, E>
where
    T: SerializeVlu4<Error = SE>,
    E: SerializeVlu4<Error = SE>,
    SE: From<NibbleBufError>,
{
    type Error = SE;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        match self {
            Ok(t) => {
                wgr.put_nibble(0)?;
                wgr.put(t)?;
                Ok(())
            }
            Err(e) => {
                wgr.put(e)?;
                Ok(())
            }
        }
    }

    fn len_nibbles(&self) -> usize {
        match self {
            Ok(t) => t.len_nibbles() + 1,
            Err(e) => e.len_nibbles(),
        }
    }
}

impl SerializeVlu4 for () {
    type Error = NibbleBufError;

    fn ser_vlu4(&self, _wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        Ok(())
    }

    fn len_nibbles(&self) -> usize {
        0
    }
}

impl<'i> DeserializeVlu4<'i> for u32 {
    type Error = crate::serdes::nibble_buf::Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        rdr.get_vlu4_u32()
    }
}

#[cfg(test)]
mod test {
    extern crate std;
    // use std::println;
    use super::*;
    use crate::serdes::xpi_vlu4::error::FailReason;
    use hex_literal::hex;

    #[test]
    fn vec_of_slices() {
        let input = hex!("22 aa bb 20 cc dd");
        let mut nrd = NibbleBuf::new_all(&input);
        let array: Vlu4Vec<&[u8]> = nrd.des_vlu4().unwrap();
        let mut iter = array.iter();
        assert_eq!(iter.next(), Some(&[0xaa, 0xbb][..]));
        assert_eq!(iter.next(), Some(&[0xcc, 0xdd][..]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn vec_of_slices_builder() {
        let mut buf = [0u8; 64];
        let mut vb = Vlu4VecBuilder::<&[u8]>::new(&mut buf);
        vb.put_aligned(&[1, 2, 3]).unwrap();
        vb.put_aligned(&[4, 5]).unwrap();
        vb.put_aligned(&[]).unwrap();

        // stride len will be updated in finish_..
        assert_eq!(&vb.wgr.buf[0..8], hex!("03 01 02 03 20 04 05 00"));
        assert_eq!(vb.wgr.nibbles_pos(), 15);
        let vec = vb.finish_as_vec().unwrap();
        assert_eq!(&vec.rdr.buf[0..8], hex!("33 01 02 03 20 04 05 00"));
        assert_eq!(vec.total_len, 3);
        assert_eq!(vec.rdr.nibbles_left(), 15);

        let mut iter = vec.iter();
        assert_eq!(iter.next(), Some(&[1, 2, 3][..]));
        assert_eq!(iter.next(), Some(&[4, 5][..]));
        assert_eq!(iter.next(), Some(&[][..]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn vec_of_slices_builder_empty() {
        let mut buf = [0u8; 64];
        let vb = Vlu4VecBuilder::<&[u8]>::new(&mut buf);
        let vec = vb.finish_as_vec().unwrap();
        assert_eq!(vec.total_len, 0);
        let mut iter = vec.iter();
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn vec_of_unit_results() {
        let input = hex!("50 12 30");
        let mut nrd = NibbleBuf::new_all(&input);
        let results: Vlu4Vec<Result<(), FailReason>> = nrd.des_vlu4().unwrap();
        let mut iter = results.iter();
        assert_eq!(iter.next(), Some(Ok(())));
        assert_eq!(iter.next(), Some(Err(FailReason::Timeout)));
        assert_eq!(iter.next(), Some(Err(FailReason::DeviceRebooted)));
        assert_eq!(iter.next(), Some(Err(FailReason::PriorityLoss)));
        assert_eq!(iter.next(), Some(Ok(())));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn vec_of_unit_results_builder() {
        let mut buf = [0u8; 64];
        let mut vb = Vlu4VecBuilder::<Result<(), FailReason>>::new(&mut buf);
        vb.put(Ok(())).unwrap();
        vb.put(Ok(())).unwrap();
        vb.put(Err(FailReason::Timeout)).unwrap();
        vb.put(Err(FailReason::PriorityLoss)).unwrap();
        vb.put(Ok(())).unwrap();
        assert_eq!(&vb.wgr.buf[0..3], hex!("00 01 30"));
        assert_eq!(vb.wgr.nibbles_pos(), 6);
        let nwr = vb.finish().unwrap();
        assert_eq!(&nwr.buf[0..3], hex!("50 01 30"));
    }

    #[test]
    fn vec_of_slice_results() {
        let input = hex!("40 20 aa bb 01 cc 11");
        let mut nrd = NibbleBuf::new_all(&input);
        let results: Vlu4Vec<Result<&[u8], FailReason>> = nrd.des_vlu4().unwrap();
        let mut iter = results.iter();
        assert_eq!(iter.next(), Some(Ok(&[0xaa, 0xbb][..])));
        assert_eq!(iter.next(), Some(Ok(&[0xcc][..])));
        assert_eq!(iter.next(), Some(Err(FailReason::Timeout)));
        assert_eq!(iter.next(), Some(Err(FailReason::Timeout)));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn vec_of_slice_results_builder() {
        let mut buf = [0u8; 64];
        let mut vb = Vlu4VecBuilder::<Result<&[u8], FailReason>>::new(&mut buf);
        vb.put_result_with_slice(Ok(&[1, 2, 3])).unwrap();
        vb.put_result_with_slice(Ok(&[4, 5])).unwrap();
        vb.put_result_with_slice(Err(FailReason::Timeout)).unwrap();
        vb.put_result_with_slice(Ok(&[])).unwrap();
        vb.put_result_with_slice(Err(FailReason::Timeout)).unwrap();

        let vec = vb.finish_as_vec().unwrap();
        assert_eq!(&vec.rdr.buf[..10], hex!("50 30 01 02 03 02 04 05 10 01"));

        let mut iter = vec.iter();
        assert_eq!(iter.next(), Some(Ok(&[1, 2, 3][..])));
        assert_eq!(iter.next(), Some(Ok(&[4, 5][..])));
        assert_eq!(iter.next(), Some(Err(FailReason::Timeout)));
        assert_eq!(iter.next(), Some(Ok(&[][..])));
        assert_eq!(iter.next(), Some(Err(FailReason::Timeout)));
    }

    #[test]
    fn vec_of_vlu32() {
        let input = hex!("31 91 ff f7");
        let mut nrd = NibbleBuf::new_all(&input);
        let results: Vlu4Vec<u32> = nrd.des_vlu4().unwrap();
        let mut iter = results.iter();
        assert_eq!(iter.next(), Some(1));
        assert_eq!(iter.next(), Some(9));
        assert_eq!(iter.next(), Some(4095));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn aligned_start() {
        let input_buf = hex!("33 ab cd ef 20 ed cb 20 ab cd /* slices end */ 11 22");
        let mut buf = NibbleBuf::new_all(&input_buf);

        let slices: Vlu4Vec<&[u8]> = buf.des_vlu4().unwrap();
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

        let slices: Vlu4Vec<&[u8]> = buf.des_vlu4().unwrap();
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
        let slices: Vlu4Vec<&[u8]> = buf.des_vlu4().unwrap();
        assert_eq!(slices.total_len, 2);
        assert_eq!(slices.rdr.nibbles_left(), 12);

        let mut output_buf = [0u8; 6];
        let mut wgr = NibbleBufMut::new_all(&mut output_buf);
        wgr.put(&slices).unwrap();
        let (output_buf, _, is_at_byte_boundary) = wgr.finish();
        assert_eq!(output_buf, &[0x22, 0xab, 0xcd, 0x20, 0xef, 0xfe]);
        assert_eq!(is_at_byte_boundary, true);
    }

    #[test]
    fn round_trip_unaligned() {
        let input_buf = hex!("22 ab cd 20 ef fe /* slices end */ aa bb");
        let mut buf = NibbleBuf::new_all(&input_buf);
        let slices: Vlu4Vec<&[u8]> = buf.des_vlu4().unwrap();
        assert_eq!(slices.total_len, 2);
        assert_eq!(slices.rdr.nibbles_left(), 12);

        let mut output_buf = [0u8; 7];
        let mut wgr = NibbleBufMut::new_all(&mut output_buf);
        wgr.put_nibble(0x7).unwrap();
        wgr.put(&slices).unwrap();
        let (output_buf, _, is_at_byte_boundary) = wgr.finish();
        assert_eq!(output_buf, hex!("72 20 ab cd 20 ef fe"));
        assert_eq!(is_at_byte_boundary, true);
    }

    #[test]
    fn slice_array_builder_len_3() {
        let mut buf = [0u8; 256];
        let mut vb = Vlu4VecBuilder::<&[u8]>::new(&mut buf);
        vb.put_aligned(&[1, 2, 3]).unwrap();
        vb.put_aligned(&[4, 5, 6]).unwrap();
        vb.put_aligned(&[7, 8, 9]).unwrap();
        assert_eq!(vb.slices_written(), 3);
        let wgr = vb.finish().unwrap();
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

        let mut wgr = wgr.put_vec::<&[u8]>();
        assert_eq!(wgr.wgr.nibbles_pos(), 3);
        wgr.put_aligned(&[1, 2, 3]).unwrap();
        wgr.put_aligned(&[4, 5, 6]).unwrap();
        wgr.put_aligned(&[7, 8, 9]).unwrap();
        assert_eq!(wgr.slices_written(), 3);
        assert_eq!(wgr.wgr.nibbles_pos(), 28);

        let slice_array = wgr.finish_as_vec().unwrap();
        assert_eq!(
            &slice_array.rdr.buf[0..13],
            hex!("b3 30 01 02 03 30 04 05 06 30 07 08 09")
        );
        assert_eq!(slice_array.rdr.nibbles_pos(), 1);
        assert_eq!(slice_array.total_len, 3);
        assert_eq!(slice_array.rdr.buf[0], 0xb3); // should start from vec start
        assert_eq!(slice_array.rdr.buf.len(), 13);
        assert_eq!(slice_array.rdr.nibbles_left(), 25);
        let mut iter = slice_array.iter();
        assert_eq!(iter.next(), Some(&[1, 2, 3][..]));
        assert_eq!(iter.next(), Some(&[4, 5, 6][..]));
        assert_eq!(iter.next(), Some(&[7, 8, 9][..]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn long_slices_builder() {
        use rand::prelude::*;
        use rand_chacha::rand_core::SeedableRng;
        use rand_chacha::ChaCha8Rng;
        use std::vec::Vec;

        const START_LEN: usize = 16;
        const ITERATIONS: usize = 10;

        let mut buf = [0u8; 20_000];
        let mut vb = Vlu4VecBuilder::<&[u8]>::new(&mut buf);
        let mut len = START_LEN;
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        for _ in 0..ITERATIONS {
            let mut slice = Vec::new();
            slice.reserve(len);
            for _ in 0..len {
                slice.push(rng.gen_range(0..255u8));
            }
            vb.put_aligned(&slice).unwrap();
            len *= 2;
        }

        let vec = vb.finish_as_vec().unwrap();
        assert_eq!(vec.len(), ITERATIONS);
        let mut iter = vec.iter();
        let mut len = START_LEN;
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        for _ in 0..ITERATIONS {
            let slice = iter.next().unwrap();
            assert_eq!(slice.len(), len);
            for i in 0..len {
                assert_eq!(slice[i], rng.gen_range(0..255u8));
            }
            len *= 2;
        }
    }

    #[test]
    fn slice_array_builder_finish_as_slice_array_aligned() {
        let mut buf = [0u8; 32];
        let mut wgr = NibbleBufMut::new_all(&mut buf);
        wgr.put_u8(0xaa).unwrap();

        let mut wgr = wgr.put_vec::<&[u8]>();
        assert_eq!(wgr.wgr.nibbles_pos(), 2);
        wgr.put_aligned(&[1, 2, 3]).unwrap();
        wgr.put_aligned(&[4, 5, 6]).unwrap();
        wgr.put_aligned(&[7, 8, 9]).unwrap();
        assert_eq!(wgr.slices_written(), 3);
        assert_eq!(
            &wgr.wgr.buf[0..13],
            hex!("aa 03 01 02 03 30 04 05 06 30 07 08 09")
        );
        assert_eq!(wgr.wgr.nibbles_pos(), 26);

        let slice_array = wgr.finish_as_vec().unwrap();
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
        let mut wgr = Vlu4VecBuilder::<&[u8]>::new(&mut buf);
        for i in 0..20u8 {
            wgr.put_aligned(&[i, i + 1, i + 2]).unwrap();
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
        let slices: Vlu4Vec<&[u8]> = rdr.des_vlu4().unwrap();
        assert_eq!(slices.len(), 20);
        let mut slices_iter = slices.iter();
        for i in 0..20u8 {
            let slice = slices_iter.next().unwrap();
            assert_eq!(slice.len(), 3);
            assert_eq!(slice, &[i, i + 1, i + 2]);
        }
    }

    use crate::serdes::buf::{BufMut, Error as BufError};
    use crate::serdes::nibble_buf::Error as NibbleBufError;

    #[derive(Debug, PartialEq, Eq)]
    enum MyError {
        NibbleBufError(NibbleBufError),
        BufError(BufError),
        // Fake,
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
    fn put_aligned_with() {
        let mut args_set = [0u8; 128];
        let args_set = {
            let wgr = NibbleBufMut::new_all(&mut args_set);
            let mut wgr = wgr.put_vec::<&[u8]>();
            wgr.put_aligned_with::<MyError, _>(4, |slice| {
                let mut wgr = BufMut::new(slice);
                wgr.put_u16_le(0x1234)?;
                wgr.put_u16_le(0x5678)?;
                Ok(())
            })
            .unwrap();
            assert_eq!(&wgr.wgr.buf[0..5], hex!("04 34 12 78 56"));
            wgr.finish_as_vec().unwrap()
        };
        assert_eq!(args_set.total_len, 1);
        assert_eq!(args_set.rdr.nibbles_pos(), 0);
        assert_eq!(args_set.rdr.nibbles_left(), 10);
        let mut iter = args_set.iter();
        assert_eq!(iter.next(), Some(&[0x34, 0x12, 0x78, 0x56][..]));
        assert_eq!(iter.next(), None);
    }
}
