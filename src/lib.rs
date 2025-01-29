#![no_std]
#![allow(async_fn_in_trait)]

#[cfg(test)]
#[macro_use]
extern crate std;
// #[cfg(test)]
// extern crate alloc;

use shrink_wrap::{BufReader, BufWriter};
use strum_macros::FromRepr;
use wire_weaver_derive::ww_repr;

#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, FromRepr)]
pub enum Kind {
    NoOp = 0,

    /// 0x1l, 0xll, `data[0..len]` in first packet
    MessageStart = 1,
    /// 0x2l, 0xll, `data[prev..prev+len]` at the start of next packet
    MessageContinue = 2,
    /// 0x3l, 0xll, `data[prev..prev+len]`, CRC (2 bytes) at the start of next packet
    MessageEnd = 3,
    /// 0x4l, 0xll, `data[0..len]` in one packet.
    MessageStartEnd = 4,

    GetMaxMessageLength = 5,
    MaxMessageLength = 6,

    TestModeSetup = 7,
    TestMessage = 8,
}

pub struct FrameBuilder<'i, S, C> {
    wr: BufWriter<'i>,
    sink: S,
    crc: C,
}

pub trait FrameSink {
    type Error;
    async fn write_frame(&mut self, data: &[u8]) -> Result<(), Self::Error>;
}

pub trait FrameSource {
    type Error;
    async fn read_frame(&mut self, data: &mut [u8]) -> Result<usize, Self::Error>;
}

pub trait CrcProvider {
    // fn reset(&mut self);
    // fn update(&mut self, data: &[u8]);
    // fn finalize(&mut self) -> u16;
    fn checksum(&mut self, data: &[u8]) -> u16;
}

impl<'i, S: FrameSink, C: CrcProvider> FrameBuilder<'i, S, C> {
    pub fn new(buf: &'i mut [u8], sink: S, crc: C) -> Self {
        debug_assert!(buf.len() >= 8);
        Self {
            wr: BufWriter::new(buf),
            sink,
            crc,
        }
    }

    /// Try to write provided message bytes into the current packet and return None if it fits.
    /// Otherwise, fill up current packet till the end and return Some(remaining bytes), which
    /// must be sent in next packets.
    pub async fn write_packet(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        if (bytes.len() + 2 <= self.wr.bytes_left()) && bytes.len() <= 4095 {
            // message fits fully
            self.write_message_start_end(bytes);
            // need at least 3 bytes for next message
            if self.wr.bytes_left() < 3 {
                self.force_send().await?;
            }
        } else {
            let mut remaining_bytes = bytes;
            let mut crc_in_next_packet = false;
            let mut is_first_chunk = true;
            while remaining_bytes.len() > 0 {
                if self.wr.bytes_left() < 3 {
                    self.force_send().await?;
                }
                let len_chunk = remaining_bytes
                    .len()
                    .min(self.wr.bytes_left() - 2)
                    .min(4095);
                let kind = if is_first_chunk {
                    is_first_chunk = false;
                    Kind::MessageStart
                } else if remaining_bytes.len() - len_chunk > 0 {
                    Kind::MessageContinue
                } else {
                    if self.wr.bytes_left() - len_chunk - 2 >= 2 {
                        // CRC will fit
                        Kind::MessageEnd
                    } else {
                        // CRC in the next packet with 0 remaining bytes of the message
                        crc_in_next_packet = true;
                        Kind::MessageContinue
                    }
                };
                self.wr.write_u4(kind as u8).unwrap();
                self.write_len(len_chunk as u16);
                self.wr
                    .write_raw_slice(&remaining_bytes[..len_chunk])
                    .unwrap();
                remaining_bytes = &remaining_bytes[len_chunk..];
                if kind == Kind::MessageEnd {
                    // TODO: CRC
                    self.wr.write_u16(0xAACC).unwrap();
                }
            }
            if crc_in_next_packet {
                if self.wr.bytes_left() < 2 {
                    self.force_send().await?;
                }
                // TODO: CRC
                self.wr.write_u4(Kind::MessageEnd as u8).unwrap();
                self.write_len(0);
                self.wr.write_u16(0xAADD).unwrap();
            }
            if self.wr.bytes_left() < 3 {
                self.force_send().await?;
            }
        }
        Ok(())
    }

    fn write_message_start_end(&mut self, bytes: &[u8]) {
        self.wr.write_u4(Kind::MessageStartEnd as u8).unwrap();
        self.write_len(bytes.len() as u16);
        self.wr.write_raw_slice(bytes).unwrap();
    }

    fn write_len(&mut self, len: u16) {
        let len11_8 = (len >> 8) as u8;
        let len7_0 = (len & 0xFF) as u8;
        self.wr.write_u4(len11_8).unwrap();
        self.wr.write_u8(len7_0).unwrap();
    }

    pub fn test_link(&mut self) {
        self.wr.write_u4(Kind::TestMessage as u8).unwrap();
    }

    pub async fn force_send(&mut self) -> Result<(), S::Error> {
        let data = self.wr.finish().unwrap();
        if data.len() > 0 {
            self.sink.write_frame(data).await?;
        }
        Ok(())
    }

    pub fn deinit(self) -> (&'i mut [u8], S) {
        (self.wr.deinit(), self.sink)
    }

    pub fn sink_mut<F: FnMut(&mut S)>(&mut self, f: F) {
        f(&mut self.sink);
    }
}

pub struct FrameReader<'a, S> {
    source: S,
    staging: &'a mut [u8],
    receive: &'a mut [u8],
    stats: Stats,
}

#[derive(Default, Debug, Copy, Clone)]
pub struct Stats {
    pub packets_received: u32,
    pub packets_lost: u32,
    pub malformed_bytes: u32,
}

impl<'a, S: FrameSource> FrameReader<'a, S> {
    pub fn new(frame_source: S, staging: &'a mut [u8], receive: &'a mut [u8]) -> Self {
        Self {
            source: frame_source,
            staging,
            receive,
            stats: Stats::default(),
        }
    }

    pub async fn read_packet<F: FnMut(&[u8])>(&mut self, mut f: F) -> Result<(), S::Error> {
        let mut staging_idx = 0;
        let mut in_fragmented_packet = false;
        'next_frame: loop {
            let len = self.source.read_frame(&mut self.receive).await?;
            if len == 0 {
                break Ok(());
            }
            let frame = &self.receive[..len];
            let mut rd = BufReader::new(frame);
            while rd.bytes_left() >= 2 {
                let kind = rd.read_u4().unwrap();
                let Some(kind) = Kind::from_repr(kind) else {
                    self.stats.malformed_bytes += 1;
                    continue 'next_frame;
                };
                let len11_8 = rd.read_u4().unwrap();
                let len7_0 = rd.read_u8().unwrap();
                let len = (len11_8 as usize) << 8 | len7_0 as usize;
                match kind {
                    Kind::NoOp => {}
                    Kind::MessageStart | Kind::MessageContinue | Kind::MessageEnd => {
                        let Ok(packet_piece) = rd.read_raw_slice(len) else {
                            self.stats.packets_lost += 1;
                            staging_idx = 0;
                            in_fragmented_packet = false;
                            continue 'next_frame;
                        };
                        if kind == Kind::MessageStart {
                            in_fragmented_packet = true;
                            staging_idx = 0;
                        } else if !in_fragmented_packet {
                            self.stats.packets_lost += 1;
                            if kind == Kind::MessageEnd {
                                if let Ok(_crc) = rd.read_u16() {
                                    continue;
                                } else {
                                    continue 'next_frame;
                                }
                            } else {
                                continue;
                            }
                        }
                        let staging_bytes_left = self.staging.len() - staging_idx;
                        if packet_piece.len() <= staging_bytes_left {
                            self.staging[staging_idx..(staging_idx + packet_piece.len())]
                                .copy_from_slice(packet_piece);
                            staging_idx += packet_piece.len();
                            if kind == Kind::MessageEnd {
                                let Ok(_crc) = rd.read_u16() else {
                                    self.stats.packets_lost += 1;
                                    staging_idx = 0;
                                    continue 'next_frame;
                                };
                                if _crc == 0xAACC || _crc == 0xAADD {
                                    f(&self.staging[..staging_idx]);
                                    in_fragmented_packet = false;
                                } else {
                                    self.stats.packets_lost += 1;
                                    staging_idx = 0;
                                    continue; // try to receive other packets if any, previous frames might be lost leading to crc error
                                }
                            }
                        } else {
                            staging_idx = 0;
                            self.stats.packets_lost += 1;
                            in_fragmented_packet = false;
                            continue 'next_frame;
                        }
                    }
                    Kind::MessageStartEnd => {
                        if let Ok(packet) = rd.read_raw_slice(len) {
                            f(packet);
                        } else {
                            self.stats.packets_lost += 1;
                            staging_idx = 0;
                            in_fragmented_packet = false;
                            continue 'next_frame;
                        }
                    }
                    Kind::GetMaxMessageLength => {}
                    Kind::MaxMessageLength => {}
                    Kind::TestModeSetup => {}
                    Kind::TestMessage => {}
                }
            }
        }
    }

    pub fn source_mut<F: FnMut(&mut S)>(&mut self, f: F) {
        f(&mut self.source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::future::{ready, Future};
    use std::collections::VecDeque;
    use std::vec::Vec;
    use worst_executor::block_on;

    struct VecSink {
        frames: VecDeque<Vec<u8>>,
    }

    impl VecSink {
        fn new() -> Self {
            Self {
                frames: VecDeque::new(),
            }
        }
    }

    impl FrameSink for VecSink {
        type Error = ();

        async fn write_frame(&mut self, data: &[u8]) -> Result<(), ()> {
            self.frames.push_back(data.to_vec());
            Ok(())
        }
    }

    impl FrameSource for VecSink {
        type Error = ();

        fn read_frame(&mut self, data: &mut [u8]) -> impl Future<Output = Result<usize, ()>> {
            if let Some(frame) = self.frames.pop_front() {
                data[..frame.len()].copy_from_slice(frame.as_slice());
                ready(Ok(frame.len()))
            } else {
                ready(Ok(0))
            }
        }
    }

    struct SoftCrc {}

    impl CrcProvider for SoftCrc {
        fn checksum(&mut self, data: &[u8]) -> u16 {
            const X25: crc::Crc<u16> = crc::Crc::<u16>::new(&crc::CRC_16_IBM_SDLC);
            X25.checksum(data)
        }
    }

    #[test]
    fn packet_not_sent_automatically() {
        let mut buf = [0u8; 8];
        let mut builder = FrameBuilder::new(&mut buf, VecSink::new(), SoftCrc {});
        block_on(builder.write_packet(&[1, 2, 3])).unwrap();
        let (_, sink) = builder.deinit();
        // 3 bytes still remain in the buffer, unless force_send() is called, packet will not be sent
        assert_eq!(sink.frames.len(), 0);
    }

    #[test]
    fn message_fits_fully() {
        let mut buf = [0u8; 8];
        let mut builder = FrameBuilder::new(&mut buf, VecSink::new(), SoftCrc {});
        block_on(builder.write_packet(&[1, 2, 3, 4, 5, 6])).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 1);
        assert_eq!(
            sink.frames[0],
            vec![(Kind::MessageStartEnd as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );

        let mut staging = [0u8; 8];
        let mut receive = [0u8; 8];
        let mut reader = FrameReader::new(sink, &mut staging, &mut receive);
        block_on(reader.read_packet(|data| {
            assert_eq!(data, &[1, 2, 3, 4, 5, 6]);
        }))
        .unwrap();
    }

    #[test]
    fn split_into_two() {
        let mut buf = [0u8; 8];
        let mut builder = FrameBuilder::new(&mut buf, VecSink::new(), SoftCrc {});
        block_on(builder.write_packet(&[1, 2, 3, 4, 5, 6, 7, 8])).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 2);
        assert_eq!(
            sink.frames[0],
            vec![(Kind::MessageStart as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );
        let crc = 0xaacc_u16;
        assert_eq!(
            sink.frames[1],
            vec![
                (Kind::MessageEnd as u8) << 4,
                0x02,
                7,
                8,
                (crc & 0xFF) as u8,
                (crc >> 8) as u8
            ]
        );

        let mut staging = [0u8; 8];
        let mut receive = [0u8; 8];
        let mut reader = FrameReader::new(sink, &mut staging, &mut receive);
        block_on(reader.read_packet(|data| {
            assert_eq!(data, &[1, 2, 3, 4, 5, 6, 7, 8]);
        }))
        .unwrap();
    }

    #[test]
    fn split_into_three() {
        let mut buf = [0u8; 8];
        let mut builder = FrameBuilder::new(&mut buf, VecSink::new(), SoftCrc {});
        block_on(builder.write_packet(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14])).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 3);
        assert_eq!(
            sink.frames[0],
            vec![(Kind::MessageStart as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            sink.frames[1],
            vec![
                (Kind::MessageContinue as u8) << 4,
                0x06,
                7,
                8,
                9,
                10,
                11,
                12
            ]
        );
        let crc = 0xaacc_u16;
        assert_eq!(
            sink.frames[2],
            vec![
                (Kind::MessageEnd as u8) << 4,
                0x02,
                13,
                14,
                (crc & 0xFF) as u8,
                (crc >> 8) as u8
            ]
        );

        let mut staging = [0u8; 16];
        let mut receive = [0u8; 8];
        let mut reader = FrameReader::new(sink, &mut staging, &mut receive);
        block_on(reader.read_packet(|data| {
            assert_eq!(data, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]);
        }))
        .unwrap();
    }

    #[test]
    fn left_3_write_4() {
        let mut buf = [0u8; 8];
        let mut builder = FrameBuilder::new(&mut buf, VecSink::new(), SoftCrc {});
        block_on(builder.write_packet(&[1, 2, 3])).unwrap();
        // 3 bytes still remain in the buffer
        block_on(builder.write_packet(&[4, 5, 6, 7])).unwrap();
        block_on(builder.force_send()).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 2);
        assert_eq!(
            sink.frames[0],
            vec![
                (Kind::MessageStartEnd as u8) << 4,
                0x03,
                1,
                2,
                3,
                (Kind::MessageStart as u8) << 4,
                1,
                4
            ]
        );
        let crc = 0xaacc_u16;
        assert_eq!(
            sink.frames[1],
            vec![
                (Kind::MessageEnd as u8) << 4,
                0x03,
                5,
                6,
                7,
                (crc & 0xFF) as u8,
                (crc >> 8) as u8
            ]
        );
    }

    #[test]
    fn left_3_write_6() {
        let mut buf = [0u8; 8];
        let mut builder = FrameBuilder::new(&mut buf, VecSink::new(), SoftCrc {});
        block_on(builder.write_packet(&[1, 2, 3])).unwrap();
        // 3 bytes still remain in the buffer
        block_on(builder.write_packet(&[4, 5, 6, 7, 8, 9])).unwrap();
        block_on(builder.force_send()).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 3);
        assert_eq!(
            sink.frames[0],
            vec![
                (Kind::MessageStartEnd as u8) << 4,
                0x03,
                1,
                2,
                3,
                (Kind::MessageStart as u8) << 4,
                1,
                4
            ]
        );
        let crc = 0xaadd_u16;
        assert_eq!(sink.frames[1].len(), 7);
        assert_eq!(
            sink.frames[1],
            vec![(Kind::MessageContinue as u8) << 4, 0x05, 5, 6, 7, 8, 9]
        );
        assert_eq!(sink.frames[2].len(), 4);
        assert_eq!(
            sink.frames[2],
            vec![
                (Kind::MessageEnd as u8) << 4,
                0x00,
                (crc & 0xFF) as u8,
                (crc >> 8) as u8
            ]
        );
    }
}
