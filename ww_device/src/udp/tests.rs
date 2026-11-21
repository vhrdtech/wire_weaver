extern crate std;
use core::cell::RefCell;
use core::task::Poll;
use std::collections::VecDeque;
use std::vec::Vec;

use embassy_futures::{block_on, poll_once};

use super::*;
use crate::transport::{MessageRx, MessageTx, TxFramer};
use crate::udp::peers::sealed::Table;

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

/// A host's frame counter, so that its datagrams follow each other
#[derive(Default)]
struct Host(Option<ww_framer::tx::TxState>);

impl Host {
    /// One datagram with the given messages, as the host would send them
    fn datagram(&mut self, messages: &[(Kind, &[u8])]) -> Vec<u8> {
        let mut buf = [0u8; UDP_MAX_DATAGRAM_LEN];
        let mut tx = match self.0.take() {
            Some(state) => TxFramer::<UdpFraming>::from_parts(&mut buf, state),
            None => TxFramer::<UdpFraming>::new(&mut buf),
        };
        for (kind, m) in messages {
            assert!(tx.write(*kind as u8, m).unwrap());
        }
        let len = tx.flush();
        let d = tx.buf()[..len].to_vec();
        self.0 = Some(tx.into_parts());
        d
    }
}

/// A datagram of a host that sends only this one
fn datagram(messages: &[(Kind, &[u8])]) -> Vec<u8> {
    Host::default().datagram(messages)
}

fn push<const N: usize, H>(peers: &UdpPeers<MockSocket, N, H>, from: u8, d: Vec<u8>) {
    peers.socket().input.borrow_mut().push_back((from, d));
}

struct Rx<'a, P: Deref<Target: PeerTable>> {
    rx: UdpRx<'a, P>,
}

impl<P: Deref<Target: PeerTable + Table<Socket = MockSocket>>> Rx<'_, P> {
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

    /// One poll of `wait_connected`, as a server waiting on several slots does: true once a host is adopted.
    fn try_connect(&mut self) -> bool {
        let up = poll_once(self.rx.wait_connected()).is_ready();
        if up {
            self.rx.reset();
        }
        up
    }
}

/// The server of one slot of a two-slot table.
fn slot<'a>(peers: &'a Two, slot: usize, buf: &'a mut Buf) -> Rx<'a, &'a Two> {
    Rx {
        rx: FramedRx::new(
            UdpSource::new(peers, slot),
            buf.assembly_buf(UDP_MAX_DATAGRAM_LEN),
        ),
    }
}

type Two = UdpPeers<MockSocket, 2>;
type Buf = RxBuffer<UDP_MAX_DATAGRAM_LEN, 64>;

/// The hosts `sink` of `slot` sends to, after one message.
fn reply_to(peers: &Two, slot: usize) -> Result<u8, UdpError<()>> {
    let mut tx_buf = [0u8; UDP_MAX_DATAGRAM_LEN];
    let mut tx = FramedTx::new(UdpSink::new(peers, slot), &mut tx_buf);
    block_on(tx.write_message(Kind::DeviceInfo as u8, &[7])).unwrap();
    match block_on(tx.flush()) {
        Ok(()) => Ok(peers.socket().output.borrow().last().unwrap().0),
        Err(crate::transport::FramedError::Transport(e)) => Err(e),
        Err(e) => panic!("{e:?}"),
    }
}

const NOP: (Kind, &[u8]) = (Kind::Nop, &[]);
/// A whole hello payload: magic, role, 4 generations, token
const HELLO: &[u8] = &[b'W', b'W', b'H', 4, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];

/// The host's hello datagram, in its frozen frame outside the framer
fn hello() -> Vec<u8> {
    ww_link::hello::udp::frame(&ww_link::Hello::from_bytes(HELLO).unwrap()).to_vec()
}
const DATA: (Kind, &[u8]) = (Kind::Data0, &[1, 2, 3]);

#[test]
fn setup_datagrams_are_tiny() {
    assert!(datagram(&[NOP]).len() <= SETUP_DATAGRAM_MAX_LEN);
    assert!(hello().len() <= SETUP_DATAGRAM_MAX_LEN);
    assert!(starts_link_setup(&datagram(&[NOP])));
    assert!(starts_link_setup(&hello()));
    // a hello through the framer is not how a host starts link setup
    assert!(!starts_link_setup(&datagram(&[(Kind::Hello, HELLO)])));
    assert!(!starts_link_setup(&datagram(&[DATA])));
    assert!(!starts_link_setup(&datagram(&[(Kind::Ping, &[])])));
    assert!(!starts_link_setup(&[]));
    assert!(datagram(&[NOP, DATA, DATA, DATA, DATA]).len() > SETUP_DATAGRAM_MAX_LEN);
    // a flipped bit anywhere (prefix, head, token, CRC) fails the CRC: no takeover by a corrupted datagram
    for good in [datagram(&[NOP]), hello()] {
        for bit in 0..good.len() * 8 {
            let mut bad = good.clone();
            bad[bit / 8] ^= 1 << (bit % 8);
            assert!(!starts_link_setup(&bad), "bit {bit}");
        }
    }
}

#[test]
fn first_host_to_start_link_setup_is_adopted() {
    let conn = UdpConnection::new(MockSocket::default());
    let mut buf = RxBuffer::<UDP_MAX_DATAGRAM_LEN, 64>::new();
    let mut rx = Rx {
        rx: FramedRx::new(
            UdpSource::new(&conn, 0),
            buf.assembly_buf(UDP_MAX_DATAGRAM_LEN),
        ),
    };

    let (mut h1, mut h2) = (Host::default(), Host::default());
    // stray data and a datagram too long to be a setup one are ignored while waiting
    push(&conn, 1, h1.datagram(&[DATA]));
    push(&conn, 1, h1.datagram(&[NOP, DATA, DATA, DATA, DATA]));
    push(&conn, 2, hello());
    push(&conn, 2, h2.datagram(&[DATA]));
    assert!(poll_once(rx.rx.wait_connected()).is_ready());
    rx.rx.reset();
    assert_eq!(conn.peer(0), Some(2));
    // Hello that started it is not lost
    assert_eq!(
        rx.messages().unwrap(),
        [(4, HELLO.to_vec()), (0, std::vec![1, 2, 3])]
    );

    // replies go to the peer only
    let mut tx_buf = [0u8; UDP_MAX_DATAGRAM_LEN];
    let mut tx = FramedTx::new(UdpSink::new(&conn, 0), &mut tx_buf);
    block_on(tx.write_message(Kind::DeviceInfo as u8, &[7])).unwrap();
    block_on(tx.flush()).unwrap();
    let out = conn.socket().output.borrow();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, 2);
}

#[test]
fn another_host_takes_over_with_link_setup() {
    let conn = UdpConnection::new(MockSocket::default());
    let mut buf = RxBuffer::<UDP_MAX_DATAGRAM_LEN, 64>::new();
    let mut rx = Rx {
        rx: FramedRx::new(
            UdpSource::new(&conn, 0),
            buf.assembly_buf(UDP_MAX_DATAGRAM_LEN),
        ),
    };
    let (mut h1, mut h2) = (Host::default(), Host::default());
    push(&conn, 1, h1.datagram(&[NOP]));
    rx.connect();
    assert_eq!(rx.messages().unwrap(), [(3, std::vec![])]);

    // data from someone else is dropped, the peer stays
    push(&conn, 2, h2.datagram(&[DATA]));
    push(&conn, 1, h1.datagram(&[DATA]));
    assert_eq!(rx.messages().unwrap(), [(0, std::vec![1, 2, 3])]);

    // link setup from someone else: medium goes down, the new host is adopted, its datagram replayed
    push(&conn, 2, h2.datagram(&[NOP]));
    push(&conn, 2, hello());
    assert_eq!(rx.messages(), Err(UdpError::PeerChanged));
    rx.connect();
    assert_eq!(conn.peer(0), Some(2));
    assert_eq!(
        rx.messages().unwrap(),
        [(3, std::vec![]), (4, HELLO.to_vec())]
    );

    // the previous host is not heard anymore
    push(&conn, 1, h1.datagram(&[DATA]));
    assert_eq!(rx.messages().unwrap(), []);
}

/// The hello goes outside the framer both ways (EVO-23): a host whose framer is of another generation (a made-up
/// generation 2 datagram after its hello) still has its hello read, and nothing else; the device's hello goes
/// out as the frozen datagram, without the frame counter. A hello after half a split message of an earlier session
/// starts over: the framer drops what it held.
#[test]
fn hello_outside_the_framer() {
    let conn = UdpConnection::new(MockSocket::default());
    let mut buf = RxBuffer::<UDP_MAX_DATAGRAM_LEN, 256>::new();
    let mut rx = Rx {
        rx: FramedRx::new(
            UdpSource::new(&conn, 0),
            buf.assembly_buf(UDP_MAX_DATAGRAM_LEN),
        ),
    };
    let mut gen2 = ww_link::Hello::from_bytes(HELLO).unwrap();
    gen2.generations.framer = 2;
    push(&conn, 1, ww_link::hello::udp::frame(&gen2).to_vec());
    push(&conn, 1, std::vec![0xF7, 0x01, 0x02, 0x03, 0x04]);
    rx.connect();
    assert_eq!(conn.peer(0), Some(1));
    assert_eq!(rx.messages().unwrap(), [(4, gen2.to_bytes().to_vec())]);

    // the device's hello: the frozen datagram alone, after what the framer held
    let mut tx_buf = [0u8; UDP_MAX_DATAGRAM_LEN];
    let mut tx = FramedTx::new(UdpSink::new(&conn, 0), &mut tx_buf);
    let device = ww_link::Hello {
        role: ww_link::Role::Device,
        ..gen2
    };
    block_on(tx.write_message(Kind::Nop as u8, &[])).unwrap();
    block_on(tx.write_hello(&device)).unwrap();
    let out = conn.socket().output.borrow().clone();
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].1, ww_link::hello::udp::frame(&device));

    // half a split message, then a hello of this build: only the hello and what follows it
    let mut host = Host::default();
    let mut split_buf = [0u8; 64];
    let mut split = TxFramer::<UdpFraming>::new(&mut split_buf);
    assert_eq!(split.write(Kind::Data0 as u8, &[7u8; 100]), Ok(false));
    let len = split.flush();
    push(&conn, 1, split.buf()[..len].to_vec());
    push(&conn, 1, hello());
    push(&conn, 1, host.datagram(&[DATA]));
    assert_eq!(
        rx.messages().unwrap(),
        [(4, HELLO.to_vec()), (0, std::vec![1, 2, 3])]
    );
}

#[test]
fn nothing_is_sent_without_a_peer() {
    let conn = UdpConnection::new(MockSocket::default());
    let mut sink = UdpSink::new(&conn, 0);
    assert_eq!(block_on(sink.write_packet(&[1])), Err(UdpError::NoPeer));
}

/// Two fixed slots: each host that starts link setup gets one, and its datagrams reach that slot only, whichever
/// slot received them from the socket.
#[test]
fn each_host_gets_its_own_slot() {
    let peers = Two::new(MockSocket::default());
    let (mut b0, mut b1) = (Buf::new(), Buf::new());
    let mut rx0 = slot(&peers, 0, &mut b0);
    let mut rx1 = slot(&peers, 1, &mut b1);
    let (mut h1, mut h2) = (Host::default(), Host::default());

    push(&peers, 1, h1.datagram(&[NOP]));
    push(&peers, 2, hello());
    // a host without a slot is not heard
    push(&peers, 3, datagram(&[DATA]));
    push(&peers, 2, h2.datagram(&[DATA]));
    push(&peers, 1, h1.datagram(&[DATA]));
    // a slot served for good is up without a host: its reads adopt the first one
    assert!(rx0.try_connect());
    assert!(rx1.try_connect());
    assert_eq!((peers.peer(0), peers.peer(1)), (None, None));

    // slot 0 adopts host 1, then receives host 2's hello and holds it for slot 1: nothing more is received until
    // slot 1 takes it
    assert_eq!(rx0.messages().unwrap(), [(3, std::vec![])]);
    assert_eq!((peers.peer(0), peers.peer(1)), (Some(1), None));
    assert_eq!(
        rx1.messages().unwrap(),
        [(4, HELLO.to_vec()), (0, std::vec![1, 2, 3])]
    );
    assert_eq!(rx0.messages().unwrap(), [(0, std::vec![1, 2, 3])]);
    assert_eq!((peers.peer(0), peers.peer(1)), (Some(1), Some(2)));
    assert_eq!(peers.socket().input.borrow().len(), 0);

    assert_eq!(reply_to(&peers, 0), Ok(1));
    assert_eq!(reply_to(&peers, 1), Ok(2));
}

/// With every fixed slot taken, a new host takes over the slot heard from least recently; the other stays.
#[test]
fn new_host_takes_the_quietest_slot() {
    let peers = Two::new(MockSocket::default());
    let (mut b0, mut b1) = (Buf::new(), Buf::new());
    let mut rx0 = slot(&peers, 0, &mut b0);
    let mut rx1 = slot(&peers, 1, &mut b1);
    let (mut h1, mut h2, mut h3) = (Host::default(), Host::default(), Host::default());
    push(&peers, 1, h1.datagram(&[NOP]));
    push(&peers, 2, h2.datagram(&[NOP]));
    assert!(rx0.try_connect() && rx1.try_connect());
    assert_eq!(rx0.messages().unwrap().len(), 1);
    assert_eq!(rx1.messages().unwrap().len(), 1);

    // host 1 was heard last: host 3 takes slot 1
    push(&peers, 2, h2.datagram(&[DATA]));
    push(&peers, 1, h1.datagram(&[DATA]));
    push(&peers, 3, h3.datagram(&[NOP]));
    assert_eq!(rx1.messages().unwrap().len(), 1);
    assert_eq!(rx0.messages().unwrap().len(), 1);
    assert_eq!(rx1.messages(), Err(UdpError::PeerChanged));
    assert!(rx1.try_connect());
    assert_eq!(peers.peer(1), Some(3));
    assert_eq!(rx1.messages().unwrap(), [(3, std::vec![])]);

    // host 2 is not heard anymore, host 1 still is
    push(&peers, 2, h2.datagram(&[DATA]));
    push(&peers, 1, h1.datagram(&[DATA]));
    assert_eq!(rx1.messages().unwrap(), []);
    assert_eq!(rx0.messages().unwrap(), [(0, std::vec![1, 2, 3])]);
    assert_eq!(peers.peer(0), Some(1));
}

/// A receive dropped half way (`wait()` lost a select) gives the socket up: another slot receives next.
#[test]
fn cancelled_receive_gives_the_socket_up() {
    let peers = Two::new(MockSocket::default());
    let (mut b0, mut b1) = (Buf::new(), Buf::new());
    let mut rx0 = slot(&peers, 0, &mut b0);
    let mut rx1 = slot(&peers, 1, &mut b1);
    assert!(rx0.try_connect() && rx1.try_connect());
    // nothing to receive: slot 0's wait starts a receive, which is dropped with the poll
    assert_eq!(rx0.messages().unwrap(), []);
    push(&peers, 2, hello());
    assert_eq!(rx1.messages().unwrap(), [(4, HELLO.to_vec())]);
    assert_eq!(peers.peer(1), Some(2));
}

/// Accepted connections: one per host, a dropped source frees its slot, and with every slot taken a new host ends
/// the quietest connection and gets the slot from its next hello.
#[test]
fn accepted_connections() {
    let peers = Two::new(MockSocket::default());
    let (mut b0, mut b1) = (Buf::new(), Buf::new());
    let (mut h1, mut h2) = (Host::default(), Host::default());
    assert!(poll_once(peers.accept(&mut [0u8; UDP_MAX_DATAGRAM_LEN])).is_pending());

    push(&peers, 1, h1.datagram(&[NOP]));
    push(&peers, 2, hello());
    let Poll::Ready(Ok((s1, 1))) = poll_once(peers.accept(&mut [0u8; UDP_MAX_DATAGRAM_LEN])) else {
        panic!("host 1 accepted")
    };
    // hosts don't wait for each other: host 2 is accepted before host 1's connection adopts host 1
    let Poll::Ready(Ok((s2, 2))) = poll_once(peers.accept(&mut [0u8; UDP_MAX_DATAGRAM_LEN])) else {
        panic!("host 2 accepted")
    };
    assert_ne!(s1, s2);
    let mut rx1 = slot(&peers, s1, &mut b0);
    assert!(rx1.try_connect());
    assert_eq!(peers.peer(s1), Some(1));
    let mut rx2 = slot(&peers, s2, &mut b1);
    assert!(rx2.try_connect());
    assert_eq!(rx1.messages().unwrap(), [(3, std::vec![])]);
    assert_eq!(rx2.messages().unwrap(), [(4, HELLO.to_vec())]);

    // host 3 ends host 1's connection (heard from least recently), its hello is dropped
    push(&peers, 2, h2.datagram(&[DATA]));
    push(&peers, 3, hello());
    assert!(poll_once(peers.accept(&mut [0u8; UDP_MAX_DATAGRAM_LEN])).is_pending());
    assert_eq!(rx2.messages().unwrap(), [(0, std::vec![1, 2, 3])]);
    assert!(poll_once(peers.accept(&mut [0u8; UDP_MAX_DATAGRAM_LEN])).is_pending());
    assert_eq!(rx1.messages(), Err(UdpError::PeerChanged));
    assert!(!rx1.try_connect());
    assert_eq!(reply_to(&peers, s1), Err(UdpError::PeerChanged));
    // host 3 tries again before that connection is dropped: no other connection is ended for it
    push(&peers, 3, hello());
    push(&peers, 2, h2.datagram(&[DATA]));
    assert_eq!(rx2.messages().unwrap(), [(0, std::vec![1, 2, 3])]);
    assert_eq!(peers.peer(s2), Some(2));

    // dropping the connection frees the slot: host 3's next hello gets it
    drop(rx1);
    push(&peers, 3, hello());
    assert_eq!(
        poll_once(peers.accept(&mut [0u8; UDP_MAX_DATAGRAM_LEN])),
        Poll::Ready(Ok((s1, 3)))
    );
    let mut rx3 = slot(&peers, s1, &mut b0);
    assert!(rx3.try_connect());
    assert_eq!(peers.peer(s1), Some(3));
    assert_eq!(peers.peer(s2), Some(2));
}

/// A host given a slot that nobody accepts yet (a server at its connection limit) holds up no connection; a newer
/// host takes its place, and is the one accepted.
#[test]
fn a_host_waiting_for_accept_holds_up_nobody() {
    let peers = Two::new(MockSocket::default());
    let mut buf = Buf::new();
    let mut accept_buf = [0u8; UDP_MAX_DATAGRAM_LEN];
    let mut h1 = Host::default();
    push(&peers, 1, h1.datagram(&[NOP]));
    let Poll::Ready(Ok((s1, 1))) = poll_once(peers.accept(&mut accept_buf)) else {
        panic!("host 1 accepted")
    };
    let mut rx1 = slot(&peers, s1, &mut buf);
    assert!(rx1.try_connect());
    assert_eq!(rx1.messages().unwrap(), [(3, std::vec![])]);

    push(&peers, 2, hello());
    push(&peers, 1, h1.datagram(&[DATA]));
    push(&peers, 3, hello());
    push(&peers, 1, h1.datagram(&[DATA]));
    assert_eq!(
        rx1.messages().unwrap(),
        [(0, std::vec![1, 2, 3]), (0, std::vec![1, 2, 3])]
    );
    let Poll::Ready(Ok((s3, 3))) = poll_once(peers.accept(&mut accept_buf)) else {
        panic!("host 3 accepted")
    };
    assert_ne!(s1, s3);
}

/// Once the server accepts no more hosts, a new host ends no connection: the connections accepted go on.
#[test]
fn a_table_that_stopped_accepting_ends_no_connection_for_a_new_host() {
    let peers = Two::new(MockSocket::default());
    let (mut b0, mut b1) = (Buf::new(), Buf::new());
    let mut accept_buf = [0u8; UDP_MAX_DATAGRAM_LEN];
    let (mut h1, mut h2) = (Host::default(), Host::default());
    push(&peers, 1, h1.datagram(&[NOP]));
    push(&peers, 2, h2.datagram(&[NOP]));
    let Poll::Ready(Ok((s1, 1))) = poll_once(peers.accept(&mut accept_buf)) else {
        panic!("host 1 accepted")
    };
    let Poll::Ready(Ok((s2, 2))) = poll_once(peers.accept(&mut accept_buf)) else {
        panic!("host 2 accepted")
    };
    let (mut rx1, mut rx2) = (slot(&peers, s1, &mut b0), slot(&peers, s2, &mut b1));
    assert!(rx1.try_connect() && rx2.try_connect());
    assert_eq!(rx1.messages().unwrap(), [(3, std::vec![])]);
    assert_eq!(rx2.messages().unwrap(), [(3, std::vec![])]);
    peers.stop_accepting();

    // host 1 is heard from least recently: a server still accepting would end its connection for host 3
    push(&peers, 2, h2.datagram(&[DATA]));
    push(&peers, 3, hello());
    push(&peers, 1, h1.datagram(&[DATA]));
    assert_eq!(rx2.messages().unwrap(), [(0, std::vec![1, 2, 3])]);
    assert_eq!(rx1.messages().unwrap(), [(0, std::vec![1, 2, 3])]);
    assert_eq!((peers.peer(s1), peers.peer(s2)), (Some(1), Some(2)));
    assert_eq!(reply_to(&peers, s1), Ok(1));
}

/// A table on a computer: owned framers over a shared locked table, the way a host server's connections use
/// it, and everything they hold or wait on is `Send`, so they run on any thread.
#[cfg(feature = "std")]
#[test]
fn owned_framers_on_a_locked_table() {
    use std::sync::Arc;

    use crate::transport::{OwnedFramedRx, OwnedFramedTx};

    let peers = std::rc::Rc::new(LockedUdpPeers::<MockSocket, 2>::new(MockSocket::default()));
    let mut h1 = Host::default();
    push(&*peers, 1, h1.datagram(&[NOP]));
    push(&*peers, 1, h1.datagram(&[DATA]));
    let mut accept_buf = [0u8; UDP_MAX_DATAGRAM_LEN];
    let Poll::Ready(Ok((slot, 1))) = poll_once(peers.accept(&mut accept_buf)) else {
        panic!("host 1 accepted")
    };
    let mut rx = OwnedFramedRx::owned(UdpSource::new(peers.clone(), slot), 64);
    let mut tx = OwnedFramedTx::owned(UdpSink::new(peers.clone(), slot), UDP_MAX_DATAGRAM_LEN);
    assert_eq!(rx.max_message_len(), 64);
    assert!(poll_once(rx.wait_connected()).is_ready());
    let mut got = Vec::new();
    while let Poll::Ready(Ok(())) = poll_once(rx.wait_message()) {
        let (kind, m) = rx.message().unwrap();
        got.push((kind, m.to_vec()));
        rx.consume();
    }
    assert_eq!(got, [(3, std::vec![]), (0, std::vec![1, 2, 3])]);
    block_on(tx.write_message(Kind::DeviceInfo as u8, &[7])).unwrap();
    block_on(tx.flush()).unwrap();
    assert_eq!(peers.socket().output.borrow()[0].0, 1);

    // the slot is free again once the source is dropped
    drop(rx);
    assert_eq!(peers.peer(slot), None);

    fn send<T: Send>(_: &T) {}
    fn sync<T: Sync>() {}
    type Std = LockedUdpPeers<StdUdpSocket<NoReadiness>, 4>;
    sync::<Std>();
    let table = Arc::new(Std::new(
        StdUdpSocket::new(
            std::net::UdpSocket::bind("127.0.0.1:0").unwrap(),
            NoReadiness,
        )
        .unwrap(),
    ));
    let mut rx = OwnedFramedRx::owned(UdpSource::new(table.clone(), 0), 64);
    send(&rx);
    send(&rx.wait_message());
    send(&rx.wait_connected());
    send(&OwnedFramedTx::owned(
        UdpSink::new(table.clone(), 0),
        UDP_MAX_DATAGRAM_LEN,
    ));
    send(&table.accept(&mut accept_buf));
}

/// Readiness that never comes, for the `Send` checks.
#[cfg(feature = "std")]
struct NoReadiness;

#[cfg(feature = "std")]
impl Readiness for NoReadiness {
    async fn readable(&self) -> std::io::Result<()> {
        core::future::pending().await
    }
}
