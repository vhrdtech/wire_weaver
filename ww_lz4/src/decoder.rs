use std::vec::Vec;

use crate::MAX_DISTANCE;

/// Host side of the stream: decodes the [`Encoder`](crate::Encoder)'s blocks in order, keeping the last 64 KiB it
/// decoded as the dictionary of the next block.
#[derive(Default)]
pub struct Decoder {
    /// History (at least the last [`MAX_DISTANCE`] bytes), then the block being decoded.
    buf: Vec<u8>,
}

/// A block that didn't decode: not valid LZ4, or reaching back beyond the history. The history is unchanged, but
/// the stream is out of step with the encoder: reset both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError;

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("corrupt LZ4 block")
    }
}

impl std::error::Error for DecodeError {}

/// What the history is trimmed to; the extra byte keeps it a round 64 KiB.
const WINDOW: usize = MAX_DISTANCE + 1;

impl Decoder {
    /// An empty decoder, its buffer sized for 128 KiB of history.
    pub fn new() -> Self {
        Decoder {
            buf: Vec::with_capacity(2 * WINDOW + 4096),
        }
    }

    /// Decodes the next block, returns its bytes.
    pub fn decode(&mut self, block: &[u8]) -> Result<&[u8], DecodeError> {
        let len = decoded_len(block).ok_or(DecodeError)?;
        // trim the history only once it is twice the window, so the copy is amortized
        if self.buf.len() > 2 * WINDOW {
            self.buf.drain(..self.buf.len() - WINDOW);
        }
        let start = self.buf.len();
        self.buf.resize(start + len, 0);
        let (history, out) = self.buf.split_at_mut(start);
        let dict = &history[start.saturating_sub(WINDOW)..];
        match lz4_flex::block::decompress_into_with_dict(block, out, dict) {
            Ok(n) if n == len => Ok(&self.buf[start..]),
            _ => {
                self.buf.truncate(start);
                Err(DecodeError)
            }
        }
    }

    /// Starts the history over, at the point the encoder did.
    pub fn reset(&mut self) {
        self.buf.clear();
    }
}

/// The length a block decodes to, from its tokens and length bytes alone; `None` when it ends inside a sequence.
fn decoded_len(block: &[u8]) -> Option<usize> {
    let mut i = 0;
    let mut n = 0usize;
    loop {
        let token = *block.get(i)?;
        i += 1;
        let lit = ext_len(block, &mut i, (token >> 4) as usize)?;
        i = i.checked_add(lit).filter(|&i| i <= block.len())?;
        n = n.checked_add(lit)?;
        if i == block.len() {
            return Some(n);
        }
        i += 2; // offset
        let m = ext_len(block, &mut i, (token & 15) as usize)?;
        n = n.checked_add(m)?.checked_add(4)?;
    }
}

/// A token's length nibble with its continuation bytes (when it is 15).
fn ext_len(block: &[u8], i: &mut usize, nibble: usize) -> Option<usize> {
    let mut len = nibble;
    if nibble == 15 {
        loop {
            let b = *block.get(*i)?;
            *i += 1;
            len = len.checked_add(b as usize)?;
            if b != 255 {
                break;
            }
        }
    }
    Some(len)
}
