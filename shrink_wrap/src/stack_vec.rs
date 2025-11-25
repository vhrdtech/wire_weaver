use crate::{BufReader, BufWriter, DeserializeShrinkWrap, Error, SerializeShrinkWrap};
use core::marker::PhantomData;
/// A Vec-like container on the stack, storing `Option<T>` in a serialized form.
/// `T` must implement SerializeShrinkWrap + DeserializeShrinkWrap.
/// Allows to conveniently work with a dynamically sized object without allocation and specifying upper size bounds for each dynamic element.
///
/// Without this type, one could use a struct like shown below, but M and N must be set in advance:
/// ```rust
/// struct MaxBound<const M: usize, const N: usize> {
///     a: [u8; M],
///     b: [u8; N]
/// }
/// ```
///
/// With StackVec though, any complex object can be stored on the stack, with only one buffer size to choose:
/// ```rust
/// use shrink_wrap::stack_vec::StackVec;
///
/// let value = StackVec::<6, _>::some(([1u8, 2, 3, 4], [5u8, 6])).unwrap();
/// assert_eq!(value.bytes(), &[1, 2, 3, 4, 5, 6]);
/// let value = StackVec::<6, _>::some(([7u8, 8], [9u8, 10, 11, 12])).unwrap();
/// assert_eq!(value.bytes(), &[7, 8, 9, 10, 11, 12]);
/// ```
/// Note how only 6 bytes for the total buffer size need to be specified and buffer is distributed between two arrays.
///
/// See
pub struct StackVec<'i, const N: usize, T> {
    data: [u8; N],
    used: usize,
    _type: PhantomData<&'i T>,
}

impl<'i, T: SerializeShrinkWrap + DeserializeShrinkWrap<'i>, const N: usize> StackVec<'i, N, T> {
    pub fn some(value: T) -> Result<Self, Error> {
        let mut buf = [0u8; N];
        let mut wr = BufWriter::new(&mut buf);
        value.ser_shrink_wrap(&mut wr)?;
        let used = wr.finish_and_take()?.len();
        Ok(Self {
            data: buf,
            used,
            _type: Default::default(),
        })
    }

    pub fn none() -> Self {
        StackVec {
            data: [0u8; N],
            used: 0,
            _type: Default::default(),
        }
    }

    pub fn get(&'i self) -> Result<T, Error> {
        let mut rd = BufReader::new(&self.data[..self.used]);
        let value = T::des_shrink_wrap(&mut rd)?;
        Ok(value)
    }

    pub fn set(&mut self, value: T) -> Result<(), Error> {
        let mut wr = BufWriter::new(&mut self.data);
        value.ser_shrink_wrap(&mut wr)?;
        self.used = wr.finish_and_take()?.len();
        Ok(())
    }

    pub fn set_bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.data.len() < bytes.len() {
            return Err(Error::OutOfBoundsWriteRawSlice);
        }
        self.data[..bytes.len()].copy_from_slice(bytes);
        self.used = bytes.len();
        Ok(())
    }

    pub fn clear(&mut self) {
        self.used = 0;
    }

    pub fn bytes(&'i self) -> &'i [u8] {
        &self.data[..self.used]
    }
}

#[cfg(test)]
mod tests {
    use crate::stack_vec::StackVec;
    use hex_literal::hex;

    #[test]
    fn string_array() {
        let value = ["abc", "def", "gh"];
        let value = StackVec::<32, _>::some(value).unwrap();
        assert_eq!(value.bytes(), hex!("616263 646566 6768 0 2 3 3"));
        let get = value.get().unwrap();
        assert_eq!(get[0], "abc");
        assert_eq!(get[1], "def");
        assert_eq!(get[2], "gh");
    }

    #[test]
    fn byte_arrays() {
        let value = StackVec::<6, _>::some(([1u8, 2, 3, 4], [5u8, 6])).unwrap();
        assert_eq!(value.bytes(), hex!("01020304 0506"));
        let value = StackVec::<6, _>::some(([1u8, 2], [3u8, 4, 5, 6])).unwrap();
        assert_eq!(value.bytes(), hex!("0102 03040506"));
    }
}
