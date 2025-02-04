#![no_std]
#![allow(async_fn_in_trait)]

mod common;
mod receiver;
mod sender;
mod tests;

/// CRC used on packets that span multiple frames.
const CRC_KIND: crc::Crc<u16> = crc::Crc::<u16>::new(&crc::CRC_16_IBM_SDLC);

/// Link versions itself, checked to guard against interpreting incompatible data.
const LINK_PROTOCOL_VERSION: u8 = 1;

/// Minimum packet sized assumed to be supported before link setup is done and higher number is
/// potentially received.
const MIN_PACKET_SIZE: usize = 64;

// Some features are host and device specific to reduce confusion.
#[cfg(all(feature = "device", feature = "host"))]
compile_error!("Exactly one of 'device' or 'host' features must be enabled");

pub use common::{LinkMgmtCmd, PacketSink, PacketSource, ProtocolInfo};
pub use receiver::{MessageKind, MessageReceiver, ReceiveError, ReceiverStats};
pub use sender::{MessageSender, SendError, SenderStats};
