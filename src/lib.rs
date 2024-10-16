#![no_std]

use shrink_wrap::BufWriter;
use strum_macros::FromRepr;
use wire_weaver_derive::ww_repr;

#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, FromRepr)]
pub enum Kind {
    MessageStart = 0,
    MessageContinue = 1,
    MessageEnd = 2,
    MessageStartEnd = 3,

    GetMaxMessageLength = 4,
    MaxMessageLength = 5,

    TestModeSetup = 6,
    TestMessage = 7,
}

pub struct PacketBuilder<'i> {
    wr: BufWriter<'i>,
}

impl<'i> PacketBuilder<'i> {
    pub fn new(buf: &'i mut [u8]) -> Self {
        if buf.len() > 1024 {
            Self {
                wr: BufWriter::new(&mut buf[..1024])
            }
        } else {
            Self {
                wr: BufWriter::new(buf)
            }
        }
    }

    /// Try to write provided message bytes into the current packet and return None if it fits.
    /// Otherwise, fill up current packet till the end and return Some(remaining bytes), which
    /// must be sent in next packets.
    pub fn write_message<'m>(&mut self, bytes: &'m [u8]) -> Option<&'m [u8]> {
        let bytes_left = self.wr.bytes_left();
        if bytes_left <= 2 {
            return Some(bytes);
        }
        let msg_bytes_left = bytes_left - 2;
        if bytes.len() <= msg_bytes_left {
            self.wr.write_u4(Kind::MessageStartEnd as u8).unwrap();
            let len = bytes.len() as u16;
            let len11_8 = (len >> 8) as u8;
            let len7_0 = (len & 0xFF) as u8;
            self.wr.write_u4(len11_8).unwrap();
            self.wr.write_u8(len7_0).unwrap();
            self.wr.write_raw_slice(bytes).unwrap();
            None
        } else {
            unimplemented!();
            // Some(bytes)
        }
    }

    pub fn test_link(&mut self) {
        self.wr.write_u4(Kind::TestMessage as u8).unwrap();
    }

    pub fn finish(self) -> &'i [u8] {
        self.wr.finish().unwrap()
    }
}

pub struct PacketReader {
    // TODO
}