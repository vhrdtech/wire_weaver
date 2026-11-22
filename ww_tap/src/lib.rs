//! The link tap: what crosses a wire_weaver link, recorded at the transport edge, for monitors, statistics and
//! captures (TR-14).
//!
//! A transport calls a [Tap] at the two layers it has: [Tap::frame] with the bytes as they cross the medium (a USB
//! packet, a datagram, an RTT chunk) and [Tap::message] with every `ww_link` message `(kind, bytes)` as it goes down
//! to the framer or comes back up; the event loop adds what never shows in bytes with [Tap::link_event]. The tap
//! only copies bytes and stamps time: linking frames to messages, statistics and decoding happen in the consumer
//! (`wire_weaver_client::tap`). [NoTap] is the default everywhere, its calls are empty and inline, so a build
//! without a tap compiles them away.
//!
//! The records are ordinary wire_weaver types, streams of the [LinkTap] trait, so a capture is described by its API
//! bundle like any device log: one track per stream of each link, Session sticky. A producer that writes them as
//! tracks names them with [api] and [ids], and tells links apart by their [dims]: the link's medium and peer, so the
//! same device gives the same track ids in every capture. A link named by its peer is [LinkKey]. With the `std`
//! feature, [tape] defines a link's tracks and the tap clock in `ww_tape` and finds them again.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[cfg(feature = "std")]
pub mod tape;
#[cfg(feature = "std")]
pub use tape::*;

/// Direction, from the side that taps ([Session::side]): Tx is what it sends.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    Tx,
    Rx,
}

/// Where a transport reports what crosses the link it serves; one per link, see the [crate docs](crate).
pub trait Tap {
    /// Whether frames are wanted at all: a transport skips the work around [Tap::frame] when false.
    fn wants_frames(&self) -> bool {
        true
    }
    /// Whether messages are wanted at all.
    fn wants_messages(&self) -> bool {
        true
    }
    /// The link starts: medium and framing, everything a decoder needs to re-run the framer.
    fn session(&mut self, session: &Session<'_>);
    /// A frame as it crossed the medium: the bytes of a USB packet or a datagram, a chunk of a byte stream.
    fn frame(&mut self, dir: Dir, bytes: &[u8]);
    /// A `ww_link` message as it went down to the framer or came back up.
    fn message(&mut self, dir: Dir, kind: u8, bytes: &[u8]);
    /// Something the bytes don't show.
    fn link_event(&mut self, event: &LinkEvent<'_>);
}

/// A tap that records nothing.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoTap;

impl Tap for NoTap {
    #[inline(always)]
    fn wants_frames(&self) -> bool {
        false
    }
    #[inline(always)]
    fn wants_messages(&self) -> bool {
        false
    }
    #[inline(always)]
    fn session(&mut self, _session: &Session<'_>) {}
    #[inline(always)]
    fn frame(&mut self, _dir: Dir, _bytes: &[u8]) {}
    #[inline(always)]
    fn message(&mut self, _dir: Dir, _kind: u8, _bytes: &[u8]) {}
    #[inline(always)]
    fn link_event(&mut self, _event: &LinkEvent<'_>) {}
}

impl<T: Tap + ?Sized> Tap for &mut T {
    #[inline(always)]
    fn wants_frames(&self) -> bool {
        (**self).wants_frames()
    }
    #[inline(always)]
    fn wants_messages(&self) -> bool {
        (**self).wants_messages()
    }
    #[inline(always)]
    fn session(&mut self, session: &Session<'_>) {
        (**self).session(session)
    }
    #[inline(always)]
    fn frame(&mut self, dir: Dir, bytes: &[u8]) {
        (**self).frame(dir, bytes)
    }
    #[inline(always)]
    fn message(&mut self, dir: Dir, kind: u8, bytes: &[u8]) {
        (**self).message(dir, kind, bytes)
    }
    #[inline(always)]
    fn link_event(&mut self, event: &LinkEvent<'_>) {
        (**self).link_event(event)
    }
}

impl<T: Tap> Tap for Option<T> {
    #[inline(always)]
    fn wants_frames(&self) -> bool {
        self.as_ref().is_some_and(|t| t.wants_frames())
    }
    #[inline(always)]
    fn wants_messages(&self) -> bool {
        self.as_ref().is_some_and(|t| t.wants_messages())
    }
    #[inline(always)]
    fn session(&mut self, session: &Session<'_>) {
        if let Some(t) = self {
            t.session(session)
        }
    }
    #[inline(always)]
    fn frame(&mut self, dir: Dir, bytes: &[u8]) {
        if let Some(t) = self {
            t.frame(dir, bytes)
        }
    }
    #[inline(always)]
    fn message(&mut self, dir: Dir, kind: u8, bytes: &[u8]) {
        if let Some(t) = self {
            t.message(dir, kind, bytes)
        }
    }
    #[inline(always)]
    fn link_event(&mut self, event: &LinkEvent<'_>) {
        if let Some(t) = self {
            t.link_event(event)
        }
    }
}

/// Records of a link tap, one stream per kind of record. Implemented by nobody: the trait is there for its API
/// bundle, which describes the tracks of a capture.
///
/// One link's records are one set of these streams: the tracks of every link share their keys, and the link's
/// identity, its [dimensions](dims) (the medium and the peer, a USB serial or an address), tells them
/// apart. The same device recorded twice, in one capture or in two, gives its tracks the same ids.
#[ww_trait]
pub trait LinkTap {
    /// The link's session, updated when it is set up (a state track: put near each use).
    stream!(session: Session<'i>);
    /// Connects, disconnects, errors, lost records.
    stream!(events: LinkEvent<'i>);
    /// Frames sent.
    stream!(tx_frames: Frame<'i>);
    /// Frames received.
    stream!(rx_frames: Frame<'i>);
    /// Messages sent.
    stream!(tx_messages: Message<'i>);
    /// Messages received.
    stream!(rx_messages: Message<'i>);
    /// Server only: which client's request a device-side request was.
    stream!(client_map: ClientMap);
}

/// Resource ids of [LinkTap]'s streams, as in its bundle (declaration order), for track paths.
pub mod ids {
    pub const SESSION: u32 = 0;
    pub const EVENTS: u32 = 1;
    pub const TX_FRAMES: u32 = 2;
    pub const RX_FRAMES: u32 = 3;
    pub const TX_MESSAGES: u32 = 4;
    pub const RX_MESSAGES: u32 = 5;
    pub const CLIENT_MAP: u32 = 6;

    /// The name of a [LinkTap](crate::LinkTap) stream by its id, as in a track's name path; None for an id that is
    /// not a stream.
    pub const fn stream_name(stream: u32) -> Option<&'static str> {
        Some(match stream {
            SESSION => "session",
            EVENTS => "events",
            TX_FRAMES => "tx_frames",
            RX_FRAMES => "rx_frames",
            TX_MESSAGES => "tx_messages",
            RX_MESSAGES => "rx_messages",
            CLIENT_MAP => "client_map",
            _ => return None,
        })
    }
}

/// Names of the dimensions of a link's tracks: what tells the tracks of two links apart, whose keys are the same.
pub mod dims {
    /// The link's medium, [Medium::name](crate::Medium::name).
    pub const MEDIUM: &str = "medium";
    /// The peer of the link's first session: a USB serial, an address, an in-process path.
    pub const PEER: &str = "peer";
    /// The producer's number of the link, only where the peer doesn't tell it apart: no peer, or another open link of
    /// the same medium and peer.
    pub const LINK: &str = "link";
}

/// The API the tap's tracks are in: this crate's name and version, as in its bundle ([LINK_TAP_FULL_GID]).
pub const fn api() -> FullVersion<'static> {
    LINK_TAP_FULL_GID
}

/// The start of a link: everything a decoder needs to re-run the framer and find the API bundle.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Session<'i> {
    /// Which end of the link taps.
    pub side: Side,
    pub medium: Medium,
    /// What the medium's frames are packed with.
    pub framer: Framer,
    /// Largest frame, 0 for media without frames (WebSocket, in-process messages).
    pub frame_size: UNib32,
    /// Which device or peer: a USB serial, an address, an in-process path.
    pub peer: &'i str,
    /// The device's `ww_link` `DeviceInfo` in its ShrinkWrap encoding (with the `ApiHashPair` that finds the API
    /// bundle), once the link is set up; empty before.
    pub device_info: RefVec<'i, u8>,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    ww_repr = u4
)]
pub enum Side {
    Host,
    Device,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    ww_repr = u4
)]
pub enum Medium {
    Usb,
    Udp,
    WebSocket,
    Rtt,
    /// In-process, messages moved as they are.
    InProcess,
    /// In-process with simulated USB packets.
    InProcessUsb,
    Other,
    /// Local socket (Unix domain socket, named pipe), e.g. to the ww hub (TR-16); messages as they are.
    LocalSocket,
    /// In-process with simulated UDP datagrams.
    InProcessUdp,
    /// In-process WebSocket over an in-memory pipe; messages as they are.
    InProcessWebSocket,
    /// A byte stream other than a local socket (TR-19): TCP, a command's stdio, a stream the program opened;
    /// messages as they are.
    ByteStream,
}

impl Medium {
    /// Its name in snake case, the value of a link's [dims::MEDIUM] dimension.
    pub const fn name(self) -> &'static str {
        match self {
            Medium::Usb => "usb",
            Medium::Udp => "udp",
            Medium::WebSocket => "web_socket",
            Medium::Rtt => "rtt",
            Medium::InProcess => "in_process",
            Medium::InProcessUsb => "in_process_usb",
            Medium::Other => "other",
            Medium::LocalSocket => "local_socket",
            Medium::InProcessUdp => "in_process_udp",
            Medium::InProcessWebSocket => "in_process_web_socket",
            Medium::ByteStream => "byte_stream",
            Medium::Unknown(_) => "unknown",
        }
    }
}

/// How frames are packed (`ww_framer`), see `ww_link` for each medium's configuration.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    ww_repr = u4
)]
pub enum Framer {
    /// No framer: one message per medium message, the frame layer is empty.
    None,
    /// `U2Head`, then the checksum and tail.
    U2 {
        checksum: FramerChecksum,
        tail: FramerTail,
        /// Stream medium: frames are chunks of a byte stream, every message is Full and may span chunks.
        stream: bool,
        /// What every frame starts with, before the first head.
        #[default = FramerPrefix::None]
        prefix: FramerPrefix,
    },
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    ww_repr = u4
)]
pub enum FramerChecksum {
    None,
    /// `CrcChecksum<Crc16IbmSdlc, FULL>`: split messages always, Full ones when `full`.
    Crc16IbmSdlc {
        full: bool,
    },
    /// `CrcChecksum<Crc32IsoHdlc, FULL>` (`ww_link::UdpChecksum` is the one with `full`): split messages always, Full
    /// ones when `full`.
    Crc32IsoHdlc {
        full: bool,
    },
}

/// Frame prefix (`ww_framer::prefix`).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    ww_repr = u4
)]
pub enum FramerPrefix {
    /// `NopPrefix`: frames start with the first head.
    None,
    /// `SeqPrefix`: one byte `00ee_ssss`, frame counter mod 16 and session epoch mod 4.
    Seq,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    ww_repr = u4
)]
pub enum FramerTail {
    None,
    Byte(u8),
}

/// A frame as it crossed the medium.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Frame<'i> {
    /// Counts per link and direction from 0.
    pub no: UVlq64,
    pub bytes: RefVec<'i, u8>,
}

/// A `ww_link` message.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Message<'i> {
    /// Counts per link and direction from 0.
    pub no: UVlq64,
    /// `ww_link::Kind`
    pub kind: u8,
    pub bytes: RefVec<'i, u8>,
}

/// What the bytes don't show.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone), ww_repr = u4)]
pub enum LinkEvent<'i> {
    /// The medium is open.
    Connected,
    /// Link setup is done, the device's `DeviceInfo` bytes are in the session.
    LinkReady,
    Disconnected {
        reason: &'i str,
        /// The client keeps its stream subscriptions for a re-connect.
        keep_streams: bool,
    },
    /// A transport or protocol error.
    Error { what: &'i str },
    /// Records the producer couldn't keep, or a consumer missed, counted since the last such event; the gap is between this event and the record before
    /// it.
    Lost { records: UVlq64 },
    /// The link is gone: its tracks get no more records. A relay of the stream drops their definitions
    /// (`tape::drop_closed_links`), so they don't pile up with every link ever seen.
    Closed,
}

/// A link named by its peer, as the hub names the links it watches: one device keeps its name across reconnects.
/// The same two fields are a link's track [dimensions](dims).
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone))]
pub struct LinkKey<'i> {
    pub medium: Medium,
    /// [Session::peer] of the link's first session.
    pub peer: &'i str,
}

impl<'i> LinkKey<'i> {
    /// The key of the link that `session` starts.
    pub fn of(session: &Session<'i>) -> Self {
        LinkKey {
            medium: session.medium,
            peer: session.peer,
        }
    }
}

/// Server only: a client's request as the server sent it on to the device.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Copy, Clone)
)]
pub struct ClientMap {
    pub client: UNib32,
    pub client_seq: UNib32,
    pub device_seq: UNib32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Track definitions carry the crate's name and version, as producers wrote them before `api()` was the GID.
    #[test]
    fn api_is_the_crate_name_and_version() {
        let v = api();
        assert_eq!(v.crate_id, env!("CARGO_PKG_NAME"));
        let n = |s: &str| s.parse::<u32>().unwrap();
        assert_eq!(v.version.major.0, n(env!("CARGO_PKG_VERSION_MAJOR")));
        assert_eq!(v.version.minor.0, n(env!("CARGO_PKG_VERSION_MINOR")));
        assert_eq!(v.version.patch.0, n(env!("CARGO_PKG_VERSION_PATCH")));
    }

    #[test]
    fn link_key_of_a_session_round_trips() {
        let session = Session {
            side: Side::Host,
            medium: Medium::Usb,
            framer: Framer::None,
            frame_size: UNib32(64),
            peer: "SN-1234",
            device_info: RefVec::Slice { slice: &[] },
        };
        let key = LinkKey::of(&session);
        let mut buf = [0u8; 32];
        let bytes = key.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(LinkKey::from_ww_bytes(bytes).unwrap(), key);
    }
}
