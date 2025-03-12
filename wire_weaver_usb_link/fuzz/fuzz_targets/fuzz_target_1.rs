#![no_main]

use libfuzzer_sys::fuzz_target;
use shrink_wrap::BufReader;
use std::collections::VecDeque;
use std::future::{ready, Future};
use wire_weaver_usb_common::{FrameBuilder, FrameReader, FrameSink, FrameSource};
use worst_executor::block_on;

const MAX_PACKET_SIZE: usize = 2048;

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
}

fuzz_target!(|data: &[u8]| {
    let mut buf = [0u8; 64];
    let mut builder = FrameBuilder::new(&mut buf, VecSink::new());
    let mut rdr = BufReader::new(data);
    let mut packets = vec![];
    loop {
        if rdr.bytes_left() <= 1 {
            break;
        }
        let Ok(len) = rdr.read_u16() else {
            break;
        };

        let len = (len as usize) % MAX_PACKET_SIZE;
        let len = if len == 0 { 8 } else { len };
        let Ok(packet) = rdr.read_raw_slice(len) else {
            break;
        };
        // println!("tx: {:?}", packet);
        packets.push(Vec::from(packet));
        block_on(builder.write_packet(packet));
    }
    block_on(builder.force_send());
    let (_, sink) = builder.deinit();

    let mut staging = [0u8; 64];
    let mut receive = [0u8; 2048];
    let mut reader = FrameReader::new(sink, &mut staging);
    for expected_packet in packets.iter() {
        let len = block_on(reader.read_packet(&mut receive)).unwrap();
        let packet = &receive[..len];
        // println!("rx: {:?}", packet);
        assert_eq!(packet, &expected_packet[..]);
    }
    // assert_eq!(sink.packets.len(), 1);
    // assert_eq!(sink.packets[0], vec![(Kind::MessageStartEnd as u8) << 4, 0x06, 1, 2, 3, 4, 5, 6]);
    // println!("\n");
});
