extern crate std;
use core::cell::RefCell;
use core::task::Poll;
use std::collections::VecDeque;
use std::vec::Vec;

use embassy_futures::{block_on, poll_once};

use super::*;
use crate::transport::{MessageRx, MessageTx, TxFramer};

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
            UdpSource::new(&conn),
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
    assert_eq!(conn.peer(), Some(2));
    // Hello that started it is not lost
    assert_eq!(
        rx.messages().unwrap(),
        [(4, HELLO.to_vec()), (0, std::vec![1, 2, 3])]
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
    assert_eq!(conn.peer(), Some(2));
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
            UdpSource::new(&conn),
            buf.assembly_buf(UDP_MAX_DATAGRAM_LEN),
        ),
    };
    let mut gen2 = ww_link::Hello::from_bytes(HELLO).unwrap();
    gen2.generations.framer = 2;
    push(&conn, 1, ww_link::hello::udp::frame(&gen2).to_vec());
    push(&conn, 1, std::vec![0xF7, 0x01, 0x02, 0x03, 0x04]);
    rx.connect();
    assert_eq!(conn.peer(), Some(1));
    assert_eq!(rx.messages().unwrap(), [(4, gen2.to_bytes().to_vec())]);

    // the device's hello: the frozen datagram alone, after what the framer held
    let mut tx_buf = [0u8; UDP_MAX_DATAGRAM_LEN];
    let mut tx = FramedTx::new(UdpSink::new(&conn), &mut tx_buf);
    let device = ww_link::Hello {
        role: ww_link::Role::Device,
        ..gen2
    };
    block_on(tx.write_message(Kind::Nop as u8, &[])).unwrap();
    block_on(tx.write_hello(&device)).unwrap();
    let out = conn.socket.output.borrow().clone();
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
    let mut sink = UdpSink::new(&conn);
    assert_eq!(block_on(sink.write_packet(&[1])), Err(UdpError::NoPeer));
}
