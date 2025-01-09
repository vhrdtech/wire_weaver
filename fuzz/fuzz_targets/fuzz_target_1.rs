#![no_main]

use libfuzzer_sys::fuzz_target;
use worst_executor::block_on;
use shrink_wrap::BufReader;
use wire_weaver_usb_common::{FrameBuilder, FrameSink, CrcProvider};

struct VecSink {
    frames: Vec<Vec<u8>>,
}

impl VecSink {
    fn new() -> Self {
        Self {
            frames: Vec::new(),
        }
    }
}

impl PacketSink for VecSink {
    async fn write_frame(&mut self, data: &[u8]) {
        self.frames.push(data.to_vec());
    }
}

struct SoftCrc {
}

impl SoftCrc {
    fn new() -> Self {
        Self {
        }
    }
}

impl CrcProvider for SoftCrc {
    fn checksum(&mut self, data: &[u8]) -> u16 {
        const X25: crc::Crc<u16> = crc::Crc::<u16>::new(&crc::CRC_16_IBM_SDLC);
        X25.checksum(data)
    }
}

fuzz_target!(|data: &[u8]| {
    let mut buf = [0u8; 8];
    let mut builder = FrameBuilder::new(&mut buf, VecSink::new(), SoftCrc {});
    let mut rdr = BufReader::new(data);
    let mut packets = vec![];
    loop {
        if rdr.bytes_left() <= 1 {
            break;
        }
        let Ok(len) = rdr.read_u8() else {
            break;
        };
        let len = rdr.bytes_left().min(len as usize);
        let Ok(packet) = rdr.read_raw_slice(len) else {
            break;
        };
        packets.push(Vec::from(packet));
        block_on(builder.write_packet(packet));
    }
    block_on(builder.force_send());
    let (_, sink) = builder.deinit();
    // assert_eq!(sink.packets.len(), 1);
    // assert_eq!(sink.packets[0], vec![(Kind::MessageStartEnd as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]);
});
