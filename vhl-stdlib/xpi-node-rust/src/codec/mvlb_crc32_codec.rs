use tokio_util::codec::{Decoder, Encoder};
use bytes::{Buf, BufMut, BytesMut};
use std::{cmp, fmt};
use vhl_stdlib::serdes::{SerDesSize, SerializeBytes};
use xpi::error::XpiError;

/// Marked + CRC, variable length codec working with byte slice frames.
/// Start marker of 0x55, 0xaa is used to quickly find potential frame boundaries.
/// Alternative is not using marks at all, but decoding is more resource intensive then.
#[derive(Clone, Debug)]
pub struct MvlbCrc32Codec {
    max_length: usize,
}

impl MvlbCrc32Codec {
    pub fn new_with_max_length(max_length: usize) -> MvlbCrc32Codec {
        MvlbCrc32Codec {
            max_length
        }
    }
}

impl Decoder for MvlbCrc32Codec {
    type Item = Vec<u8>;
    type Error = XpiError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        todo!()
    }
}

impl Encoder<&[u8]> for MvlbCrc32Codec
{
    type Error = XpiError;

    fn encode(&mut self, item: &[u8], dst: &mut BytesMut) -> Result<(), Self::Error> {
        let item_len = item.len();
        let len_len = if item_len <= 127 {
            1
        } else if item_len <= 32_767 {
            2
        } else if item_len <= 8_388_607 {
            3
        } else {
            return Err(XpiError::OutOfBounds);
        };
        Ok(())
    }
}