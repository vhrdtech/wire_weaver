//! UDP as a medium: one [ww_framer] frame per datagram, with `ww_link`'s UDP framer configuration, the same format
//! the `udp` transport of `wire_weaver_client` speaks. Small messages share a datagram, big ones are split across
//! several, exactly as over USB, so [FramedTx] / [FramedRx] do the framing on top of [UdpSink] / [UdpSource].
//!
//! UDP has no connections, so the device serves one host (peer) at a time and replies to it only:
//! - a peer is adopted when it starts link setup, i.e., when the first message of its datagram is `Nop` or
//!   `GetDeviceInfo` (the host sends both alone, before anything else);
//! - datagrams from anyone else are dropped, unless they start link setup: then the current peer is replaced, as if a
//!   USB device was opened by another host process. The medium goes down and up again, so the link and the framers
//!   start over, and the datagram that started the takeover is not lost;
//! - a peer that is gone is noticed by the link's peer timeout, a new one can take over at any time.
//!
//! There is no authentication: anyone that can reach the port can take over. Nothing is retransmitted either, see the
//! UDP transport docs for what happens on loss.
//!
//! [UdpSink] and [UdpSource] share the socket and the current peer through a [UdpConnection], datagram sockets are
//! used through `&self`, so no locking is needed. With the `embassy-net` feature, [EmbassyNetUdpSocket] implements
//! [DatagramSocket] on an `embassy-net` UDP socket, [udp_server] puts everything together.

use core::cell::Cell;
use core::future::Future;

use wire_weaver::shrink_wrap::BufReader;
use ww_framer::traits::{Head, MessageKind};
use ww_link::{Kind, UDP_MAX_DATAGRAM_LEN, UdpChecksum, UdpHead, UdpTail};

use crate::buffer::RxBuffer;
use crate::fmt::{debug, warn};
use crate::server::Clock;
use crate::transport::{FramedRx, FramedTx, PacketSink, PacketSource, RxFramer, TxFramer};
use crate::{LinkConfig, Server};

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

/// Socket shared by [UdpSink] and [UdpSource], together with the peer that the device talks to.
pub struct UdpConnection<S: DatagramSocket> {
    socket: S,
    peer: Cell<Option<S::Addr>>,
}

impl<S: DatagramSocket> UdpConnection<S> {
    pub const fn new(socket: S) -> Self {
        UdpConnection {
            socket,
            peer: Cell::new(None),
        }
    }

    /// Host that the device talks to, if any.
    pub fn peer(&self) -> Option<S::Addr> {
        self.peer.get()
    }

    pub fn socket(&self) -> &S {
        &self.socket
    }
}

/// [PacketSink] sending datagrams to the current peer.
pub struct UdpSink<'a, S: DatagramSocket> {
    conn: &'a UdpConnection<S>,
}

impl<'a, S: DatagramSocket> UdpSink<'a, S> {
    pub fn new(conn: &'a UdpConnection<S>) -> Self {
        UdpSink { conn }
    }
}

impl<S: DatagramSocket> PacketSink for UdpSink<'_, S> {
    type Error = UdpError<S::SendError>;

    async fn write_packet(&mut self, packet: &[u8]) -> Result<(), Self::Error> {
        let peer = self.conn.peer.get().ok_or(UdpError::NoPeer)?;
        self.conn
            .socket
            .send_to(packet, peer)
            .await
            .map_err(UdpError::Socket)
    }
}

/// Longest datagram that can start link setup. The host sends `Nop` and `GetDeviceInfo` alone, 2 bytes each.
const SETUP_DATAGRAM_MAX_LEN: usize = 8;

/// Datagram that starts link setup, kept across the medium going down and up again.
#[derive(Copy, Clone)]
struct SetupDatagram<A> {
    from: A,
    bytes: [u8; SETUP_DATAGRAM_MAX_LEN],
    len: usize,
}

impl<A> SetupDatagram<A> {
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

/// First message in `datagram` is a whole `Nop` or `GetDeviceInfo`.
fn starts_link_setup(datagram: &[u8]) -> bool {
    let mut rd = BufReader::new(datagram);
    matches!(
        UdpHead::read(&mut rd),
        Ok((MessageKind::Full, kind, _)) if kind == Kind::Nop as u8 || kind == Kind::GetDeviceInfo as u8
    )
}

/// [PacketSource] receiving datagrams from the current peer, see the [module docs](self) on how peers are adopted.
pub struct UdpSource<'a, S: DatagramSocket> {
    conn: &'a UdpConnection<S>,
    /// Another host started link setup, adopted in `wait_connected()`
    takeover: Option<SetupDatagram<S::Addr>>,
    /// Setup datagram of the adopted peer, returned by the first `read_packet()`
    replay: Option<SetupDatagram<S::Addr>>,
}

impl<'a, S: DatagramSocket> UdpSource<'a, S> {
    pub fn new(conn: &'a UdpConnection<S>) -> Self {
        UdpSource {
            conn,
            takeover: None,
            replay: None,
        }
    }
}

impl<S: DatagramSocket> PacketSource for UdpSource<'_, S> {
    type Error = UdpError<S::RecvError>;

    /// The host sends datagrams of at most this size.
    fn max_packet_len(&self) -> usize {
        UDP_MAX_DATAGRAM_LEN
    }

    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if let Some(d) = self.replay.take() {
            buf[..d.len].copy_from_slice(&d.bytes[..d.len]);
            return Ok(d.len);
        }
        loop {
            let (len, from) = self
                .conn
                .socket
                .recv_from(buf)
                .await
                .map_err(UdpError::Socket)?;
            if self.conn.peer.get() == Some(from) {
                return Ok(len);
            }
            if let Some(d) = SetupDatagram::new(&buf[..len], from) {
                debug!("udp: another host started link setup");
                self.takeover = Some(d);
                return Err(UdpError::PeerChanged);
            }
            // e.g., a host from a previous session that did not notice the takeover yet
            warn!("udp: dropping datagram from another host");
        }
    }

    async fn wait_connected(&mut self) {
        loop {
            if let Some(d) = self.takeover.take() {
                self.conn.peer.set(Some(d.from));
                self.replay = Some(d);
                debug!("udp: peer adopted");
                return;
            }
            // one more byte, to tell a truncated datagram from one that fits exactly
            let mut buf = [0u8; SETUP_DATAGRAM_MAX_LEN + 1];
            // an error (e.g., too long for buf) is not a datagram that starts link setup either
            if let Ok((len, from)) = self.conn.socket.recv_from(&mut buf).await {
                self.takeover = SetupDatagram::new(&buf[..len.min(buf.len())], from);
            }
        }
    }
}

/// Fails to compile if the UDP framer configuration in `ww_link` ever diverges from the USB one used by [FramedTx]
/// and [FramedRx].
#[allow(dead_code)]
fn framer_config_is_the_same<'a>(
    tx: TxFramer<'a>,
    rx: RxFramer<'a>,
) -> (
    ww_framer::Tx<'a, UdpHead, UdpChecksum, UdpTail>,
    ww_framer::FramedRx<'a, UdpHead, UdpChecksum, UdpTail>,
) {
    (tx, rx)
}

pub type UdpTx<'a, S> = FramedTx<'a, UdpSink<'a, S>>;
pub type UdpRx<'a, S> = FramedRx<'a, UdpSource<'a, S>>;
/// [UdpTx] / [UdpRx] server, see [Server] on how to use it.
pub type UdpServer<'a, S, C, M = ()> = Server<'a, UdpTx<'a, S>, UdpRx<'a, S>, C, M>;

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

/// UDP server on a [DatagramSocket], e.g.:
/// ```ignore
/// let socket = EmbassyNetUdpSocket::bind(UdpSocket::new(stack, rx_meta, rx_buf, tx_meta, tx_buf), 9000).unwrap();
/// let conn = UDP_CONNECTION.init(UdpConnection::new(socket));
/// let mut server = udp_server(link_config, conn, EmbassyClock, UDP_BUFFERS.init(UdpBuffers::new()));
/// server.run(&mut state).await;
/// ```
pub fn udp_server<'a, const MAX_MESSAGE_LEN: usize, S: DatagramSocket, C: Clock>(
    link_config: LinkConfig<'a>,
    conn: &'a UdpConnection<S>,
    clock: C,
    buffers: &'a mut UdpBuffers<MAX_MESSAGE_LEN>,
) -> UdpServer<'a, S, C> {
    Server::new(
        link_config,
        FramedTx::new(UdpSink::new(conn), &mut buffers.tx),
        FramedRx::new(
            UdpSource::new(conn),
            buffers.rx.assembly_buf(UDP_MAX_DATAGRAM_LEN),
        ),
        clock,
        &mut buffers.scratch,
        &mut buffers.event_scratch,
    )
}

#[cfg(feature = "embassy-net")]
pub use embassy::EmbassyNetUdpSocket;

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
mod tests {
    extern crate std;
    use core::cell::RefCell;
    use core::task::Poll;
    use std::collections::VecDeque;
    use std::vec::Vec;

    use embassy_futures::{block_on, poll_once};

    use super::*;
    use crate::transport::{MessageRx, MessageTx};

    /// Datagrams to be received `(from, bytes)`, and sent ones `(to, bytes)`. Receiving with nothing queued stays
    /// pending.
    #[derive(Default)]
    struct MockSocket {
        input: RefCell<VecDeque<(u8, Vec<u8>)>>,
        output: RefCell<Vec<(u8, Vec<u8>)>>,
    }

    impl DatagramSocket for MockSocket {
        type Addr = u8;
        type RecvError = ();
        type SendError = ();

        async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, u8), ()> {
            let (from, d) = core::future::poll_fn(|_| match self.input.borrow_mut().pop_front() {
                Some(d) => Poll::Ready(d),
                None => Poll::Pending,
            })
            .await;
            // truncate, as Unix sockets do
            let len = d.len().min(buf.len());
            buf[..len].copy_from_slice(&d[..len]);
            Ok((len, from))
        }

        async fn send_to(&self, datagram: &[u8], addr: u8) -> Result<(), ()> {
            self.output.borrow_mut().push((addr, datagram.to_vec()));
            Ok(())
        }
    }

    /// One datagram with the given messages, as the host would send them
    fn datagram(messages: &[(Kind, &[u8])]) -> Vec<u8> {
        let mut buf = [0u8; UDP_MAX_DATAGRAM_LEN];
        let mut tx = TxFramer::new(&mut buf);
        for (kind, m) in messages {
            assert!(tx.write(*kind as u8, m).unwrap());
        }
        let len = tx.flush();
        tx.buf()[..len].to_vec()
    }

    fn push(conn: &UdpConnection<MockSocket>, from: u8, d: Vec<u8>) {
        conn.socket.input.borrow_mut().push_back((from, d));
    }

    struct Rx<'a> {
        rx: UdpRx<'a, MockSocket>,
    }

    impl Rx<'_> {
        /// What the server does: wait for the medium if down, then receive until nothing is left
        fn messages(&mut self) -> Result<Vec<(u8, Vec<u8>)>, UdpError<()>> {
            let mut received = Vec::new();
            loop {
                match poll_once(self.rx.wait_message()) {
                    Poll::Ready(Ok(())) => {
                        let (kind, m) = self.rx.message().unwrap();
                        received.push((kind, m.to_vec()));
                        self.rx.consume();
                    }
                    Poll::Ready(Err(crate::transport::RxError::Transport(e))) => return Err(e),
                    Poll::Pending => return Ok(received),
                }
            }
        }

        fn connect(&mut self) {
            block_on(self.rx.wait_connected());
            self.rx.reset();
        }
    }

    const NOP: (Kind, &[u8]) = (Kind::Nop, &[]);
    const GET_INFO: (Kind, &[u8]) = (Kind::GetDeviceInfo, &[]);
    const DATA: (Kind, &[u8]) = (Kind::Data0, &[1, 2, 3]);

    #[test]
    fn setup_datagrams_are_tiny() {
        assert!(datagram(&[NOP]).len() <= SETUP_DATAGRAM_MAX_LEN);
        assert!(datagram(&[GET_INFO]).len() <= SETUP_DATAGRAM_MAX_LEN);
        assert!(starts_link_setup(&datagram(&[NOP])));
        assert!(starts_link_setup(&datagram(&[GET_INFO])));
        assert!(!starts_link_setup(&datagram(&[DATA])));
        assert!(!starts_link_setup(&datagram(&[(Kind::Ping, &[])])));
        assert!(!starts_link_setup(&[]));
    }

    #[test]
    fn first_host_to_start_link_setup_is_adopted() {
        let conn = UdpConnection::new(MockSocket::default());
        let mut buf = RxBuffer::<UDP_MAX_DATAGRAM_LEN, 64>::new();
        let mut rx = Rx {
            rx: FramedRx::new(
                UdpSource::new(&conn),
                buf.assembly_buf(UDP_MAX_DATAGRAM_LEN),
            ),
        };

        // stray data and a datagram too long to be a setup one are ignored while waiting
        push(&conn, 1, datagram(&[DATA]));
        push(&conn, 1, datagram(&[NOP, DATA, DATA]));
        push(&conn, 2, datagram(&[GET_INFO]));
        push(&conn, 2, datagram(&[DATA]));
        assert!(poll_once(rx.rx.wait_connected()).is_ready());
        rx.rx.reset();
        assert_eq!(conn.peer(), Some(2));
        // GetDeviceInfo that started it is not lost
        assert_eq!(
            rx.messages().unwrap(),
            [(4, std::vec![]), (0, std::vec![1, 2, 3])]
        );

        // replies go to the peer only
        let mut tx_buf = [0u8; UDP_MAX_DATAGRAM_LEN];
        let mut tx = FramedTx::new(UdpSink::new(&conn), &mut tx_buf);
        block_on(tx.write_message(Kind::DeviceInfo as u8, &[7])).unwrap();
        block_on(tx.flush()).unwrap();
        let out = conn.socket.output.borrow();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, 2);
    }

    #[test]
    fn another_host_takes_over_with_link_setup() {
        let conn = UdpConnection::new(MockSocket::default());
        let mut buf = RxBuffer::<UDP_MAX_DATAGRAM_LEN, 64>::new();
        let mut rx = Rx {
            rx: FramedRx::new(
                UdpSource::new(&conn),
                buf.assembly_buf(UDP_MAX_DATAGRAM_LEN),
            ),
        };
        push(&conn, 1, datagram(&[NOP]));
        rx.connect();
        assert_eq!(rx.messages().unwrap(), [(3, std::vec![])]);

        // data from someone else is dropped, the peer stays
        push(&conn, 2, datagram(&[DATA]));
        push(&conn, 1, datagram(&[DATA]));
        assert_eq!(rx.messages().unwrap(), [(0, std::vec![1, 2, 3])]);

        // link setup from someone else: medium goes down, the new host is adopted, its datagram replayed
        push(&conn, 2, datagram(&[NOP]));
        push(&conn, 2, datagram(&[GET_INFO]));
        assert_eq!(rx.messages(), Err(UdpError::PeerChanged));
        rx.connect();
        assert_eq!(conn.peer(), Some(2));
        assert_eq!(rx.messages().unwrap(), [(3, std::vec![]), (4, std::vec![])]);

        // the previous host is not heard anymore
        push(&conn, 1, datagram(&[DATA]));
        assert_eq!(rx.messages().unwrap(), []);
    }

    #[test]
    fn nothing_is_sent_without_a_peer() {
        let conn = UdpConnection::new(MockSocket::default());
        let mut sink = UdpSink::new(&conn);
        assert_eq!(block_on(sink.write_packet(&[1])), Err(UdpError::NoPeer));
    }
}
