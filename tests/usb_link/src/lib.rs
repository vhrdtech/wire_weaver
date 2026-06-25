#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Receiver, Sender, channel};
    use wire_weaver_usb_link::{PacketSink, PacketSource, WireWeaverUsbLink};
    use ww_version::{
        CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version, VersionOwned,
    };

    const USER_API_SIGNATURE: &'static [u8] = &[1, 2, 3, 4, 5, 6, 7, 8];

    struct VirtualUsb {
        tx: Sender<Vec<u8>>,
        rx: Receiver<Vec<u8>>,
    }

    impl VirtualUsb {
        fn new() -> (Self, Self) {
            let (tx1, rx1) = channel();
            let (tx2, rx2) = channel();
            (Self { tx: tx1, rx: rx2 }, Self { tx: tx2, rx: rx1 })
        }
    }

    impl PacketSink for VirtualUsb {
        type Error = ();

        async fn write_packet(&mut self, data: &[u8]) -> Result<(), ()> {
            self.tx.send(data.to_vec()).unwrap();
            Ok(())
        }
    }

    impl PacketSource for VirtualUsb {
        type Error = ();

        async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, Self::Error> {
            let packet = self.rx.recv().unwrap();
            data.clone_from_slice(&packet);
            Ok(packet.len())
        }

        async fn wait_usb_connection(&mut self) {}
    }

    #[test]
    fn usb_link_setup() {
        let mut tx_buf = [0u8; 2048];
        let mut rx_buf = [0u8; 2048];
        let (tx, rx) = VirtualUsb::new();
        let device = WireWeaverUsbLink::new_device(
            FullVersion::new("test".into(), Version::new(0, 0, 0)),
            USER_API_SIGNATURE,
            CompactVersion::new(GlobalTypeId::new(0), 1, 1, 1),
            100,
            tx,
            &mut tx_buf,
            rx,
            &mut rx_buf,
        );

        let mut tx_buf = [0u8; 2048];
        let mut rx_buf = [0u8; 2048];
        let (tx, rx) = VirtualUsb::new();
        let host = WireWeaverUsbLink::new_host(
            FullVersionOwned::new("test".into(), VersionOwned::new(0, 0, 0)),
            tx,
            &mut tx_buf,
            rx,
            &mut rx_buf,
        );
    }
}
