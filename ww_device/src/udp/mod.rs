//! UDP as a medium: one [ww_framer] frame per datagram, with `ww_link`'s UDP framer configuration, the same format
//! the `udp` transport of `wire_weaver_client` speaks. Small messages share a datagram, big ones are split across
//! several, exactly as over USB, so [FramedTx] / [FramedRx] do the framing on top of [UdpSink] / [UdpSource].
//!
//! UDP has no connections, so the device tells hosts (peers) apart by their address, and serves each from a slot of
//! a [UdpPeers]: up to `PEERS` hosts on one socket, chosen at build time ([UdpConnection] for one). Each slot has its
//! own [UdpSink] / [UdpSource] pair, framers and link, so each host has a link of its own:
//! - a host gets a slot when it starts link setup, i.e., when its datagram is a `Nop` first or the frozen hello
//!   (`ww_link::hello::udp`, outside the framer; the host sends both alone, before anything else). The medium of
//!   that slot goes down and up again, so the link and the framers start over, and the datagram that started it is
//!   not lost;
//! - datagrams from a host without a slot are dropped;
//! - with every slot taken, the host heard from least recently gives its slot up: one gone quiet for longer than
//!   the link's peer timeout comes first, as a live host pings well within it. With one slot, any host that starts
//!   link setup takes it over, as if a USB device was opened by another host process.
//!
//! A firmware runs one server per slot for good; a host program accepts a connection per host
//! ([UdpPeers::accept]) and ends it when its link goes down, which frees the slot. See [peers] for the details.
//!
//! There is no authentication: anyone that can reach the port can take a slot. Nothing is retransmitted either, see
//! the UDP transport docs for what happens on loss.
//!
//! Datagram sockets are used through `&self`, so no locking is needed. With the `embassy-net` feature,
//! `EmbassyNetUdpSocket` implements [DatagramSocket] on an `embassy-net` UDP socket, with `std` `StdUdpSocket` on
//! the standard library's, on any event loop. [udp_server] puts everything together.

use core::future::Future;
use core::ops::Deref;

use wire_weaver::shrink_wrap::BufReader;
use ww_framer::prefix::FramePrefix;
use ww_framer::traits::{Checksum, Head, MessageKind};
use ww_link::{Framing, Kind, UDP_MAX_DATAGRAM_LEN, UdpChecksum, UdpFraming, UdpHead, UdpPrefix};

use crate::buffer::RxBuffer;
use crate::server::Clock;
use crate::transport::{FramedRx, FramedTx, PacketSink, PacketSource};
use crate::{LinkConfig, Server};

pub mod peers;
use peers::sealed::Table;
pub use peers::{Direct, PeerTable, Shared, UdpConnection, UdpPeers};

/// Datagram socket bound to a local port. Both methods take `&self`, so that tx and rx can use it independently.
pub trait DatagramSocket {
    /// Remote address (IP and port).
    type Addr: Copy + PartialEq;
    type RecvError;
    type SendError;

    /// Receive one datagram into `buf`, returns its length and sender. A datagram longer than `buf` is truncated or
    /// dropped with an error. Must be cancel-safe.
    fn recv_from(
        &self,
        buf: &mut [u8],
    ) -> impl Future<Output = Result<(usize, Self::Addr), Self::RecvError>>;

    /// Send one datagram to `addr`. Should not wait forever (e.g., when the network interface is down), so that the
    /// event loop keeps running.
    fn send_to(
        &self,
        datagram: &[u8],
        addr: Self::Addr,
    ) -> impl Future<Output = Result<(), Self::SendError>>;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum UdpError<E> {
    Socket(E),
    /// Nothing to send to: no host has started link setup yet
    NoPeer,
    /// Another host started link setup and replaced the current one
    PeerChanged,
}

/// [PacketSink] sending datagrams to the host of one slot of a [UdpPeers].
pub struct UdpSink<P> {
    peers: P,
    slot: usize,
}

impl<P: Deref<Target: PeerTable>> UdpSink<P> {
    /// Sends to the host `slot` serves.
    pub fn new(peers: P, slot: usize) -> Self {
        UdpSink { peers, slot }
    }
}

impl<P: Deref<Target: PeerTable>> PacketSink for UdpSink<P> {
    type Error = UdpError<<Socket<P> as DatagramSocket>::SendError>;
    type Framing = UdpFraming;

    async fn write_packet(&mut self, packet: &[u8]) -> Result<(), Self::Error> {
        let peer = self.peers.send_to(self.slot)?;
        self.peers
            .socket()
            .send_to(packet, peer)
            .await
            .map_err(UdpError::Socket)
    }
}

/// Longest datagram that can start link setup. The host sends `Nop` (1 byte prefix, 1 byte head, 4 byte CRC) and the
/// frozen hello (3 byte head, 16 byte hello, 4 byte CRC, `ww_link::hello::udp`) alone; a later host's longer hello
/// fits too.
const SETUP_DATAGRAM_MAX_LEN: usize = ww_link::hello::HELLO_FRAME_MAX;

/// Datagram that starts link setup, kept across the medium going down and up again.
#[derive(Copy, Clone)]
struct SetupDatagram<A> {
    from: A,
    bytes: [u8; SETUP_DATAGRAM_MAX_LEN],
    len: usize,
}

impl<A> SetupDatagram<A> {
    /// [Self::new] of the first `len` bytes of `buf`, as a socket reported them: all of `buf` when it was truncated.
    fn received(buf: &[u8], len: usize, from: A) -> Option<Self> {
        Self::new(&buf[..len.min(buf.len())], from)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// Some if `datagram` starts link setup and is short enough to keep.
    fn new(datagram: &[u8], from: A) -> Option<Self> {
        if datagram.len() > SETUP_DATAGRAM_MAX_LEN || !starts_link_setup(datagram) {
            return None;
        }
        let mut bytes = [0u8; SETUP_DATAGRAM_MAX_LEN];
        bytes[..datagram.len()].copy_from_slice(datagram);
        Some(SetupDatagram {
            from,
            bytes,
            len: datagram.len(),
        })
    }
}

/// `datagram` is the frozen hello, read outside the framer as [FramedRx] reads it, or its first message (after the
/// frame prefix) is a whole `Nop`; both with a good CRC, so that a corrupted datagram can't take over.
fn starts_link_setup(datagram: &[u8]) -> bool {
    if UdpFraming::find_hello(datagram).is_some() {
        return true;
    }
    let Some(after_prefix) = datagram.get(UdpPrefix::LEN..) else {
        return false;
    };
    let mut rd = BufReader::new(after_prefix);
    let Ok((MessageKind::Full, kind, len)) = UdpHead::read(&mut rd) else {
        return false;
    };
    if kind != Kind::Nop as u8 {
        return false;
    }
    let header = &datagram[..datagram.len() - rd.bytes_left()];
    let Ok(message) = rd.read_raw_slice(len) else {
        return false;
    };
    UdpChecksum::read(header, message, false, &mut rd).is_ok()
}

/// [PacketSource] receiving the datagrams of one slot of a [UdpPeers], see the [module docs](peers) on how hosts get
/// a slot. Dropping it frees the slot.
pub struct UdpSource<P: Deref<Target: PeerTable>> {
    peers: P,
    slot: usize,
}

impl<P: Deref<Target: PeerTable>> UdpSource<P> {
    /// Receives for `slot`: for good, or for the connection [UdpPeers::accept] handed out.
    ///
    /// # Panics
    ///
    /// `slot` is out of range, or already has a source.
    pub fn new(peers: P, slot: usize) -> Self {
        peers.claim(slot);
        UdpSource { peers, slot }
    }

    pub fn slot(&self) -> usize {
        self.slot
    }
}

impl<P: Deref<Target: PeerTable>> Drop for UdpSource<P> {
    fn drop(&mut self) {
        self.peers.release(self.slot);
    }
}

impl<P: Deref<Target: PeerTable>> PacketSource for UdpSource<P> {
    type Error = UdpError<<Socket<P> as DatagramSocket>::RecvError>;
    type Framing = UdpFraming;

    /// The host sends datagrams of at most this size.
    fn max_packet_len(&self) -> usize {
        UDP_MAX_DATAGRAM_LEN
    }

    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.peers.read(self.slot, buf).await
    }

    async fn wait_connected(&mut self) {
        self.peers.wait_connected(self.slot).await
    }
}

/// The socket of the table behind `P`.
type Socket<P> = <<P as Deref>::Target as Table>::Socket;

pub type UdpTx<'a, P> = FramedTx<'a, UdpSink<P>>;
pub type UdpRx<'a, P> = FramedRx<'a, UdpSource<P>>;
/// [UdpTx] / [UdpRx] server of one slot, see [Server] on how to use it.
pub type UdpServer<'a, P, C, M = ()> = Server<'a, UdpTx<'a, P>, UdpRx<'a, P>, C, M>;

/// Buffers used by [UdpServer], `MAX_MESSAGE_LEN` is the longest message the device accepts and the longest reply
/// it can serialize, reported to the host exactly as is. Longer messages than a datagram are split across several.
///
/// Takes `3 * MAX_MESSAGE_LEN + 2 * UDP_MAX_DATAGRAM_LEN` bytes, in addition to the socket's own buffers, which
/// must fit at least one datagram of [UDP_MAX_DATAGRAM_LEN] each way.
pub struct UdpBuffers<const MAX_MESSAGE_LEN: usize> {
    /// Datagrams are reassembled into messages here
    rx: RxBuffer<UDP_MAX_DATAGRAM_LEN, MAX_MESSAGE_LEN>,
    /// Outgoing datagram is accumulated here
    tx: [u8; UDP_MAX_DATAGRAM_LEN],
    /// Used to serialize replies and link messages
    scratch: [u8; MAX_MESSAGE_LEN],
    /// Used to serialize events sent from handlers and through `server.sink()`
    event_scratch: [u8; MAX_MESSAGE_LEN],
}

impl<const MAX_MESSAGE_LEN: usize> UdpBuffers<MAX_MESSAGE_LEN> {
    pub const fn new() -> Self {
        UdpBuffers {
            rx: RxBuffer::new(),
            tx: [0u8; UDP_MAX_DATAGRAM_LEN],
            scratch: [0u8; MAX_MESSAGE_LEN],
            event_scratch: [0u8; MAX_MESSAGE_LEN],
        }
    }
}

impl<const MAX_MESSAGE_LEN: usize> Default for UdpBuffers<MAX_MESSAGE_LEN> {
    fn default() -> Self {
        Self::new()
    }
}

/// UDP server of one slot of a [UdpPeers], e.g. for one host:
/// ```ignore
/// let socket = EmbassyNetUdpSocket::bind(UdpSocket::new(stack, rx_meta, rx_buf, tx_meta, tx_buf), 9000).unwrap();
/// let conn = UDP_CONNECTION.init(UdpConnection::new(socket));
/// let mut server = udp_server(link_config, &*conn, 0, EmbassyClock, UDP_BUFFERS.init(UdpBuffers::new()));
/// server.run(&mut state).await;
/// ```
/// For several hosts, one server per slot of a `UdpPeers<_, PEERS>`, selected on together as [Server] shows for
/// several media, or one per connection [UdpPeers::accept] hands out.
pub fn udp_server<'a, const MAX_MESSAGE_LEN: usize, P, C: Clock>(
    link_config: LinkConfig<'a>,
    peers: P,
    slot: usize,
    clock: C,
    buffers: &'a mut UdpBuffers<MAX_MESSAGE_LEN>,
) -> UdpServer<'a, P, C>
where
    P: Deref<Target: PeerTable> + Clone,
{
    Server::new(
        link_config,
        FramedTx::new(UdpSink::new(peers.clone(), slot), &mut buffers.tx),
        FramedRx::new(
            UdpSource::new(peers, slot),
            buffers.rx.assembly_buf(UDP_MAX_DATAGRAM_LEN),
        ),
        clock,
        &mut buffers.scratch,
        &mut buffers.event_scratch,
    )
}

#[cfg(feature = "embassy-net")]
pub use embassy::EmbassyNetUdpSocket;

#[cfg(feature = "std")]
mod std_socket;
#[cfg(feature = "std")]
pub use std_socket::{Readiness, StdUdpSocket};

#[cfg(feature = "embassy-net")]
mod embassy {
    use embassy_net::IpEndpoint;
    use embassy_net::udp::{BindError, RecvError, SendError, UdpSocket};
    use embassy_time::{Duration, with_timeout};

    use super::DatagramSocket;

    /// [DatagramSocket] on an `embassy-net` UDP socket.
    ///
    /// Sends give up after [send_timeout](Self::set_send_timeout) (1 s by default), e.g., when the interface is down
    /// and the socket's tx buffer is full, the link then goes down instead of blocking the event loop.
    pub struct EmbassyNetUdpSocket<'a> {
        socket: UdpSocket<'a>,
        send_timeout: Duration,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub enum EmbassyNetSendError {
        Send(SendError),
        Timeout,
    }

    impl<'a> EmbassyNetUdpSocket<'a> {
        /// Bind `socket` to `port` on all local addresses.
        pub fn bind(mut socket: UdpSocket<'a>, port: u16) -> Result<Self, BindError> {
            socket.bind(port)?;
            Ok(EmbassyNetUdpSocket {
                socket,
                send_timeout: Duration::from_secs(1),
            })
        }

        pub fn set_send_timeout(&mut self, timeout: Duration) {
            self.send_timeout = timeout;
        }

        pub fn socket(&self) -> &UdpSocket<'a> {
            &self.socket
        }
    }

    impl DatagramSocket for EmbassyNetUdpSocket<'_> {
        type Addr = IpEndpoint;
        type RecvError = RecvError;
        type SendError = EmbassyNetSendError;

        async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, IpEndpoint), RecvError> {
            let (len, meta) = self.socket.recv_from(buf).await?;
            Ok((len, meta.endpoint))
        }

        async fn send_to(
            &self,
            datagram: &[u8],
            addr: IpEndpoint,
        ) -> Result<(), EmbassyNetSendError> {
            match with_timeout(self.send_timeout, self.socket.send_to(datagram, addr)).await {
                Ok(r) => r.map_err(EmbassyNetSendError::Send),
                Err(_) => Err(EmbassyNetSendError::Timeout),
            }
        }
    }
}

#[cfg(test)]
mod tests;
