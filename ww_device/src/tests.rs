//! Link core driven by hand with hand-advanced time, and the blocking server against a host
//! simulated with a plain framer. The async server is exercised end-to-end against the real host
//! event loop in `wire_weaver_client`.

extern crate std;
use std::vec;
use std::vec::Vec;

use core::time::Duration;

use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{
    DeserializeShrinkWrap, Error as ShrinkWrapError, SerializeShrinkWrap,
};
use wire_weaver::{MessageSink, WireWeaverApiBackend};
use ww_client_server::{ErrorKind, Event, EventKind};
use ww_link::{
    ApiHashPair, CompactVersion, DisconnectReason, FullVersion, Kind, LinkSetup, Message,
    PEER_TIMEOUT_MS, PING_INTERVAL_MS,
};
use ww_version::Version;

use crate::blocking;
use crate::link::{
    DeviceLink, DownReason, LinkConfig, LinkEvent, Phase, Received, SendError, Transmit,
};
use crate::transport::{RxFramer, TxFramer};
use crate::{GENERIC_ERROR_SEQ, Instant, generic_error_reply, loopback_reply, loopback_seqs};

const PING: Duration = Duration::from_millis(PING_INTERVAL_MS);
const PEER_TIMEOUT: Duration = Duration::from_millis(PEER_TIMEOUT_MS);
const ACC: Duration = Duration::from_millis(1);

fn config() -> LinkConfig<'static> {
    LinkConfig::new(
        FullVersion::new("test_api", Version::new(0, 3, 1)),
        ApiHashPair::empty(),
        CompactVersion::new(ww_global_gid(), 0, 2, 0),
    )
}

fn ww_global_gid() -> ww_version::GlobalTypeId {
    ww_version::GlobalTypeId::new(513)
}

/// Transmits in a form that is easy to compare: decoded kind, or None for Flush
#[derive(Debug, PartialEq)]
enum Tx {
    Msg(Kind, Vec<u8>),
    Flush,
}

fn drain(link: &mut DeviceLink<'_>, now: Instant) -> Vec<Tx> {
    let mut scratch = [0u8; 256];
    let mut out = vec![];
    while let Some(t) = link.poll_transmit(now, &mut scratch) {
        out.push(match t {
            Transmit::Message { kind, bytes } => {
                Tx::Msg(Kind::from_repr(kind).unwrap(), bytes.to_vec())
            }
            Transmit::Flush => Tx::Flush,
        });
    }
    out
}

fn kinds(tx: &[Tx]) -> Vec<Option<Kind>> {
    tx.iter()
        .map(|t| match t {
            Tx::Msg(k, _) => Some(*k),
            Tx::Flush => None,
        })
        .collect()
}

fn feed<'m>(
    link: &mut DeviceLink<'_>,
    now: Instant,
    msg: &Message<'_>,
    scratch: &'m mut [u8],
) -> Option<Received<'m>> {
    let (kind, payload) = msg.encode(scratch).unwrap();
    // payload may borrow msg for Data, copy into the tail of scratch to get a 'm lifetime
    let len = payload.len();
    let copy: Vec<u8> = payload.to_vec();
    scratch[..len].copy_from_slice(&copy);
    link.handle_message(now, kind, &scratch[..len])
}

fn link_setup(host_version: FullVersion<'_>) -> Message<'_> {
    Message::LinkSetup(LinkSetup {
        host_user_version: host_version,
        host_max_message_len: 1000,
    })
}

/// Link up at `now`, returns the transmits of link setup.
fn connect(link: &mut DeviceLink<'_>, now: Instant) -> Vec<Tx> {
    let mut scratch = [0u8; 256];
    link.on_transport_up(now);
    assert!(feed(link, now, &Message::GetDeviceInfo, &mut scratch).is_none());
    let mut sent = drain(link, now);
    let host = FullVersion::new("test_api", Version::new(0, 3, 0));
    assert!(feed(link, now, &link_setup(host), &mut scratch).is_none());
    sent.extend(drain(link, now));
    assert_eq!(link.poll_event(), Some(LinkEvent::Up));
    sent
}

#[test]
fn link_setup_sequence() {
    let now = Instant::from_millis(100);
    let mut link = DeviceLink::new(config(), 512);
    assert_eq!(link.phase(), Phase::Down);
    let sent = connect(&mut link, now);
    assert_eq!(
        kinds(&sent),
        [
            Some(Kind::Nop),
            None,
            Some(Kind::DeviceInfo),
            None,
            Some(Kind::LinkReady),
            None
        ],
        "Nop is flushed alone, control messages are flushed right away"
    );
    let Tx::Msg(_, info) = &sent[2] else { panic!() };
    let Message::DeviceInfo(info) = Message::decode(Kind::DeviceInfo as u8, info).unwrap() else {
        panic!()
    };
    assert_eq!(info.dev_max_message_len, 512);
    assert_eq!(info.packet_accumulation_time_us, 1000);
    assert_eq!(info.dev_link_version, ww_link::LINK_VERSION);
    assert_eq!(info.user_api_version.crate_id, "test_api");
    assert!(link.is_up());
    assert_eq!(link.remote_max_message_len(), 1000);
    assert_eq!(link.poll_timeout(), Some(now + PING));
}

#[test]
fn data_accumulation_ping_and_peer_timeout() {
    let t0 = Instant::from_millis(100);
    let mut link = DeviceLink::new(config(), 512);
    connect(&mut link, t0);
    let mut scratch = [0u8; 64];

    let r = feed(
        &mut link,
        t0,
        &Message::Data {
            channel: 0,
            bytes: &[1, 2, 3],
        },
        &mut scratch,
    );
    assert!(matches!(r, Some(Received::Data(&[1, 2, 3]))));

    // reply written into the framer: flushed after the accumulation window, not before
    link.check_send(10).unwrap();
    link.on_data_written(t0);
    assert_eq!(link.poll_timeout(), Some(t0 + ACC));
    link.handle_timeout(t0 + ACC / 2);
    assert!(drain(&mut link, t0 + ACC / 2).is_empty());
    let t1 = t0 + ACC;
    link.handle_timeout(t1);
    assert_eq!(drain(&mut link, t1), [Tx::Flush]);
    assert_eq!(
        link.poll_timeout(),
        Some(t1 + PING),
        "flush restarts the ping timer"
    );

    let t2 = t1 + PING;
    link.handle_timeout(t2);
    assert_eq!(kinds(&drain(&mut link, t2)), [Some(Kind::Ping), None]);

    // peer timeout counts from the last received message, the next ping comes first
    assert_eq!(link.poll_timeout(), Some(t2 + PING));
    assert!(t2 + PING < t0 + PEER_TIMEOUT);
    link.handle_timeout(t0 + PEER_TIMEOUT);
    assert_eq!(
        link.poll_event(),
        Some(LinkEvent::Down(DownReason::PeerTimeout))
    );
    assert_eq!(link.phase(), Phase::Idle);
    assert_eq!(link.poll_timeout(), None);
    assert_eq!(link.check_send(1), Err(SendError::NotConnected));
}

#[test]
fn incompatible_and_dynamic_hosts() {
    let now = Instant::from_millis(1);
    let mut link = DeviceLink::new(config(), 512);
    link.on_transport_up(now);
    let mut scratch = [0u8; 256];

    let other_minor = FullVersion::new("test_api", Version::new(0, 4, 0));
    assert!(feed(&mut link, now, &link_setup(other_minor), &mut scratch).is_none());
    let sent = drain(&mut link, now);
    assert_eq!(kinds(&sent), [Some(Kind::Disconnect), None]);
    let Tx::Msg(_, reason) = &sent[0] else {
        panic!()
    };
    assert!(matches!(
        Message::decode(Kind::Disconnect as u8, reason).unwrap(),
        Message::Disconnect(DisconnectReason::IncompatibleVersion)
    ));
    assert_eq!(link.poll_event(), None);
    assert!(!link.is_up());

    let other_crate = FullVersion::new("other_api", Version::new(0, 3, 0));
    feed(&mut link, now, &link_setup(other_crate), &mut scratch);
    assert!(!link.is_up());
    drain(&mut link, now);

    // host without generated client code, working through introspection
    let dynamic = FullVersion::new("", Version::new(0, 1, 0));
    feed(&mut link, now, &link_setup(dynamic), &mut scratch);
    assert!(link.is_up());
    assert_eq!(kinds(&drain(&mut link, now)), [Some(Kind::LinkReady), None]);
}

#[test]
fn stragglers_and_session_changes() {
    let now = Instant::from_millis(1);
    let mut link = DeviceLink::new(config(), 512);
    let mut scratch = [0u8; 256];

    // transport down: nothing is processed
    assert!(feed(&mut link, now, &Message::GetDeviceInfo, &mut scratch).is_none());
    assert!(drain(&mut link, now).is_empty());

    link.on_transport_up(now);
    // data and Disconnect from a previous session are ignored
    assert!(
        feed(
            &mut link,
            now,
            &Message::Data {
                channel: 0,
                bytes: &[1]
            },
            &mut scratch
        )
        .is_none()
    );
    feed(
        &mut link,
        now,
        &Message::Disconnect(DisconnectReason::RequestByUser),
        &mut scratch,
    );
    assert!(drain(&mut link, now).is_empty());
    assert_eq!(link.poll_event(), None);

    connect(&mut link, now);
    // host application restarted without disconnecting
    feed(&mut link, now, &Message::GetDeviceInfo, &mut scratch);
    assert_eq!(
        link.poll_event(),
        Some(LinkEvent::Down(DownReason::NewSession))
    );
    assert_eq!(
        kinds(&drain(&mut link, now)),
        [Some(Kind::Nop), None, Some(Kind::DeviceInfo), None]
    );

    connect(&mut link, now);
    feed(
        &mut link,
        now,
        &Message::Disconnect(DisconnectReason::RequestByUser),
        &mut scratch,
    );
    assert_eq!(
        link.poll_event(),
        Some(LinkEvent::Down(DownReason::Disconnect(
            DisconnectReason::RequestByUser
        )))
    );
    assert!(
        drain(&mut link, now).is_empty(),
        "no Disconnect back to a host that disconnected"
    );

    connect(&mut link, now);
    link.disconnect(DisconnectReason::ApplicationCrash);
    assert_eq!(
        link.poll_event(),
        Some(LinkEvent::Down(DownReason::Local(
            DisconnectReason::ApplicationCrash
        )))
    );
    assert_eq!(
        kinds(&drain(&mut link, now)),
        [Some(Kind::Disconnect), None]
    );

    connect(&mut link, now);
    link.on_transport_down();
    assert_eq!(
        link.poll_event(),
        Some(LinkEvent::Down(DownReason::Transport))
    );
    assert_eq!(link.phase(), Phase::Down);
}

#[test]
fn generic_error() {
    let mut scratch = [0u8; 64];
    let reply = generic_error_reply(&[0x34, 0x12, 0xAA], &mut scratch).unwrap();
    let event = Event::from_ww_bytes(reply).unwrap();
    assert_eq!(event.seq, 0x1234);
    let err = event.result.unwrap_err();
    let s = std::format!("{err:?}");
    assert!(s.contains("ResponseSerFailed"), "{s}");
    assert!(s.contains(&std::format!("{GENERIC_ERROR_SEQ}")), "{s}");

    assert!(
        generic_error_reply(&[0, 0, 0xAA], &mut scratch).is_none(),
        "seq 0 = no reply expected"
    );
    assert!(generic_error_reply(&[1], &mut scratch).is_none());
    assert!(
        generic_error_reply(&[1, 0], &mut [0u8; 2]).is_none(),
        "scratch too small"
    );
}

#[test]
fn loopback() {
    let mut scratch = [0u8; 16];
    let (kind, bytes) = loopback_reply(7, &[1, 2, 3], &mut scratch).unwrap();
    let Message::Loopback { repeat, seq, data } = Message::decode(kind, bytes).unwrap() else {
        panic!()
    };
    assert_eq!((repeat, seq, data), (0, 7, &[1u8, 2, 3][..]));
    assert!(loopback_reply(0, &[0; 9], &mut scratch).is_none());

    assert_eq!(loopback_seqs(0, 5).count(), 0);
    assert_eq!(loopback_seqs(1, 5).collect::<Vec<_>>(), [5]);
    assert_eq!(loopback_seqs(3, 5).collect::<Vec<_>>(), [0, 1, 2]);
}

/// Echoes request args back as a Value event, fails on requests starting with 0xFF after seq.
struct EchoBackend;

impl WireWeaverApiBackend for EchoBackend {
    fn process_bytes<'a>(
        &mut self,
        _sink: &mut impl MessageSink,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        let seq = u16::from_le_bytes([data[0], data[1]]);
        if data.get(2) == Some(&0xFF) {
            return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
        }
        Event {
            seq,
            result: Ok(EventKind::Value {
                data: TailBytes(&data[2..]),
            }),
        }
        .to_ww_bytes(scratch)
    }

    fn version(&self) -> FullVersion<'_> {
        FullVersion::new("test_api", Version::new(0, 3, 1))
    }
}

#[derive(Default)]
struct Packets(Vec<Vec<u8>>);

impl blocking::PacketSink for &mut Packets {
    type Error = ();

    fn write_packet(&mut self, packet: &[u8]) -> Result<(), ()> {
        self.0.push(packet.to_vec());
        Ok(())
    }
}

/// Host side: frames messages into packets and de-frames device packets
struct Host {
    tx_buf: [u8; 16],
    rx_buf: [u8; 512],
}

impl Host {
    fn packets(&mut self, messages: &[Message<'_>]) -> Vec<Vec<u8>> {
        let mut tx = TxFramer::new(&mut self.tx_buf);
        let mut out = vec![];
        let mut scratch = [0u8; 256];
        for m in messages {
            let (kind, payload) = m.encode(&mut scratch).unwrap();
            loop {
                let done = tx.write(kind, payload).unwrap();
                if !done {
                    let len = tx.flush();
                    out.push(tx.buf()[..len].to_vec());
                } else {
                    break;
                }
            }
            let len = tx.flush();
            out.push(tx.buf()[..len].to_vec());
        }
        out
    }

    fn messages(&mut self, packets: &[Vec<u8>]) -> Vec<(Kind, Vec<u8>)> {
        let mut rx = RxFramer::new(&mut self.rx_buf);
        let mut out = vec![];
        for p in packets {
            rx.stage(p).unwrap();
            loop {
                rx.reassemble();
                let Some((kind, m)) = rx.message() else { break };
                out.push((Kind::from_repr(kind).unwrap(), m.to_vec()));
            }
        }
        out
    }
}

#[test]
fn blocking_server() {
    const PACKET: usize = 16;
    let mut tx_frame = [0u8; PACKET];
    let mut rx = crate::RxBuffer::<PACKET, 128>::new();
    let mut scratch = [0u8; 128];
    let mut sent = Packets::default();
    let mut host = Host {
        tx_buf: [0; 16],
        rx_buf: [0; 512],
    };
    let mut backend = EchoBackend;
    let mut now = Instant::from_millis(10);
    {
        let mut server = blocking::Server::new(
            config(),
            &mut sent,
            &mut tx_frame,
            rx.assembly_buf(PACKET),
            &mut scratch,
        );
        assert_eq!(server.link().max_message_len(), 128);
        server.on_transport_up(now);
        let host_version = FullVersion::new("test_api", Version::new(0, 3, 0));
        let mut event = None;
        for p in host.packets(&[
            Message::Nop,
            Message::GetDeviceInfo,
            link_setup(host_version),
        ]) {
            event = server.on_packet(now, &p, &mut backend).or(event);
        }
        assert_eq!(event, Some(LinkEvent::Up));
        assert_eq!(server.link().config().accumulation_time, ACC);

        // two requests in one go, the second one longer than a packet
        let long: Vec<u8> = [2u8, 0].into_iter().chain(0..40).collect();
        for p in host.packets(&[
            Message::Data {
                channel: 0,
                bytes: &[1, 0, 0xAA],
            },
            Message::Data {
                channel: 0,
                bytes: &long,
            },
            Message::Data {
                channel: 0,
                bytes: &[3, 0, 0xFF],
            },
        ]) {
            server.on_packet(now, &p, &mut backend);
        }
        // replies are held back until the accumulation window ends
        now = now + ACC;
        assert_eq!(server.poll_timeout(), Some(now));
        server.poll(now);

        // stream update from the main loop
        let (mut sink, scratch) = server.sink(now);
        let update = Event {
            seq: 0,
            result: Ok(EventKind::Value {
                data: TailBytes(&[0x55]),
            }),
        }
        .to_ww_bytes(scratch)
        .unwrap();
        sink.send_message(update).unwrap();
        now = now + ACC;
        server.poll(now);

        server.disconnect(now, DisconnectReason::RequestByUser);
        assert!(!server.is_up());
    }
    let received = host.messages(&sent.0);
    let kinds: Vec<Kind> = received.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        kinds,
        [
            Kind::Nop,
            Kind::DeviceInfo,
            Kind::LinkReady,
            Kind::Data0,
            Kind::Data0,
            Kind::Data0,
            Kind::Data0,
            Kind::Disconnect
        ]
    );
    let events: Vec<Event<'_>> = received[3..7]
        .iter()
        .map(|(_, bytes)| Event::from_ww_bytes(bytes).unwrap())
        .collect();
    assert_eq!(events[0].seq, 1);
    assert!(matches!(&events[0].result, Ok(EventKind::Value { data }) if data.0 == [0xAA]));
    assert_eq!(events[1].seq, 2);
    assert!(matches!(&events[1].result, Ok(EventKind::Value { data }) if data.0.len() == 40));
    assert_eq!(
        events[2].seq, 3,
        "generic error for a request the backend failed on"
    );
    assert!(matches!(
        &events[2].result,
        Err(e) if std::format!("{e:?}").contains("ResponseSerFailed")
    ));
    assert_eq!(events[3].seq, 0);
    let _ = ErrorKind::ResponseSerFailed;
}

#[test]
fn rx_buffer() {
    let mut rx = crate::RxBuffer::<64, 1000>::new();
    assert_eq!(rx.assembly_buf(64).len(), 1064);
    // smaller actual packet (e.g., bulk endpoint capped), max message stays the same
    assert_eq!(rx.assembly_buf(32).len(), 1032);
    assert_eq!(
        rx.assembly_buf(128).len(),
        1064,
        "clamped to MAX_PACKET_LEN"
    );
    // whole range is usable
    rx.assembly_buf(64).fill(0xAA);
    assert!(rx.assembly_buf(64).iter().all(|b| *b == 0xAA));
}

/// Stream transport over an in-memory byte pipe that hands out bytes in arbitrary small pieces,
/// like an RTT down channel or UART would.
mod stream {
    extern crate std;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::vec;
    use std::vec::Vec;

    use core::cell::RefCell;

    use ww_framer::traits::{ByteTail, NopChecksum};
    use ww_link::{RttChecksum, RttHead, RttTail};

    use crate::transport::{
        MessageRx, MessageTx, StreamRx, StreamSink, StreamSource, StreamTx, stream_overhead,
    };

    #[derive(Clone, Default)]
    struct Pipe {
        bytes: Rc<RefCell<VecDeque<u8>>>,
        /// Longest read served at once
        read_chunk: usize,
    }

    impl StreamSink for Pipe {
        type Error = ();
        async fn write_all(&mut self, bytes: &[u8]) -> Result<(), ()> {
            self.bytes.borrow_mut().extend(bytes);
            Ok(())
        }
    }

    impl StreamSource for Pipe {
        type Error = ();
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
            let mut bytes = self.bytes.borrow_mut();
            if bytes.is_empty() {
                return Err(());
            }
            let n = buf.len().min(self.read_chunk).min(bytes.len());
            for b in buf[..n].iter_mut() {
                *b = bytes.pop_front().unwrap();
            }
            Ok(n)
        }
        async fn wait_connected(&mut self) {}
    }

    type Tx<'a> = StreamTx<'a, Pipe, RttHead, RttChecksum, RttTail>;
    type Rx<'a> = StreamRx<'a, Pipe, RttHead, RttChecksum, RttTail>;
    /// ww_link enables all length forms of U2Head, so the longest head is 5 bytes
    const OVERHEAD: usize = stream_overhead::<RttHead, RttChecksum, RttTail>();

    fn block<T>(f: impl Future<Output = T>) -> T {
        embassy_futures::block_on(f)
    }

    fn drain(rx: &mut Rx<'_>) -> Vec<(u8, Vec<u8>)> {
        let mut out = Vec::new();
        while block(rx.wait_message()).is_ok() {
            let (kind, bytes) = rx.message().unwrap();
            out.push((kind, bytes.to_vec()));
            rx.consume();
        }
        assert_eq!(rx.message(), None);
        out
    }

    #[test]
    fn overhead_and_max_message_len() {
        assert_eq!(OVERHEAD, 5);
        assert_eq!(stream_overhead::<RttHead, NopChecksum, ByteTail<0>>(), 6);
        let mut buf = [0u8; 64 + OVERHEAD];
        let rx = Rx::new(Pipe::default(), &mut buf);
        assert_eq!(rx.max_message_len(), 64);
    }

    #[test]
    fn messages_are_packed_and_cut_at_any_byte() {
        let messages: [(u8, &[u8]); 4] = [
            (0, &[1, 2, 3]),
            (5, &[]),
            (0, &(0..40).collect::<Vec<u8>>()),
            (255, &[9, 8, 7, 6, 5]),
        ];
        for read_chunk in [1, 2, 3, 7, 64] {
            let pipe = Pipe {
                read_chunk,
                ..Pipe::default()
            };
            let mut tx_buf = [0u8; 64];
            let mut tx = Tx::new(pipe.clone(), &mut tx_buf);
            for (kind, m) in messages {
                block(tx.write_message(kind, m)).unwrap();
            }
            // nothing goes out until flushed or the chunk is full
            assert!(pipe.bytes.borrow().is_empty());
            block(tx.flush()).unwrap();

            let mut rx_buf = [0u8; 43];
            let mut rx = Rx::new(pipe.clone(), &mut rx_buf);
            let got = drain(&mut rx);
            assert_eq!(got.len(), messages.len(), "read_chunk = {read_chunk}");
            for (g, e) in got.iter().zip(messages) {
                assert_eq!((g.0, g.1.as_slice()), e, "read_chunk = {read_chunk}");
            }
        }
    }

    #[test]
    fn chunk_full_sends_and_never_splits() {
        let pipe = Pipe {
            read_chunk: 64,
            ..Pipe::default()
        };
        let mut tx_buf = [0u8; 16];
        let mut tx = Tx::new(pipe.clone(), &mut tx_buf);
        // 2 byte head (len > 7) + 10 bytes
        block(tx.write_message(0, &[0xAA; 10])).unwrap();
        assert!(pipe.bytes.borrow().is_empty());
        // 2 + 10 more do not fit, the first chunk goes out as is
        block(tx.write_message(0, &[0xBB; 10])).unwrap();
        assert_eq!(pipe.bytes.borrow().len(), 12);
        block(tx.flush()).unwrap();
        assert_eq!(pipe.bytes.borrow().len(), 24);
        // does not fit even into an empty chunk
        assert_eq!(
            block(tx.write_message(0, &[0xCC; 15])),
            Err(crate::transport::FramedError::Framing)
        );
        assert_eq!(pipe.bytes.borrow().len(), 24);

        let mut rx_buf = [0u8; 10 + OVERHEAD];
        let mut rx = Rx::new(pipe, &mut rx_buf);
        let got = drain(&mut rx);
        assert_eq!(got, [(0, vec![0xAA; 10]), (0, vec![0xBB; 10])]);
    }

    #[test]
    fn oversized_message_is_dropped() {
        let pipe = Pipe {
            read_chunk: 5,
            ..Pipe::default()
        };
        let mut tx_buf = [0u8; 64];
        let mut tx = Tx::new(pipe.clone(), &mut tx_buf);
        block(tx.write_message(0, &[0xEE; 30])).unwrap();
        block(tx.write_message(1, &[1, 2])).unwrap();
        block(tx.flush()).unwrap();

        // rx accepts up to 16 byte messages: the 30 byte one is skipped. There is no delimiter to
        // re-synchronize on, so what follows is garbage until the stream goes quiet and is reset,
        // i.e., a host must never send more than the device advertised.
        let mut rx_buf = [0u8; 16 + OVERHEAD];
        let mut rx = Rx::new(pipe.clone(), &mut rx_buf);
        assert_eq!(drain(&mut rx), []);
        rx.reset();
        block(tx.write_message(2, &[3, 4])).unwrap();
        block(tx.flush()).unwrap();
        assert_eq!(drain(&mut rx), [(2, vec![3, 4])]);
    }
}
