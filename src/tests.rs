#[cfg(test)]
#[macro_use]
extern crate std;

#[cfg(test)]
mod tests {
    use crate::*;
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

    impl PacketSink for VecSink {
        type Error = ();

        async fn write_packet(&mut self, data: &[u8]) -> Result<(), ()> {
            self.frames.push_back(data.to_vec());
            Ok(())
        }

        async fn wait_connection(&mut self) {}

        async fn rx_from_source(&mut self) -> LinkMgmtCmd {
            unimplemented!()
        }

        fn try_rx_from_source(&mut self) -> Option<LinkMgmtCmd> {
            None
        }
    }

    impl PacketSource for VecSink {
        type Error = ();

        fn read_packet(&mut self, data: &mut [u8]) -> impl Future<Output = Result<usize, ()>> {
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

    fn create_frame_builder(buf: &mut [u8]) -> MessageSender<VecSink> {
        MessageSender::new(
            buf,
            VecSink::new(),
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
        block_on(builder.send_message(&[1, 2, 3])).unwrap();
        let (_, sink) = builder.deinit();
        // 3 bytes still remain in the buffer, unless force_send() is called, packet will not be sent
        assert_eq!(sink.frames.len(), 0);
    }

    #[test]
    fn message_fits_fully() {
        let mut buf = [0u8; 8];
        let mut builder = create_frame_builder(&mut buf);
        block_on(builder.send_message(&[1, 2, 3, 4, 5, 6])).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 1);
        assert_eq!(
            sink.frames[0],
            vec![(Kind::PacketStartEnd as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );

        let mut staging = [0u8; 8];
        let mut receive = [0u8; 8];
        let mut reader = MessageReceiver::new(
            sink,
            &mut staging,
            ProtocolInfo {
                protocol_id: 0,
                major_version: 0,
                minor_version: 0,
            },
        );
        let len = block_on(reader.receive_message(&mut receive)).unwrap();
        let MessageKind::Data(len) = len else {
            panic!("Expected data packet");
        };
        assert_eq!(&receive[..len], &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn split_into_two() {
        let mut buf = [0u8; 8];
        let mut builder = create_frame_builder(&mut buf);
        block_on(builder.send_message(&[1, 2, 3, 4, 5, 6, 7, 8])).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 2);
        assert_eq!(
            sink.frames[0],
            vec![(Kind::PacketStart as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );
        let crc = CRC_KIND.checksum(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            sink.frames[1],
            vec![
                (Kind::PacketEnd as u8) << 4,
                0x02,
                7,
                8,
                (crc & 0xFF) as u8,
                (crc >> 8) as u8
            ]
        );

        let mut staging = [0u8; 8];
        let mut receive = [0u8; 8];
        let mut reader = MessageReceiver::new(
            sink,
            &mut staging,
            ProtocolInfo {
                protocol_id: 0,
                major_version: 0,
                minor_version: 0,
            },
        );
        let len = block_on(reader.receive_message(&mut receive)).unwrap();
        let MessageKind::Data(len) = len else {
            panic!("Expected data packet");
        };
        assert_eq!(&receive[..len], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn split_into_three() {
        let mut buf = [0u8; 8];
        let mut builder = create_frame_builder(&mut buf);
        const PACKET: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
        block_on(builder.send_message(PACKET)).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 3);
        assert_eq!(
            sink.frames[0],
            vec![(Kind::PacketStart as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            sink.frames[1],
            vec![(Kind::PacketContinue as u8) << 4, 0x06, 7, 8, 9, 10, 11, 12]
        );
        let crc = CRC_KIND.checksum(PACKET);
        assert_eq!(
            sink.frames[2],
            vec![
                (Kind::PacketEnd as u8) << 4,
                0x02,
                13,
                14,
                (crc & 0xFF) as u8,
                (crc >> 8) as u8
            ]
        );

        let mut staging = [0u8; 16];
        let mut receive = [0u8; 16];
        let mut reader = MessageReceiver::new(
            sink,
            &mut staging,
            ProtocolInfo {
                protocol_id: 0,
                major_version: 0,
                minor_version: 0,
            },
        );
        let len = block_on(reader.receive_message(&mut receive)).unwrap();
        let MessageKind::Data(len) = len else {
            panic!("Expected data packet");
        };
        assert_eq!(
            &receive[..len],
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]
        );
    }

    #[test]
    fn left_3_write_4() {
        let mut buf = [0u8; 8];
        let mut builder = create_frame_builder(&mut buf);
        block_on(builder.send_message(&[1, 2, 3])).unwrap();
        // 3 bytes still remain in the buffer
        block_on(builder.send_message(&[4, 5, 6, 7])).unwrap();
        block_on(builder.force_send()).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 2);
        assert_eq!(
            sink.frames[0],
            vec![
                (Kind::PacketStartEnd as u8) << 4,
                0x03,
                1,
                2,
                3,
                (Kind::PacketStart as u8) << 4,
                1,
                4
            ]
        );
        let crc = CRC_KIND.checksum(&[4, 5, 6, 7]);
        assert_eq!(
            sink.frames[1],
            vec![
                (Kind::PacketEnd as u8) << 4,
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
        block_on(builder.send_message(&[1, 2, 3])).unwrap();
        // 3 bytes still remain in the buffer
        block_on(builder.send_message(&[4, 5, 6, 7, 8, 9])).unwrap();
        block_on(builder.force_send()).unwrap();
        let (_, sink) = builder.deinit();
        assert_eq!(sink.frames.len(), 3);
        assert_eq!(
            sink.frames[0],
            vec![
                (Kind::PacketStartEnd as u8) << 4,
                0x03,
                1,
                2,
                3,
                (Kind::PacketStart as u8) << 4,
                1,
                4
            ]
        );
        let crc = CRC_KIND.checksum(&[4, 5, 6, 7, 8, 9]);
        assert_eq!(sink.frames[1].len(), 7);
        assert_eq!(
            sink.frames[1],
            vec![(Kind::PacketContinue as u8) << 4, 0x05, 5, 6, 7, 8, 9]
        );
        assert_eq!(sink.frames[2].len(), 4);
        assert_eq!(
            sink.frames[2],
            vec![
                (Kind::PacketEnd as u8) << 4,
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
