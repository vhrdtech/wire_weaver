use super::Error;
use bytes::{Buf, BufMut, BytesMut};
use std::{cmp, fmt};
use tokio_util::codec::{Decoder, Encoder};
use vhl_stdlib::serdes::{SerDesSize, SerializeBytes};

/// Marked, variable length codec working with byte slice frames.
/// To be used only with lossless underlying transports like TCP.
///
/// Start marker of 0x55, 0xaa is used to quickly find potential frame boundaries.
/// CRC16 of the length only is added as well, to potentially guard against reading garbage if started
/// not at the frame boundary. Reopening a TCP socket on the same port was observed to produce this,
/// though this might need more checking.
/// Alternative is not using marks at all (only length and crc), but decoding is more resource intensive then.
#[derive(Clone, Debug)]
pub struct MvlbCodec {
    state: State,

    max_length: usize,

    discarded_current: usize,
    discarded_total: usize,
    error_threshold: usize,
}

enum State {
    /// Initial state or when found 0xaa, 0x55, but not yet received length and valid crc.
    /// When correct length and valid crc if found, transition into WaitingForFullFrame or return a
    /// whole frame right away if available. If received incorrect data transition into Discarding.
    /// Otherwise wait for more data.
    MaybeAtTheBoundary,
    /// Received incorrect length or incorrect crc or didn't started reading from a valid boundary.
    /// Continue discarding until marker is found, then transition into MaybeAtTheBoundary state.
    Discarding,
    /// Received correct frame header, but not all the data yet.
    WaitingForFullFrame {
        len: usize,
    },
}

impl MvlbCodec {
    pub fn new_with_max_length(max_length: usize, error_threshold: usize) -> MvlbCodec {
        MvlbCodec {
            state: State::MaybeAtTheBoundary,
            max_length,
            discarded_current: 0,
            discarded_total: 0,
            error_threshold,
        }
    }
}

impl Decoder for MvlbCodec {
    type Item = Vec<u8>;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            // Determine how far into the buffer we'll search for a [0xaa, 0x55] separator.
            let read_to = cmp::min(self.max_length.saturating_add(1), buf.len());
            let separator_offset = buf[self.next_index..read_to]
                .chunks_exact(2)
                .position(|halfw| halfw == &[0xaa, 0x55]);

            match (self.is_discarding, separator_offset) {
                (true, Some(offset)) => {}
                (true, None) => {}
                (false, Some(offset)) => {
                    // Found a potential frame boundary
                    if buf.
                }
                (false, None) if buf.len() > self.max_length => {}
                (false, None) => {}
            }
        }
    }
}

impl Encoder<Vec<u8>> for MvlbCodec {
    type Error = Error;

    fn encode(&mut self, item: Vec<u8>, buf: &mut BytesMut) -> Result<(), Self::Error> {
        let item_len = item.len();
        let (len_len, len_bytes) = if item_len <= 127 {
            // 0b0 + 7 bit size
            (1, [item_len as u8, 0, 0])
        } else if item_len <= 16_383 {
            // 0b1 + high 7 bits, 0b0 + low 7 bits
            (
                2,
                [
                    0x80 | ((item_len >> 7) as u8),
                    ((item_len & 0b0111_1111) as u8),
                    0,
                ],
            )
        } else if item_len <= 2_097_151 {
            // ... 21 bit size
            (
                3,
                [
                    0x80 | ((item_len >> 14) as u8),
                    0x80 | ((item_len >> 7) as u8),
                    (item_len & 0b0111_1111) as u8,
                ],
            )
        } else {
            return Err(Error::TooBig);
        };
        buf.reserve(2 + len_len + 2 + item_len);
        buf.put_slice(&[0xaa, 0x55]);
        buf.put_slice(&len_bytes[..len_len]);

        let mut crc16 = crc_any::CRCu16::crc16ccitt_false();
        crc16.digest(&len_bytes[..len_len]);
        let crc16 = crc16.get_crc();

        buf.put_u16_le(crc16);
        buf.put_slice(&item);

        Ok(())
    }
}
