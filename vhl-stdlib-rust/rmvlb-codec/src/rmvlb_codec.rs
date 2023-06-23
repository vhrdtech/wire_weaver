use super::Error;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::cmp;
use tokio_util::codec::{Decoder, Encoder};
use vhl_stdlib::serdes::buf::Error as VhlBufError;
use vhl_stdlib::serdes::vlu32b::Vlu32B;
use vhl_stdlib::serdes::Buf as VhlBuf;
use vhl_stdlib::serdes::BufMut as VhlBufMut;

/// Marked + reinforced, variable length codec working with byte slice frames.
/// To be used only with lossless underlying transports like TCP, as only frame headers are crc checked.
///
/// Wire format
/// ---
/// `[..., 0xaa, 0x55, len: Vlu32B (1-5B), crc16 of len bytes, frame bytes, ...]`
///
/// Start marker of 0x55, 0xaa is used to quickly find potential frame boundaries.
/// CRC16 of the length only is added as well, to potentially guard against reading garbage if started
/// not at the frame boundary. Reopening a TCP socket on the same port was observed to produce this,
/// though this might need more checking.
/// Alternative is not using marks at all (only length and crc), but decoding is more resource intensive then.
#[derive(Clone, Debug)]
pub struct RmvlbCodec {
    state: State,

    max_length: usize,

    discarded: usize,
    error_threshold: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum State {
    /// Initial state or when found 0xaa, 0x55, but not yet received length and valid crc.
    /// When marker is found, transition into WaitingForHeader.
    MaybeAtTheBoundary,
    /// Marker was found, now waiting for valid length
    WaitingForLength,
    /// Length was received, now waiting for it's crc to double check
    WaitingForCrc { len: usize, expected_crc: u16 },
    /// Received correct frame header, but not all the data yet.
    WaitingForFullFrame { len: usize },
}

impl RmvlbCodec {
    pub fn new_with_max_length(max_length: usize) -> RmvlbCodec {
        RmvlbCodec {
            state: State::MaybeAtTheBoundary,
            max_length,
            discarded: 0,
            error_threshold: max_length * 3,
        }
    }
}

impl Decoder for RmvlbCodec {
    type Item = Bytes;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            match self.state {
                State::MaybeAtTheBoundary => {
                    // Determine how far into the buffer we'll search for a [0xaa, 0x55] separator.
                    let read_to = cmp::min(self.max_length.saturating_add(1), buf.len());
                    let marker_offset = buf[..read_to]
                        .windows(2)
                        .position(|halfw| *halfw == [0xaa, 0x55]);
                    match marker_offset {
                        Some(offset) => {
                            self.state = State::WaitingForLength;
                            if offset == 0 {
                                // Found marker immediately, all good, on second iteration of the loop
                                // try to read header
                                buf.advance(2);
                            } else {
                                // Found marker, but after some unexpected garbage, skip it and record,
                                // if not reached error threshold, on second iteration of the loop try
                                // to read header
                                self.discarded += offset;
                                buf.advance(offset + 2);
                                if self.discarded >= self.error_threshold {
                                    return Err(Error::ErrorThresholdReached);
                                }
                            }
                        }
                        None => {
                            if buf.len() < 2 {
                                // Didn't found marker, but buffer wasn't even big enough yet
                                return Ok(None);
                            } else {
                                // Didn't found marker, but did found some garbage, skip it and record
                                self.discarded += buf.len();
                                buf.clear();
                                if self.discarded >= self.error_threshold {
                                    return Err(Error::ErrorThresholdReached);
                                }
                            }
                        }
                    }
                }
                State::WaitingForLength => {
                    if buf.is_empty() {
                        // Need at least one byte for length
                        return Ok(None);
                    }
                    // No more than 5 bytes for vlu32b
                    let read_to = cmp::min(5, buf.len());
                    let mut rd = VhlBuf::new(&buf[..read_to]);
                    let length = rd.get_vlu32b();
                    match length {
                        Ok(len) => {
                            if len as usize > self.max_length {
                                // incorrect length, start over and search for a new marker
                                self.state = State::MaybeAtTheBoundary;
                                continue;
                            }

                            let mut crc16 = crc_any::CRCu16::crc16ccitt_false();
                            crc16.digest(&buf[..rd.byte_pos()]);
                            let expected_crc = crc16.get_crc();

                            buf.advance(rd.byte_pos());
                            self.state = State::WaitingForCrc {
                                len: len as usize,
                                expected_crc,
                            };
                        }
                        Err(VhlBufError::OutOfBounds) => {
                            // not yet received all bytes to decode length
                            return Ok(None);
                        }
                        Err(VhlBufError::MalformedVlu32B) => {
                            // malformed vlu32b, 5th byte with bit 7 set, probably garbage
                            // try to find new marker
                            self.state = State::MaybeAtTheBoundary;
                        }
                        _ => unreachable!(),
                    }
                }
                State::WaitingForCrc { len, expected_crc } => {
                    if buf.len() < 2 {
                        // Need at least two bytes for crc
                        return Ok(None);
                    }
                    let crc = buf.get_u16_le();
                    if crc == expected_crc {
                        // now that we now the length, reserve needed capacity for
                        // frame data itself and next header of a maximum size
                        buf.reserve(len + 9);
                        self.state = State::WaitingForFullFrame { len };
                    } else {
                        // malformed header, probably garbage, try to find new marker
                        self.state = State::MaybeAtTheBoundary;
                    }
                }
                State::WaitingForFullFrame { len } => {
                    return if buf.len() >= len {
                        let frame = buf.split_to(len).freeze();
                        self.state = State::MaybeAtTheBoundary;
                        // On successful reception of a frame, lower discarded bytes amount, but at a slower pace than increasing it
                        self.discarded = self.discarded.saturating_sub(frame.len() / 10);
                        Ok(Some(frame))
                    } else {
                        // not enough data received yet for a full frame
                        return Ok(None);
                    };
                }
            }

            // only 1 iteration per run in test mode, so that state can be inspected with asserts.
            #[cfg(test)]
            return Ok(None);
        }
    }
}

impl Encoder<Bytes> for RmvlbCodec {
    type Error = Error;

    fn encode(&mut self, item: Bytes, buf: &mut BytesMut) -> Result<(), Self::Error> {
        let item_len = item.len();
        if item_len > self.max_length {
            return Err(Error::TooBig);
        }
        let item_len_vlu = Vlu32B(item_len as u32);
        let len_len = item_len_vlu.len_bytes_known_to_be_sized();

        buf.reserve(2 + len_len + 2 + item_len);
        buf.put_slice(&[0xaa, 0x55]);

        let mut wr_buf = [0u8; 5];
        let mut wr = VhlBufMut::new(&mut wr_buf);
        let _ = wr.put(&item_len_vlu);
        buf.put_slice(&wr_buf[..len_len]);

        let mut crc16 = crc_any::CRCu16::crc16ccitt_false();
        crc16.digest(&wr_buf[..len_len]);
        let crc16 = crc16.get_crc();
        buf.put_u16_le(crc16);

        buf.put_slice(&item);

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::{RmvlbCodec, State};
    use bytes::{Bytes, BytesMut};
    use tokio_util::codec::{Decoder, Encoder};

    #[test]
    fn encode_lt127() {
        let mut buf = BytesMut::with_capacity(512);
        let mut codec = RmvlbCodec::new_with_max_length(512);
        codec
            .encode(Bytes::from(&[1, 2, 3, 4, 5][..]), &mut buf)
            .unwrap();
        assert_eq!(buf.len(), 10);
        assert_eq!(&buf[..], [0xaa, 0x55, 5, 85, 177, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn encode_lt16_383() {
        let mut buf = BytesMut::with_capacity(2048);
        let mut codec = RmvlbCodec::new_with_max_length(1024);
        let frame: Vec<u8> = (0..1024).map(|v| (v % 255) as u8).collect();
        codec.encode(Bytes::from(frame), &mut buf).unwrap();
        assert_eq!(buf.len(), 6 + 1024);
        assert_eq!(&buf[..11], [0xaa, 0x55, 0x88, 0, 62, 143, 0, 1, 2, 3, 4]);
    }

    #[test]
    fn encode_lt2_097_151() {
        let mut buf = BytesMut::with_capacity(32768);
        let mut codec = RmvlbCodec::new_with_max_length(16384);
        let frame: Vec<u8> = (0..16384).map(|v| (v % 255) as u8).collect();
        codec.encode(Bytes::from(frame), &mut buf).unwrap();
        assert_eq!(buf.len(), 7 + 16384);
        assert_eq!(
            &buf[..12],
            [0xaa, 0x55, 0x81, 0x80, 0, 110, 219, 0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn decode_proper_start() {
        let mut buf = BytesMut::from(&[0xaa, 0x55, 5, 0x55, 0xb1, 1, 2, 3, 4, 5][..]);
        let mut codec = RmvlbCodec::new_with_max_length(16384);
        assert!(matches!(codec.decode(&mut buf), Ok(None)));
        assert_eq!(codec.state, State::WaitingForLength);
        assert!(matches!(codec.decode(&mut buf), Ok(None)));
        assert_eq!(
            codec.state,
            State::WaitingForCrc {
                len: 5,
                expected_crc: 0xb155,
            }
        );
        assert!(matches!(codec.decode(&mut buf), Ok(None)));
        assert_eq!(codec.state, State::WaitingForFullFrame { len: 5 });
        let frame = codec.decode(&mut buf).unwrap();
        assert_eq!(codec.state, State::MaybeAtTheBoundary);
        assert_eq!(frame, Some(Bytes::from(&[1, 2, 3, 4, 5][..])));
    }

    #[test]
    fn decode_garbage_start() {
        let mut buf = BytesMut::from(&[1, 2, 3, 0xaa, 0x55, 5, 85, 177, 1, 2, 3, 4, 5][..]);
        let mut codec = RmvlbCodec::new_with_max_length(16384);
        assert!(matches!(codec.decode(&mut buf), Ok(None)));
        assert_eq!(codec.state, State::WaitingForLength);
        assert_eq!(codec.discarded, 3);
        assert_eq!(buf.len(), 8);

        assert!(matches!(codec.decode(&mut buf), Ok(None)));
        assert!(matches!(codec.decode(&mut buf), Ok(None)));
        let frame = codec.decode(&mut buf).unwrap();
        assert_eq!(codec.state, State::MaybeAtTheBoundary);
        assert_eq!(frame, Some(Bytes::from(&[1, 2, 3, 4, 5][..])));
    }
}
