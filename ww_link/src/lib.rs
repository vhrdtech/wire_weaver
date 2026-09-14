//! WireWeaver link layer: message kinds and their payloads carried over [ww_framer] frames.
//!
//! This crate is sans-IO and `no_std`: it only knows how to encode a [Message] into framer
//! messages and how to decode them back. Sending frames, timers and connection state are up to
//! the host ([wire_weaver_client](https://docs.rs/wire_weaver_client)) or device event loops.
//!
//! Frame layout is defined by [ww_framer]: each frame holds one or more link messages, each
//! prefixed with a [U2Head] carrying [Kind] as `user_kind`. Data messages use the compact 2-bit
//! user kind form, all the rest use the extended one.
#![cfg_attr(not(feature = "std"), no_std)]

use shrink_wrap::prelude::*;
use ww_framer::crc::{Crc16IbmSdlc, CrcChecksum};
use ww_framer::framed::U2Head;
use ww_framer::traits::NopTail;
pub use ww_version::{ApiHashPair, CompactVersion, FullVersion};
#[cfg(feature = "std")]
pub use ww_version::{ApiHashPairOwned, FullVersionOwned};

pub use wire_weaver::DisconnectReason;

/// Framer head used by the link.
pub type Head = U2Head;
/// Split messages are protected with CRC-16 to detect lost frames, Full ones rely on the medium.
pub type Checksum = CrcChecksum<Crc16IbmSdlc>;
/// Frame based media (USB, CAN, etc.) do not need a delimiter.
pub type Tail = NopTail;

pub type Tx<'i> = ww_framer::Tx<'i, Head, Checksum, Tail>;
pub type Rx<'i> = ww_framer::FramedRx<'i, Head, Checksum, Tail>;
#[cfg(feature = "std")]
pub type TxOwned = ww_framer::TxOwned<Head, Checksum, Tail>;
#[cfg(feature = "std")]
pub type RxOwned = ww_framer::FramedRxOwned<Head, Checksum, Tail>;

/// How often to send [Kind::Ping] when there is no other traffic.
pub const PING_INTERVAL_MS: u64 = 3000;
/// Peer is considered gone if nothing at all was received for this long.
pub const PEER_TIMEOUT_MS: u64 = 10_000;

/// Message kinds, carried as [ww_framer] `user_kind`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Kind {
    /// ww_client_server bytes
    Data0 = 0,
    Data1 = 1,
    Data2 = 2,

    /// Empty message sent right before link setup and forced out immediately.
    /// If USB data toggle bits are messed up after re-connection, this ensures no useful packets are lost.
    Nop = 3,

    /// Sent from host to device to get device info.
    GetDeviceInfo = 4,
    /// Sent from device to host in response to GetDeviceInfo with [DeviceInfo] struct.
    DeviceInfo = 5,

    /// Sent from host to device with its link, client server, and user versions ([LinkSetup]).
    LinkSetup = 6,
    /// Sent from device to host to let it know that it received LinkSetup and that protocol version is compatible.
    /// Otherwise, Disconnect with DisconnectReason::IncompatibleVersion is sent.
    /// Guard against host starting to send before device received LinkSetup to avoid losing messages.
    LinkReady = 7,

    /// Sent periodically when there are no data messages from both host and device sides
    Ping = 8,

    /// Sent from host to device, if requested by user
    GetStats = 9,
    /// Sent in response to GetStats from device side
    Stats = 10,

    /// Used to test hardware and software stack by sending lots of data back and forth.
    /// Payload is two u32's in LE and then test data till the end of a message.
    /// | repeat | seq | data ... |
    ///
    /// repeat:
    /// * 0 - only count incoming packets, do not answer (used to measure host->device speed)
    /// * 1 and up - send one or more copies back (1 used to test the link integrity, more than 1 to test device-> host speed).
    Loopback = 11,

    /// Sent from host to device to let it know that driver or application is stopping.
    /// Sent from device to host to let it know that it is rebooting, e.g. to perform fw update.
    /// Payload is [DisconnectReason].
    Disconnect = 12,
}

impl Kind {
    pub fn from_repr(repr: u8) -> Option<Self> {
        Some(match repr {
            0 => Kind::Data0,
            1 => Kind::Data1,
            2 => Kind::Data2,
            3 => Kind::Nop,
            4 => Kind::GetDeviceInfo,
            5 => Kind::DeviceInfo,
            6 => Kind::LinkSetup,
            7 => Kind::LinkReady,
            8 => Kind::Ping,
            9 => Kind::GetStats,
            10 => Kind::Stats,
            11 => Kind::Loopback,
            12 => Kind::Disconnect,
            _ => return None,
        })
    }
}

/// Sent from device to host in response to [Kind::GetDeviceInfo].
#[derive_shrink_wrap]
#[derive(Debug, Clone)]
#[owned = "std"]
#[defmt = "defmt"]
pub struct DeviceInfo<'i> {
    /// This crate version on the device side
    pub dev_link_version: CompactVersion,
    /// E.g., ww_client_server version on the device side
    pub api_model_version: CompactVersion,
    /// User API and data types version on the device side
    pub user_api_version: FullVersion<'i>,
    /// First 8 bytes for SHA256 of ww_self bytes with and without doc comments
    pub hash: ApiHashPair<'i>,
    /// Maximum length message that device can process
    pub dev_max_message_len: u32,
    /// Configures host side to use the same value
    pub packet_accumulation_time_us: u16,
}

/// Sent from host to device after receiving [DeviceInfo].
#[derive_shrink_wrap]
#[derive(Debug, Clone)]
#[owned = "std"]
#[defmt = "defmt"]
pub struct LinkSetup<'i> {
    /// User API and data types version on the host side
    pub host_user_version: FullVersion<'i>,
    /// Maximum length message that host can process
    pub host_max_message_len: u32,
}

/// Decoded link message, see [Kind] for details on each.
#[derive(Debug)]
pub enum Message<'i> {
    /// `channel` is 0..=2 ([Kind::Data0] to [Kind::Data2])
    Data {
        channel: u8,
        bytes: &'i [u8],
    },
    Nop,
    GetDeviceInfo,
    DeviceInfo(DeviceInfo<'i>),
    LinkSetup(LinkSetup<'i>),
    LinkReady,
    Ping,
    GetStats,
    /// Not defined yet, raw bytes
    Stats(&'i [u8]),
    Loopback {
        repeat: u32,
        seq: u32,
        data: &'i [u8],
    },
    Disconnect(DisconnectReason),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// `user_kind` is not a known [Kind]
    UnknownKind(u8),
    /// Payload could not be deserialized, e.g. [DeviceInfo] from a device with a different link version
    Malformed(Kind),
    /// Scratch buffer provided to [Message::encode] is too small
    ScratchTooSmall,
}

impl<'i> Message<'i> {
    pub fn kind(&self) -> Kind {
        match self {
            Message::Data { channel, .. } => match channel {
                0 => Kind::Data0,
                1 => Kind::Data1,
                _ => Kind::Data2,
            },
            Message::Nop => Kind::Nop,
            Message::GetDeviceInfo => Kind::GetDeviceInfo,
            Message::DeviceInfo(_) => Kind::DeviceInfo,
            Message::LinkSetup(_) => Kind::LinkSetup,
            Message::LinkReady => Kind::LinkReady,
            Message::Ping => Kind::Ping,
            Message::GetStats => Kind::GetStats,
            Message::Stats(_) => Kind::Stats,
            Message::Loopback { .. } => Kind::Loopback,
            Message::Disconnect(_) => Kind::Disconnect,
        }
    }

    /// Decode a message returned by the framer.
    pub fn decode(user_kind: u8, payload: &'i [u8]) -> Result<Self, Error> {
        let kind = Kind::from_repr(user_kind).ok_or(Error::UnknownKind(user_kind))?;
        let mut rd = BufReader::new(payload);
        let malformed = |_| Error::Malformed(kind);
        Ok(match kind {
            Kind::Data0 | Kind::Data1 | Kind::Data2 => Message::Data {
                channel: user_kind,
                bytes: payload,
            },
            Kind::Nop => Message::Nop,
            Kind::GetDeviceInfo => Message::GetDeviceInfo,
            Kind::DeviceInfo => {
                Message::DeviceInfo(DeviceInfo::des_shrink_wrap(&mut rd).map_err(malformed)?)
            }
            Kind::LinkSetup => {
                Message::LinkSetup(LinkSetup::des_shrink_wrap(&mut rd).map_err(malformed)?)
            }
            Kind::LinkReady => Message::LinkReady,
            Kind::Ping => Message::Ping,
            Kind::GetStats => Message::GetStats,
            Kind::Stats => Message::Stats(payload),
            Kind::Loopback => {
                let repeat = rd.read_u32().map_err(malformed)?;
                let seq = rd.read_u32().map_err(malformed)?;
                let data = rd.read_raw_slice(rd.bytes_left()).map_err(malformed)?;
                Message::Loopback { repeat, seq, data }
            }
            Kind::Disconnect => {
                Message::Disconnect(DisconnectReason::des_shrink_wrap(&mut rd).map_err(malformed)?)
            }
        })
    }

    /// Encode this message into `(user_kind, payload)` to be passed to the framer's `write`.
    ///
    /// Payloads that are already bytes ([Message::Data], [Message::Stats]) are returned as is,
    /// others are serialized into `scratch`.
    pub fn encode<'s>(&self, scratch: &'s mut [u8]) -> Result<(u8, &'s [u8]), Error>
    where
        'i: 's,
    {
        let kind = self.kind() as u8;
        let mut wr = BufWriter::new(scratch);
        let too_small = |_| Error::ScratchTooSmall;
        match self {
            Message::Data { bytes, .. } => return Ok((kind, bytes)),
            Message::Stats(bytes) => return Ok((kind, bytes)),
            Message::Nop
            | Message::GetDeviceInfo
            | Message::LinkReady
            | Message::Ping
            | Message::GetStats => {}
            Message::DeviceInfo(info) => info.ser_shrink_wrap(&mut wr).map_err(too_small)?,
            Message::LinkSetup(setup) => setup.ser_shrink_wrap(&mut wr).map_err(too_small)?,
            Message::Loopback { repeat, seq, data } => {
                wr.write_u32(*repeat).map_err(too_small)?;
                wr.write_u32(*seq).map_err(too_small)?;
                wr.write_raw_slice(data).map_err(too_small)?;
            }
            Message::Disconnect(reason) => reason.ser_shrink_wrap(&mut wr).map_err(too_small)?,
        }
        Ok((kind, wr.finish_and_take().map_err(too_small)?))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    fn round_trip(msg: Message<'_>) {
        let mut scratch = [0u8; 128];
        let (kind, payload) = msg.encode(&mut scratch).unwrap();
        let payload = payload.to_vec();
        let decoded = Message::decode(kind, &payload).unwrap();
        assert_eq!(std::format!("{msg:?}"), std::format!("{decoded:?}"));
    }

    #[test]
    fn encode_decode() {
        round_trip(Message::Data {
            channel: 1,
            bytes: &[1, 2, 3],
        });
        round_trip(Message::Nop);
        round_trip(Message::GetDeviceInfo);
        round_trip(Message::DeviceInfo(DeviceInfo {
            dev_link_version: CompactVersion::new(ww_global::WIRE_WEAVER_USB_LINK, 1, 2, 3),
            api_model_version: CompactVersion::new(ww_global::WW_CLIENT_SERVER, 0, 2, 0),
            user_api_version: FullVersion::new("test", ww_version::Version::new(0, 1, 0)),
            hash: ApiHashPair::empty(),
            dev_max_message_len: 2048,
            packet_accumulation_time_us: 1000,
        }));
        round_trip(Message::LinkSetup(LinkSetup {
            host_user_version: FullVersion::new("test", ww_version::Version::new(0, 1, 0)),
            host_max_message_len: 4096,
        }));
        round_trip(Message::LinkReady);
        round_trip(Message::Ping);
        round_trip(Message::Loopback {
            repeat: 1,
            seq: 7,
            data: &[0xAA; 10],
        });
        round_trip(Message::Disconnect(DisconnectReason::RequestByUser));
    }

    #[test]
    fn unknown_kind() {
        assert_eq!(
            Message::decode(200, &[]).err(),
            Some(Error::UnknownKind(200))
        );
    }
}
