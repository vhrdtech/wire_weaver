use crate::{Error, SerializeShrinkWrap};

/// no_std buffer writer that supports 1 bit, 4 bit, variable length integer and other operations.
/// No alignment requirements are imposed on the byte buffer provided.
/// Allocator is not required for no_std use. See BufWriterOwned for std use.
///
/// # Example
/// ```
/// let mut buf = [0u8; 256];
/// let mut wr = shrink_wrap::BufWriter::new(&mut buf);
/// wr.write_bool(true).unwrap();
/// wr.write_u8(0xaa).unwrap();
/// assert_eq!(wr.finish(), &[0x80, 0xaa]);
/// ```
pub struct BufWriter<'i> {
    buf: &'i mut [u8],
    // Maximum number of bytes available (not whole slice might be available)
    len_bytes: usize,
    // Next byte to write to
    idx: usize,
    // Next bit to write to
    bit_idx: u8,
}

impl<'i> BufWriter<'i> {
    pub fn new(buf: &'i mut [u8]) -> Self {
        let len_bytes = buf.len();
        Self {
            buf,
            len_bytes,
            idx: 0,
            bit_idx: 0,
        }
    }

    pub fn write_bool(&mut self, val: bool) -> Result<(), Error> {
        Ok(())
    }

    pub fn write_u8(&mut self, val: u8) -> Result<(), Error> {
        Ok(())
    }

    pub fn write_f32(&mut self, val: f32) -> Result<(), Error> {
        Ok(())
    }

    pub fn write<T: SerializeShrinkWrap>(&mut self, val: &T) -> Result<(), Error> {
        val.ser_shrink_wrap(self)
    }

    pub fn finish(self) -> &'i [u8] {
        if self.bit_idx == 0 {
            &self.buf[0..self.idx]
        } else {
            &self.buf[0..=self.idx]
        }
    }
}
