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

const CRC_KIND: crc::Crc<u16> = crc::Crc::<u16>::new(&crc::CRC_16_IBM_SDLC);

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

    GetLinkInfo = 5,
    LinkInfo = 6,

    TestModeSetup = 7,
    TestMessage = 8,
}

pub struct FrameBuilder<'i, S> {
    wr: BufWriter<'i>,
    sink: S,
    link_protocol: ProtocolInfo,
    user_protocol: ProtocolInfo,
    remote_protocols_matches: Option<u32>,
}

pub trait FrameSink {
    type Error;
    async fn write_frame(&mut self, data: &[u8]) -> Result<(), Self::Error>;
    async fn wait_connection(&mut self);
    fn rx_from_source(&mut self) -> Option<LinkMgmtCmd>;
}

pub trait FrameSource {
    type Error;
    async fn read_frame(&mut self, data: &mut [u8]) -> Result<usize, Self::Error>;
    async fn wait_connection(&mut self);
    fn send_to_sink(&mut self, msg: LinkMgmtCmd);
}

pub enum LinkMgmtCmd {
    SendLocalInfo {
        max_packet_size: u32,
    },
    RemoteInfoReceived {
        max_packet_size: u32,
        link_protocol: ProtocolInfo,
        user_protocol: ProtocolInfo,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProtocolInfo {
    protocol_id: u8,
    major_version: u8,
    minor_version: u8,
}

impl ProtocolInfo {
    fn write(&self, wr: &mut BufWriter) -> Result<(), shrink_wrap::Error> {
        wr.write_u8(self.protocol_id)?;
        wr.write_u8(self.major_version)?;
        wr.write_u8(self.minor_version)?;
        Ok(())
    }

    fn read(rd: &mut BufReader) -> Result<ProtocolInfo, shrink_wrap::Error> {
        Ok(ProtocolInfo {
            protocol_id: rd.read_u8()?,
            major_version: rd.read_u8()?,
            minor_version: rd.read_u8()?,
        })
    }

    fn is_compatible(&self, other: &ProtocolInfo) -> bool {
        if self.protocol_id != other.protocol_id {
            false
        } else {
            if self.major_version == 0 && other.major_version == 0 {
                self.minor_version == other.minor_version
            } else {
                // not comparing minor versions, protocols are supposed to be backwards and forwards compatible after 1.0
                self.major_version == other.major_version
            }
        }
    }
}

impl<'i, S: FrameSink> FrameBuilder<'i, S> {
    /// Create new FrameBuilder, buf needs to be of maximum size that sink can accept.
    /// Frames will be created to be as big as possible to minimize overhead.
    pub fn new(
        buf: &'i mut [u8],
        sink: S,
        link_protocol: ProtocolInfo,
        user_protocol: ProtocolInfo,
    ) -> Self {
        debug_assert!(buf.len() >= 8);
        Self {
            wr: BufWriter::new(buf),
            sink,
            link_protocol,
            user_protocol,
            #[cfg(not(test))]
            remote_protocols_matches: None,
            #[cfg(test)]
            remote_protocols_matches: Some(4095),
        }
    }

    /// Try to write provided message bytes into the current packet and return None if it fits.
    /// Otherwise, fill up current packet till the end and return Some(remaining bytes), which
    /// must be sent in next packets.
    pub async fn write_packet(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        if let Some(link_info) = self.sink.rx_from_source() {
            match link_info {
                LinkMgmtCmd::SendLocalInfo { max_packet_size } => {
                    if self.wr.bytes_left() < 2 + 4 + 3 + 3 {
                        self.force_send().await?;
                    }
                    self.wr.write_u4(Kind::LinkInfo as u8).unwrap();
                    self.write_len(10);
                    self.wr.write_u32(max_packet_size).unwrap();
                    self.link_protocol.write(&mut self.wr).unwrap();
                    self.user_protocol.write(&mut self.wr).unwrap();
                    self.force_send().await?;
                }
                LinkMgmtCmd::RemoteInfoReceived {
                    max_packet_size,
                    link_protocol,
                    user_protocol,
                } => {
                    if self.link_protocol.is_compatible(&link_protocol)
                        && self.user_protocol.is_compatible(&user_protocol)
                    {
                        self.remote_protocols_matches = Some(max_packet_size);
                    }
                }
            }
        }
        let Some(max_remote_packet_size) = self.remote_protocols_matches else {
            // remote end did not send its link info yet
            return Ok(()); // TODO: Count errors
        };
        if bytes.len() > max_remote_packet_size as usize {
            return Ok(()); // TODO: Count errors
        }
        if bytes.len() + 2 <= self.wr.bytes_left()
        /* && bytes.len() <= max_remote_packet_size*/
        {
            // message fits fully
            self.write_message_start_end(bytes);
            // need at least 3 bytes for next message
            if self.wr.bytes_left() < 3 {
                self.force_send().await?;
            }
        } else {
            let mut remaining_bytes = bytes;
            let mut crc_in_next_packet = None;
            let mut is_first_chunk = true;
            while remaining_bytes.len() > 0 {
                if self.wr.bytes_left() < 3 {
                    self.force_send().await?;
                }
                let len_chunk = remaining_bytes.len().min(self.wr.bytes_left() - 2);
                // .min(max_remote_packet_size);
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
                        let crc = CRC_KIND.checksum(bytes);
                        crc_in_next_packet = Some(crc);
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
                    let crc = CRC_KIND.checksum(bytes);
                    self.wr.write_u16(crc).unwrap();
                }
            }
            if let Some(crc) = crc_in_next_packet {
                if self.wr.bytes_left() < 2 {
                    self.force_send().await?;
                }
                // TODO: CRC
                self.wr.write_u4(Kind::MessageEnd as u8).unwrap();
                self.write_len(0);
                self.wr.write_u16(crc).unwrap();
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

    pub async fn wait_connection(&mut self) {
        self.sink.wait_connection().await;
    }
}

pub struct FrameReader<'a, S> {
    source: S,
    receive: &'a mut [u8],
    receive_start_pos: usize,
    receive_left_bytes: usize,
    stats: Stats,
    in_fragmented_packet: bool,
}

#[derive(Default, Debug, Copy, Clone)]
pub struct Stats {
    pub packets_received: u32,
    pub packets_lost: u32,
    pub malformed_bytes: u32,
}

impl<'a, S: FrameSource> FrameReader<'a, S> {
    pub fn new(frame_source: S, receive: &'a mut [u8]) -> Self {
        Self {
            source: frame_source,
            receive,
            receive_start_pos: 0,
            receive_left_bytes: 0,
            stats: Stats::default(),
            in_fragmented_packet: false,
        }
    }

    pub async fn read_packet(&mut self, packet: &mut [u8]) -> Result<usize, S::Error> {
        let mut staging_idx = 0;
        'next_frame: loop {
            let (frame, is_new_frame) = if self.receive_left_bytes > 0 {
                (
                    &self.receive
                        [self.receive_start_pos..self.receive_start_pos + self.receive_left_bytes],
                    false,
                )
            } else {
                let len = self.source.read_frame(&mut self.receive).await?;
                if len == 0 {
                    break Ok(0); // TODO: is it correct to return Ok(0)?
                }
                (&self.receive[..len], true)
            };
            // println!("rx frame: {:?}", frame);
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
                            self.in_fragmented_packet = false;
                            continue 'next_frame;
                        };
                        if kind == Kind::MessageStart {
                            self.in_fragmented_packet = true;
                            staging_idx = 0;
                        } else if !self.in_fragmented_packet {
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
                        let staging_bytes_left = packet.len() - staging_idx;
                        if packet_piece.len() <= staging_bytes_left {
                            packet[staging_idx..(staging_idx + packet_piece.len())]
                                .copy_from_slice(packet_piece);
                            staging_idx += packet_piece.len();
                            if kind == Kind::MessageEnd {
                                let Ok(crc_received) = rd.read_u16() else {
                                    self.stats.packets_lost += 1;
                                    staging_idx = 0;
                                    continue 'next_frame;
                                };
                                let crc_calculated = CRC_KIND.checksum(&packet[..staging_idx]);
                                if crc_received == crc_calculated {
                                    self.in_fragmented_packet = false;

                                    let min_bytes_left = rd.bytes_left() >= 2;
                                    let read_bytes = frame.len() - rd.bytes_left();
                                    match (is_new_frame, min_bytes_left) {
                                        (true, true) => {
                                            self.receive_start_pos = read_bytes;
                                            self.receive_left_bytes = rd.bytes_left();
                                        }
                                        (false, true) => {
                                            self.receive_start_pos += read_bytes;
                                            self.receive_left_bytes -= read_bytes;
                                        }
                                        _ => {
                                            self.receive_start_pos = 0;
                                            self.receive_left_bytes = 0;
                                        }
                                    }
                                    return Ok(staging_idx);
                                } else {
                                    self.stats.packets_lost += 1;
                                    staging_idx = 0;
                                    continue; // try to receive other packets if any, previous frames might be lost leading to crc error
                                }
                            }
                        } else {
                            staging_idx = 0;
                            self.stats.packets_lost += 1;
                            self.in_fragmented_packet = false;
                            continue 'next_frame;
                        }
                    }
                    Kind::MessageStartEnd => {
                        if let Ok(packet_read) = rd.read_raw_slice(len) {
                            packet[..packet_read.len()].copy_from_slice(packet_read);

                            let min_bytes_left = rd.bytes_left() >= 2;
                            let read_bytes = frame.len() - rd.bytes_left();
                            match (is_new_frame, min_bytes_left) {
                                (true, true) => {
                                    self.receive_start_pos = read_bytes;
                                    self.receive_left_bytes = rd.bytes_left();
                                }
                                (false, true) => {
                                    self.receive_start_pos += read_bytes;
                                    self.receive_left_bytes -= read_bytes;
                                }
                                _ => {
                                    self.receive_start_pos = 0;
                                    self.receive_left_bytes = 0;
                                }
                            }
                            return Ok(packet_read.len());
                        } else {
                            self.stats.packets_lost += 1;
                            staging_idx = 0;
                            self.in_fragmented_packet = false;
                            continue 'next_frame;
                        }
                    }
                    Kind::GetLinkInfo => {
                        self.source.send_to_sink(LinkMgmtCmd::SendLocalInfo {
                            max_packet_size: packet.len() as u32,
                        });
                    }
                    Kind::LinkInfo => {
                        if rd.bytes_left() >= 4 + 3 + 3 {
                            let max_packet_size = rd.read_u32().unwrap();
                            let link_protocol = ProtocolInfo::read(&mut rd).unwrap();
                            let user_protocol = ProtocolInfo::read(&mut rd).unwrap();
                            self.source.send_to_sink(LinkMgmtCmd::RemoteInfoReceived {
                                max_packet_size,
                                link_protocol,
                                user_protocol,
                            });
                        }
                    }
                    Kind::TestModeSetup => {}
                    Kind::TestMessage => {}
                }
            }
            self.receive_left_bytes = 0;
        }
    }

    pub async fn wait_connection(&mut self) {
        self.source.wait_connection().await;
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

        async fn wait_connection(&mut self) {}

        fn rx_from_source(&mut self) -> Option<LinkMgmtCmd> {
            None
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

        async fn wait_connection(&mut self) {}

        fn send_to_sink(&mut self, _msg: LinkMgmtCmd) {}
    }

    fn create_frame_builder(buf: &mut [u8]) -> FrameBuilder<VecSink> {
        FrameBuilder::new(
            buf,
            VecSink::new(),
            ProtocolInfo {
                protocol_id: 0,
                major_version: 0,
                minor_version: 0,
            },
            ProtocolInfo {
                protocol_id: 0,
                major_version: 0,
                minor_version: 0,
            },
        )
    }

    #[test]
    fn packet_not_sent_automatically() {
        let mut buf = [0u8; 8];
        let mut builder = create_frame_builder(&mut buf);
        block_on(builder.write_packet(&[1, 2, 3])).unwrap();
        let (_, sink) = builder.deinit();
        // 3 bytes still remain in the buffer, unless force_send() is called, packet will not be sent
        assert_eq!(sink.frames.len(), 0);
    }

    #[test]
    fn message_fits_fully() {
        let mut buf = [0u8; 8];
        let mut builder = create_frame_builder(&mut buf);
        block_on(builder.write_packet(&[1, 2, 3, 4, 5, 6])).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 1);
        assert_eq!(
            sink.frames[0],
            vec![(Kind::MessageStartEnd as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );

        let mut staging = [0u8; 8];
        let mut receive = [0u8; 8];
        let mut reader = FrameReader::new(sink, &mut staging);
        let len = block_on(reader.read_packet(&mut receive)).unwrap();
        assert_eq!(&receive[..len], &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn split_into_two() {
        let mut buf = [0u8; 8];
        let mut builder = create_frame_builder(&mut buf);
        block_on(builder.write_packet(&[1, 2, 3, 4, 5, 6, 7, 8])).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 2);
        assert_eq!(
            sink.frames[0],
            vec![(Kind::MessageStart as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );
        let crc = CRC_KIND.checksum(&[1, 2, 3, 4, 5, 6, 7, 8]);
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
        let mut reader = FrameReader::new(sink, &mut staging);
        let len = block_on(reader.read_packet(&mut receive)).unwrap();
        assert_eq!(&receive[..len], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn split_into_three() {
        let mut buf = [0u8; 8];
        let mut builder = create_frame_builder(&mut buf);
        const PACKET: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
        block_on(builder.write_packet(PACKET)).unwrap();
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
        let crc = CRC_KIND.checksum(PACKET);
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
        let mut receive = [0u8; 16];
        let mut reader = FrameReader::new(sink, &mut staging);
        let len = block_on(reader.read_packet(&mut receive)).unwrap();
        assert_eq!(
            &receive[..len],
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]
        );
    }

    #[test]
    fn left_3_write_4() {
        let mut buf = [0u8; 8];
        let mut builder = create_frame_builder(&mut buf);
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
        let crc = CRC_KIND.checksum(&[4, 5, 6, 7]);
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
        let mut builder = create_frame_builder(&mut buf);
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
        let crc = CRC_KIND.checksum(&[4, 5, 6, 7, 8, 9]);
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

    // #[test]
    // fn adhoc() {
    //     let mut buf = [0u8; 64];
    //     let mut builder = FrameBuilder::new(&mut buf, VecSink::new());
    //     block_on(builder.write_packet(&[0, 0, 0, 0, 0, 0, 0, 0])).unwrap();
    //     block_on(builder.force_send()).unwrap();
    //     let (_, sink) = builder.deinit();
    //     println!("{}", sink.frames.len());
    //
    //     let mut staging = [0u8; 64];
    //     let mut receive = [0u8; 2048];
    //     let mut reader = FrameReader::new(sink, &mut staging);
    //     let len = block_on(reader.read_packet(&mut receive)).unwrap();
    //     assert_eq!(&receive[..len], &[0, 0, 0, 0, 0, 0, 0, 0]);
    // }
}
