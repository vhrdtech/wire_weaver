use std::vec::Vec;

use crate::MAX_DISTANCE;

/// Host side of the stream: decodes the [`Encoder`](crate::Encoder)'s blocks in order, keeping the last 64 KiB it
/// decoded as the dictionary of the next block.
pub struct Decoder {
    /// History (at least the last [`MAX_DISTANCE`] bytes), then room for one chunk.
    buf: Vec<u8>,
    max_chunk: usize,
}

/// A block that didn't decode. The history is unchanged, but the stream is out of step with the encoder: reset both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The block decodes to more than `max_chunk` bytes.
    TooLong,
    /// Not a valid LZ4 block, or one reaching back beyond the history.
    Corrupt,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::TooLong => f.write_str("LZ4 block longer than the largest chunk"),
            DecodeError::Corrupt => f.write_str("corrupt LZ4 block"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl Decoder {
    /// A decoder for blocks of at most `max_chunk` bytes (the encoder's [`max_chunk`](crate::Encoder::max_chunk)).
    pub fn new(max_chunk: usize) -> Self {
        Decoder { buf: Vec::with_capacity(2 * (MAX_DISTANCE + 1) + max_chunk), max_chunk }
    }

    /// Decodes the next block, returns its bytes.
    pub fn decode(&mut self, block: &[u8]) -> Result<&[u8], DecodeError> {
        // trim the history only once it is twice the window, so the copy is amortized
        if self.buf.len() > 2 * (MAX_DISTANCE + 1) {
            self.buf.drain(..self.buf.len() - (MAX_DISTANCE + 1));
        }
        let start = self.buf.len();
        self.buf.resize(start + self.max_chunk, 0);
        let (history, out) = self.buf.split_at_mut(start);
        let dict = &history[start.saturating_sub(MAX_DISTANCE + 1)..];
        match lz4_flex::block::decompress_into_with_dict(block, out, dict) {
            Ok(n) => {
                self.buf.truncate(start + n);
                Ok(&self.buf[start..])
            }
            Err(e) => {
                self.buf.truncate(start);
                Err(match e {
                    lz4_flex::block::DecompressError::OutputTooSmall { .. } => DecodeError::TooLong,
                    _ => DecodeError::Corrupt,
                })
            }
        }
    }

    /// Starts the history over, at the point the encoder did.
    pub fn reset(&mut self) {
        self.buf.clear();
    }
}
