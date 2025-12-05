#[cfg(test)]
mod link_tests {
    use crate::common::Op;
    use crate::*;
    use core::future::{Future, ready};
    use std::collections::VecDeque;
    use std::pin::pin;
    use std::ptr::null;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    use std::vec::Vec;
    use wire_weaver::prelude::FullVersion;
    use wire_weaver::ww_version::Version;
    use worst_executor::block_on;

    struct VecSink {
        packets: VecDeque<Vec<u8>>,
    }

    impl VecSink {
        fn new() -> Self {
            Self {
                packets: VecDeque::new(),
            }
        }
    }

    impl PacketSink for VecSink {
        type Error = ();

        async fn write_packet(&mut self, data: &[u8]) -> Result<(), ()> {
            self.packets.push_back(data.to_vec());
            Ok(())
        }
    }

    impl PacketSource for VecSink {
        type Error = ();

        fn read_packet(&mut self, data: &mut [u8]) -> impl Future<Output = Result<usize, ()>> {
            if let Some(frame) = self.packets.pop_front() {
                data[..frame.len()].copy_from_slice(frame.as_slice());
                println!("read out {}: {:02x?}", frame.len(), frame);
                ready(Ok(frame.len()))
            } else {
                println!("read out empty");
                ready(Ok(0))
            }
        }

        // async fn wait_usb_connection(&mut self) {}
    }

    fn create_link<'i>(
        tx_buf: &'i mut [u8],
        rx_buf: &'i mut [u8],
    ) -> WireWeaverUsbLink<'i, VecSink, VecSink> {
        WireWeaverUsbLink::new(
            FullVersion::new("test", Version::new(0, 0, 0)),
            VecSink::new(),
            tx_buf,
            VecSink::new(),
            rx_buf,
        )
    }

    #[test]
    fn packet_not_sent_automatically() {
        let mut tx_buf = [0u8; 8];
        let mut rx_buf = [0u8; 8];
        let mut link = create_link(&mut tx_buf, &mut rx_buf);
        block_on(link.send_message(&[1, 2, 3])).unwrap();
        let (_, tx, _rx) = link.de_init();
        // 3 bytes still remain in the buffer, unless force_send() is called, packet will not be sent
        assert_eq!(tx.packets.len(), 0);
    }

    #[test]
    fn message_fits_fully() {
        let mut tx_buf = [0u8; 8];
        let mut rx_buf = [0u8; 8];
        let mut link = create_link(&mut tx_buf, &mut rx_buf);
        block_on(link.send_message(&[1, 2, 3, 4, 5, 6])).unwrap();
        let (_, tx, _rx) = link.de_init();
        assert_eq!(tx.packets.len(), 1);
        assert_eq!(
            tx.packets[0],
            vec![(Op::MessageStartEnd as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );

        let mut receive = [0u8; 8];
        let mut link = WireWeaverUsbLink::new(
            FullVersion::new("test", Version::new(0, 0, 0)),
            VecSink::new(),
            &mut tx_buf,
            tx,
            &mut rx_buf,
        );
        let len = block_on(link.receive_message(&mut receive)).unwrap();
        let MessageKind::Data(len) = len else {
            panic!("Expected data packet");
        };
        assert_eq!(&receive[..len], &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn split_into_two() {
        let mut tx_buf = [0u8; 8];
        let mut rx_buf = [0u8; 8];
        let mut link = create_link(&mut tx_buf, &mut rx_buf);
        block_on(link.send_message(&[1, 2, 3, 4, 5, 6, 7, 8])).unwrap();
        let (_, tx, _rx) = link.de_init();
        assert_eq!(tx.packets.len(), 2);
        assert_eq!(
            tx.packets[0],
            vec![(Op::MessageStart as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );
        let crc = CRC_KIND.checksum(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            tx.packets[1],
            vec![
                (Op::MessageEnd as u8) << 4,
                0x02,
                7,
                8,
                (crc & 0xFF) as u8,
                (crc >> 8) as u8
            ]
        );

        let mut receive = [0u8; 8];
        let mut link = WireWeaverUsbLink::new(
            FullVersion::new("test", Version::new(0, 0, 0)),
            VecSink::new(),
            &mut tx_buf,
            tx,
            &mut rx_buf,
        );
        let len = block_on(link.receive_message(&mut receive)).unwrap();
        let MessageKind::Data(len) = len else {
            panic!("Expected data packet");
        };
        assert_eq!(&receive[..len], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn split_into_three() {
        let mut tx_buf = [0u8; 8];
        let mut rx_buf = [0u8; 8];
        let mut link = create_link(&mut tx_buf, &mut rx_buf);
        const MESSAGE: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
        block_on(link.send_message(MESSAGE)).unwrap();
        let (_, tx, _rx) = link.de_init();
        assert_eq!(tx.packets.len(), 3);
        assert_eq!(
            tx.packets[0],
            vec![(Op::MessageStart as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            tx.packets[1],
            vec![(Op::MessageContinue as u8) << 4, 0x06, 7, 8, 9, 10, 11, 12]
        );
        let crc = CRC_KIND.checksum(MESSAGE);
        assert_eq!(
            tx.packets[2],
            vec![
                (Op::MessageEnd as u8) << 4,
                0x02,
                13,
                14,
                (crc & 0xFF) as u8,
                (crc >> 8) as u8
            ]
        );

        let mut receive = [0u8; 14];
        let mut link = WireWeaverUsbLink::new(
            FullVersion::new("test", Version::new(0, 0, 0)),
            VecSink::new(),
            &mut tx_buf,
            tx,
            &mut rx_buf,
        );
        let len = block_on(link.receive_message(&mut receive)).unwrap();
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
        let mut tx_buf = [0u8; 8];
        let mut rx_buf = [0u8; 8];
        let mut link = create_link(&mut tx_buf, &mut rx_buf);
        block_on(link.send_message(&[1, 2, 3])).unwrap();
        // 3 bytes still remain in the buffer
        block_on(link.send_message(&[4, 5, 6, 7])).unwrap();
        block_on(link.force_send()).unwrap();
        let (_, tx, _rx) = link.de_init();
        assert_eq!(tx.packets.len(), 2);
        assert_eq!(
            tx.packets[0],
            vec![
                (Op::MessageStartEnd as u8) << 4,
                0x03,
                1,
                2,
                3,
                (Op::MessageStart as u8) << 4,
                1,
                4
            ]
        );
        let crc = CRC_KIND.checksum(&[4, 5, 6, 7]);
        assert_eq!(
            tx.packets[1],
            vec![
                (Op::MessageEnd as u8) << 4,
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
        let mut tx_buf = [0u8; 8];
        let mut rx_buf = [0u8; 8];
        let mut link = create_link(&mut tx_buf, &mut rx_buf);
        block_on(link.send_message(&[1, 2, 3])).unwrap();
        // 3 bytes still remain in the buffer
        block_on(link.send_message(&[4, 5, 6, 7, 8, 9])).unwrap();
        block_on(link.force_send()).unwrap();
        let (_, tx, _rx) = link.de_init();
        assert_eq!(tx.packets.len(), 3);
        assert_eq!(
            tx.packets[0],
            vec![
                (Op::MessageStartEnd as u8) << 4,
                0x03,
                1,
                2,
                3,
                (Op::MessageStart as u8) << 4,
                1,
                4
            ]
        );
        let crc = CRC_KIND.checksum(&[4, 5, 6, 7, 8, 9]);
        assert_eq!(tx.packets[1].len(), 7);
        assert_eq!(
            tx.packets[1],
            vec![(Op::MessageContinue as u8) << 4, 0x05, 5, 6, 7, 8, 9]
        );
        assert_eq!(tx.packets[2].len(), 4);
        assert_eq!(
            tx.packets[2],
            vec![
                (Op::MessageEnd as u8) << 4,
                0x00,
                (crc & 0xFF) as u8,
                (crc >> 8) as u8
            ]
        );
    }

    #[test]
    fn receive_is_cancel_safe() {
        let mut tx_buf = [0u8; 8];
        let mut rx_buf = [0u8; 8];
        let mut link = create_link(&mut tx_buf, &mut rx_buf);
        const MESSAGE: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
        block_on(link.send_message(MESSAGE)).unwrap();
        let (_, mut tx_all, _rx) = link.de_init();

        struct AsyncPacketSource {
            rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
        }

        impl PacketSource for AsyncPacketSource {
            type Error = ();

            async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, Self::Error> {
                let bytes = self.rx.recv().await.unwrap();
                let len = bytes.len();
                data[..bytes.len()].copy_from_slice(&bytes);
                Ok(len)
            }
        }

        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let rx = AsyncPacketSource { rx };

        let mut link = WireWeaverUsbLink::new(
            FullVersion::new("test", Version::new(0, 0, 0)),
            VecSink::new(),
            &mut tx_buf,
            rx,
            &mut rx_buf,
        );

        static WAKER: Waker = {
            const RAW_WAKER: RawWaker = RawWaker::new(
                null(),
                &RawWakerVTable::new(|_| RAW_WAKER, |_| (), |_| (), |_| ()),
            );
            unsafe { Waker::from_raw(RAW_WAKER) }
        };

        let mut receive = [0u8; 14];
        {
            let fut = link.receive_message(&mut receive);
            let mut fut = pin!(fut);

            block_on(tx.send(tx_all.packets.pop_front().unwrap())).unwrap();
            assert!(matches!(
                fut.as_mut().poll(&mut Context::from_waker(&WAKER)),
                Poll::Pending
            ));

            block_on(tx.send(tx_all.packets.pop_front().unwrap())).unwrap();
            assert!(matches!(
                fut.as_mut().poll(&mut Context::from_waker(&WAKER)),
                Poll::Pending
            ));
        }
        let len = {
            let fut = link.receive_message(&mut receive);
            let mut fut = pin!(fut);

            block_on(tx.send(tx_all.packets.pop_front().unwrap())).unwrap();
            let Poll::Ready(kind) = fut.as_mut().poll(&mut Context::from_waker(&WAKER)) else {
                panic!("expected ready")
            };

            let kind = kind.unwrap();
            let MessageKind::Data(len) = kind else {
                panic!("Expected data packet");
            };
            len
        };

        assert_eq!(
            &receive[..len],
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]
        );
    }
}
