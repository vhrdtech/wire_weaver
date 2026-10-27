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
use ww_framer::crc::{CrcAlgorithm, CrcChecksum};
use ww_framer::framed::U2Head;
use ww_framer::prefix::{FramePrefix, NopPrefix, SeqPrefix};
use ww_framer::traits::{Checksum, Head, NopChecksum, NopTail, Tail};
pub use ww_version::{ApiHashPair, CompactVersion, FullVersion};
#[cfg(feature = "std")]
pub use ww_version::{ApiHashPairOwned, FullVersionOwned};

pub use wire_weaver::DisconnectReason;

/// Framer configuration of a frame based medium: both ends of a link must use the same one.
pub trait Framing {
    type Head: Head<UserKind = u8>;
    type Checksum: Checksum;
    type Tail: Tail;
    type Prefix: FramePrefix;
}

/// [ww_framer::Tx] of a [Framing].
pub type FramingTx<'a, F> = ww_framer::Tx<
    'a,
    <F as Framing>::Head,
    <F as Framing>::Checksum,
    <F as Framing>::Tail,
    <F as Framing>::Prefix,
>;
/// [ww_framer::FramedRx] of a [Framing].
pub type FramingRx<'a, F> = ww_framer::FramedRx<
    'a,
    <F as Framing>::Head,
    <F as Framing>::Checksum,
    <F as Framing>::Tail,
    <F as Framing>::Prefix,
>;
/// [ww_framer::TxOwned] of a [Framing].
#[cfg(feature = "std")]
pub type FramingTxOwned<F> = ww_framer::TxOwned<
    <F as Framing>::Head,
    <F as Framing>::Checksum,
    <F as Framing>::Tail,
    <F as Framing>::Prefix,
>;
/// [ww_framer::FramedRxOwned] of a [Framing].
#[cfg(feature = "std")]
pub type FramingRxOwned<F> = ww_framer::FramedRxOwned<
    <F as Framing>::Head,
    <F as Framing>::Checksum,
    <F as Framing>::Tail,
    <F as Framing>::Prefix,
>;

// Framer configuration for USB
/// Framer head used by the link.
pub type UsbHead = U2Head;
/// No checksum: USB checks every packet with its own CRC and retries, and bulk endpoints deliver in order without
/// losing packets within a session (a full host just NAKs). Frames left over from a previous session are told
/// apart by the link setup token, see [Message::GetDeviceInfo].
pub type UsbChecksum = NopChecksum;
/// Frame based media (USB, CAN, etc.) do not need a delimiter.
pub type UsbTail = NopTail;
/// No prefix by default, see [UsbSeqFraming] for a frame counter as a diagnostic.
pub type UsbPrefix = NopPrefix;

/// USB: one frame per packet, see [UsbHead], [UsbChecksum], [UsbTail], [UsbPrefix].
pub struct UsbFraming;

impl Framing for UsbFraming {
    type Head = UsbHead;
    type Checksum = UsbChecksum;
    type Tail = UsbTail;
    type Prefix = UsbPrefix;
}

/// [UsbFraming] with a [SeqPrefix] frame counter, as a diagnostic: counts lost, duplicated or reordered packets
/// (which a working USB stack never has). Off by default, host and device must both turn it on.
pub struct UsbSeqFraming;

impl Framing for UsbSeqFraming {
    type Head = UsbHead;
    type Checksum = UsbChecksum;
    type Tail = UsbTail;
    type Prefix = SeqPrefix;
}

/// USB interface protocol (`bInterfaceProtocol`, class 0xFF, subclass 0) of a WireWeaver interface with
/// [UsbFraming]: the host picks the framing from it.
pub const USB_INTERFACE_PROTOCOL: u8 = 0x37;
/// USB interface protocol of a WireWeaver interface with [UsbSeqFraming].
pub const USB_INTERFACE_PROTOCOL_SEQ: u8 = 0x38;

/// A framing a USB device can use, and the interface protocol that tells the host about it: [UsbFraming] or
/// [UsbSeqFraming].
pub trait UsbFramingProtocol: Framing {
    /// `bInterfaceProtocol` of the WireWeaver interface.
    const INTERFACE_PROTOCOL: u8;
}

impl UsbFramingProtocol for UsbFraming {
    const INTERFACE_PROTOCOL: u8 = USB_INTERFACE_PROTOCOL;
}

impl UsbFramingProtocol for UsbSeqFraming {
    const INTERFACE_PROTOCOL: u8 = USB_INTERFACE_PROTOCOL_SEQ;
}

// Framer configuration for RTT (SEGGER Real-Time Transfer over a debug probe)
/// RTT is a byte stream, so only `Full` messages are used (see `Tx::write_full` in [ww_framer]).
pub type RttHead = U2Head;
/// RTT channels are ring buffers in RAM read by the debug probe: nothing gets corrupted, and
/// `NoBlockSkip` mode writes each chunk atomically, so nothing is lost mid-message either.
pub type RttChecksum = NopChecksum;
/// No delimiter, see [RttChecksum].
pub type RttTail = NopTail;

// Framer configuration for UDP: one frame per datagram
/// Datagrams keep their boundaries, so messages are packed and split as over USB.
pub type UdpHead = U2Head;
/// CRC-32 on every message, with the frame prefix and the head folded in: datagrams get lost, duplicated or
/// reordered and nothing is retried, the UDP checksum is a 16-bit sum (optional over IPv4) and Ethernet's CRC only
/// covers each hop. shrink_wrap is dense, so a flipped bit often still parses: the CRC is the real guard.
pub type UdpChecksum = CrcChecksum<LinkCrc32, true>;
/// Datagrams do not need a delimiter.
pub type UdpTail = NopTail;
/// Frame counter and session epoch: lost, duplicated, reordered and previous-session datagrams are told apart.
pub type UdpPrefix = SeqPrefix;

/// UDP: one frame per datagram, see [UdpHead], [UdpChecksum], [UdpTail], [UdpPrefix].
pub struct UdpFraming;

impl Framing for UdpFraming {
    type Head = UdpHead;
    type Checksum = UdpChecksum;
    type Tail = UdpTail;
    type Prefix = UdpPrefix;
}

/// CRC-32/ISO-HDLC (zip, Ethernet) of [UdpChecksum], computed by `ww_block::crc32_update`: one implementation
/// for the link and block checksums, with the lookup table size a firmware picks once with `ww_block`'s
/// `crc-table-*` features (bitwise without one).
pub struct LinkCrc32;

impl CrcAlgorithm for LinkCrc32 {
    type Width = u32;

    #[inline]
    fn checksum(header: &[u8], message: &[u8]) -> u32 {
        !ww_block::crc32_update(ww_block::crc32_update(!0, header), message)
    }
}

/// Largest datagram payload the host sends: 1500 B Ethernet MTU - 40 B IPv6 header - 8 B UDP header, so that
/// datagrams are not fragmented over IPv4 or IPv6. A device must be able to receive datagrams of this size.
pub const UDP_MAX_DATAGRAM_LEN: usize = 1452;

/// Version of this crate, sent in [DeviceInfo::dev_link_version].
pub const LINK_VERSION: CompactVersion = CompactVersion::new(
    ww_global::WIRE_WEAVER_USB_LINK,
    parse_u32(env!("CARGO_PKG_VERSION_MAJOR")),
    parse_u32(env!("CARGO_PKG_VERSION_MINOR")),
    parse_u32(env!("CARGO_PKG_VERSION_PATCH")),
);

const fn parse_u32(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut i = 0;
    let mut n = 0u32;
    while i < b.len() {
        n = n * 10 + (b[i] - b'0') as u32;
        i += 1;
    }
    n
}

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
    /// Reserved for a later use, not free for applications: a device drops it today.
    Data1 = 1,
    /// Reserved for a later use, not free for applications: a device drops it today.
    Data2 = 2,

    /// Empty message sent right before link setup and forced out immediately.
    /// If USB data toggle bits are messed up after re-connection, this ensures no useful packets are lost.
    Nop = 3,

    /// Sent from host to device to get device info, with the host's [setup token](Message::GetDeviceInfo).
    /// Starts a new session on the device: its transmit framer drops whatever is left of the previous one.
    GetDeviceInfo = 4,
    /// Sent from device to host in response to GetDeviceInfo with [DeviceInfo] struct, echoing the token.
    DeviceInfo = 5,

    /// Sent from host to device with its link, client server, and user versions ([LinkSetup]).
    LinkSetup = 6,
    /// Sent from device to host to let it know that it received LinkSetup and that protocol version is compatible,
    /// echoing the token. Otherwise, Disconnect with DisconnectReason::IncompatibleVersion is sent.
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
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
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
    /// The token of the [Kind::GetDeviceInfo] this answers, see [Message::GetDeviceInfo]. 0 from a device built
    /// before the token existed.
    #[default = 0]
    pub token: u64,
}

#[cfg(feature = "std")]
impl DeviceInfo<'_> {
    pub fn make_owned(&self) -> DeviceInfoOwned {
        DeviceInfoOwned {
            dev_link_version: self.dev_link_version,
            api_model_version: self.api_model_version,
            user_api_version: self.user_api_version.make_owned(),
            hash: self.hash.make_owned(),
            dev_max_message_len: self.dev_max_message_len,
            packet_accumulation_time_us: self.packet_accumulation_time_us,
            token: self.token,
        }
    }
}

/// Sent from host to device after receiving [DeviceInfo].
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct LinkSetup<'i> {
    /// User API and data types version on the host side
    pub host_user_version: FullVersion<'i>,
    /// Maximum length message that host can process
    pub host_max_message_len: u32,
    /// The host's setup token, echoed in [Message::LinkReady]. 0 from a host built before the token existed, which
    /// the device refuses.
    #[default = 0]
    pub token: u64,
    /// Who connects (CLT-41): program, user, host, pid. `None` from a host built before it existed, or one that
    /// chose not to say. The device may log it (`ww_device`'s `DeviceLink::peer_meta`), a hub shows it.
    #[default = None]
    pub client_meta: Option<ClientMeta<'i>>,
}

/// What kind of program a client is, for lists of who holds or uses a device.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, Eq, PartialEq),
    ww_repr = u8,
    future_version = Newer
)]
pub enum ClientKind {
    /// A command line tool (the default).
    Cli,
    /// A program with a window.
    Gui,
    /// A Python notebook or script through `wire_weaver_py`.
    Notebook,
    /// A long-running background program.
    Service,
    /// A ww hub forwarding for other clients.
    Hub,
    /// A test run.
    Test,
    /// A kind this version has no name for.
    Unknown,
}

/// Who a client is, sent in [LinkSetup::client_meta] and written to holder lock files. Free text is for people to
/// read (lists, logs), nothing decides on it.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct ClientMeta<'i> {
    /// Program name, e.g. the executable's file name.
    pub program: &'i str,
    /// Version of the program, free text, empty if unknown.
    pub app_version: &'i str,
    /// A name the user gave this client, empty if none.
    pub label: &'i str,
    pub kind: ClientKind,
    /// Operating system process id, 0 if unknown.
    pub pid: u32,
    /// Login name of the user running the program, empty if unknown.
    pub user: &'i str,
    /// Host name of the PC running the program, empty if unknown.
    pub host: &'i str,
    /// What the program is doing with the device, e.g. "DFU update", for people.
    pub purpose: Option<&'i str>,
    /// Random number picked per client object, shared by all its connections: tells two clients of the same
    /// program and pid apart, and one client's several devices together. 0 if unknown.
    pub instance_id: u64,
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
    /// `token` is a random, non-zero number the host picks per connection (`u64`, little endian). The device
    /// echoes it in [DeviceInfo::token] and [Message::LinkReady], and the host drops everything that arrives
    /// before the reply carrying it: whatever was still on the way from a previous session (an answer already in
    /// the device's IN buffer, a half-sent message) is never taken for the new one. 0 (no payload) from a host
    /// built before the token existed.
    GetDeviceInfo {
        token: u64,
    },
    DeviceInfo(DeviceInfo<'i>),
    LinkSetup(LinkSetup<'i>),
    /// `token` echoes [LinkSetup::token] (`u64`, little endian), 0 (no payload) from a device built before it existed.
    LinkReady {
        token: u64,
    },
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
            Message::GetDeviceInfo { .. } => Kind::GetDeviceInfo,
            Message::DeviceInfo(_) => Kind::DeviceInfo,
            Message::LinkSetup(_) => Kind::LinkSetup,
            Message::LinkReady { .. } => Kind::LinkReady,
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
            Kind::GetDeviceInfo => Message::GetDeviceInfo {
                token: read_token(&mut rd).map_err(malformed)?,
            },
            Kind::DeviceInfo => {
                Message::DeviceInfo(DeviceInfo::des_shrink_wrap(&mut rd).map_err(malformed)?)
            }
            Kind::LinkSetup => {
                Message::LinkSetup(LinkSetup::des_shrink_wrap(&mut rd).map_err(malformed)?)
            }
            Kind::LinkReady => Message::LinkReady {
                token: read_token(&mut rd).map_err(malformed)?,
            },
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
            Message::Nop | Message::Ping | Message::GetStats => {}
            Message::GetDeviceInfo { token } | Message::LinkReady { token } => {
                wr.write_u64(*token).map_err(too_small)?
            }
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

/// A token: 8 bytes little endian, or 0 when there are none (a peer built before tokens existed).
fn read_token(rd: &mut BufReader<'_>) -> Result<u64, shrink_wrap::Error> {
    if rd.bytes_left() == 0 {
        return Ok(0);
    }
    rd.read_u64()
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    fn test_meta() -> ClientMeta<'static> {
        ClientMeta {
            program: "ww",
            app_version: "0.5.1",
            label: "bench",
            kind: ClientKind::Notebook,
            pid: 4711,
            user: "roman",
            host: "pc1",
            purpose: Some("dfu"),
            instance_id: 0x0102_0304_0506_0708,
        }
    }

    fn round_trip(msg: Message<'_>) {
        let mut scratch = [0u8; 256];
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
        round_trip(Message::GetDeviceInfo {
            token: 0x0123_4567_89AB_CDEF,
        });
        round_trip(Message::DeviceInfo(DeviceInfo {
            dev_link_version: CompactVersion::new(ww_global::WIRE_WEAVER_USB_LINK, 1, 2, 3),
            api_model_version: CompactVersion::new(ww_global::WW_CLIENT_SERVER, 0, 2, 0),
            user_api_version: FullVersion::new("test", ww_version::Version::new(0, 1, 0)),
            hash: ApiHashPair::empty(),
            dev_max_message_len: 2048,
            packet_accumulation_time_us: 1000,
            token: u64::MAX,
        }));
        round_trip(Message::LinkSetup(LinkSetup {
            host_user_version: FullVersion::new("test", ww_version::Version::new(0, 1, 0)),
            host_max_message_len: 4096,
            token: 7,
            client_meta: None,
        }));
        round_trip(Message::LinkSetup(LinkSetup {
            host_user_version: FullVersion::new("test", ww_version::Version::new(0, 1, 0)),
            host_max_message_len: 4096,
            token: 7,
            client_meta: Some(test_meta()),
        }));
        round_trip(Message::LinkReady { token: 7 });
        round_trip(Message::Ping);
        round_trip(Message::Loopback {
            repeat: 1,
            seq: 7,
            data: &[0xAA; 10],
        });
        round_trip(Message::Disconnect(DisconnectReason::RequestByUser));
    }

    #[test]
    fn token_bytes_and_peers_without_one() {
        let mut scratch = [0u8; 16];
        let (kind, payload) = Message::GetDeviceInfo { token: 0x0102 }
            .encode(&mut scratch)
            .unwrap();
        assert_eq!(kind, Kind::GetDeviceInfo as u8);
        assert_eq!(payload, &[0x02, 0x01, 0, 0, 0, 0, 0, 0]);
        // an older peer sends no payload
        assert!(matches!(
            Message::decode(Kind::GetDeviceInfo as u8, &[]),
            Ok(Message::GetDeviceInfo { token: 0 })
        ));
        assert!(matches!(
            Message::decode(Kind::LinkReady as u8, &[]),
            Ok(Message::LinkReady { token: 0 })
        ));
        assert!(matches!(
            Message::decode(Kind::LinkReady as u8, &[1, 2, 3]),
            Err(Error::Malformed(Kind::LinkReady))
        ));
        // LinkSetup (and DeviceInfo) of an older peer end before the token: read as 0
        #[derive_shrink_wrap(borrowed, derive(Debug))]
        struct OldLinkSetup<'i> {
            host_user_version: FullVersion<'i>,
            host_max_message_len: u32,
        }
        let old = OldLinkSetup {
            host_user_version: FullVersion::new("test", ww_version::Version::new(0, 1, 0)),
            host_max_message_len: 4096,
        };
        let mut buf = [0u8; 64];
        let bytes = old.to_ww_bytes(&mut buf).unwrap().to_vec();
        let Ok(Message::LinkSetup(read)) = Message::decode(Kind::LinkSetup as u8, &bytes) else {
            panic!("an older LinkSetup must still decode");
        };
        assert_eq!((read.host_max_message_len, read.token), (4096, 0));
    }

    #[test]
    fn client_meta_bytes_and_older_peers() {
        // an older device reads the longer LinkSetup, a newer host reads the older one without client_meta
        #[derive_shrink_wrap(borrowed, derive(Debug))]
        struct NoMetaLinkSetup<'i> {
            host_user_version: FullVersion<'i>,
            host_max_message_len: u32,
            #[default = 0]
            token: u64,
        }
        let setup = LinkSetup {
            host_user_version: FullVersion::new("test", ww_version::Version::new(0, 1, 0)),
            host_max_message_len: 4096,
            token: 7,
            client_meta: Some(test_meta()),
        };
        let mut buf = [0u8; 256];
        let with = setup.to_ww_bytes(&mut buf).unwrap().to_vec();
        let old = NoMetaLinkSetup::from_ww_bytes(&with).expect("an older device must read it");
        assert_eq!((old.host_max_message_len, old.token), (4096, 7));

        let without = NoMetaLinkSetup {
            host_user_version: FullVersion::new("test", ww_version::Version::new(0, 1, 0)),
            host_max_message_len: 4096,
            token: 7,
        };
        let mut buf = [0u8; 64];
        let bytes = without.to_ww_bytes(&mut buf).unwrap().to_vec();
        let Ok(Message::LinkSetup(read)) = Message::decode(Kind::LinkSetup as u8, &bytes) else {
            panic!("a LinkSetup without client_meta must still decode");
        };
        assert!(read.client_meta.is_none());
        assert_eq!(read.token, 7);
    }

    #[test]
    fn unknown_kind() {
        assert_eq!(
            Message::decode(200, &[]).err(),
            Some(Error::UnknownKind(200))
        );
    }

    #[test]
    fn link_crc32_is_crc32_iso_hdlc_over_header_and_message() {
        use ww_framer::crc::Crc32IsoHdlc;
        assert_eq!(LinkCrc32::checksum(b"", b"123456789"), 0xcbf4_3926);
        assert_eq!(LinkCrc32::checksum(b"1234", b"56789"), 0xcbf4_3926);
        for (h, m) in [(&b""[..], &b""[..]), (&[0x15, 0x02], &[1, 2, 3, 4, 5][..])] {
            assert_eq!(LinkCrc32::checksum(h, m), Crc32IsoHdlc::checksum(h, m));
        }
    }
}
