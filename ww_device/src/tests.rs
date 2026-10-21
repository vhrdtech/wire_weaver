//! Link core driven by hand with hand-advanced time, and the blocking server against a host
//! simulated with a plain framer. The async server is exercised end-to-end against the real host
//! event loop in `wire_weaver_client`.

extern crate std;
use std::vec;
use std::vec::Vec;

use core::time::Duration;

use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{
    BufReader, DeserializeShrinkWrap, Error as ShrinkWrapError, SerializeShrinkWrap, UVlq32,
};
use wire_weaver::{BlockingEventOut, WireWeaverApiBackend};
use ww_client_server::{ErrorKind, Event, EventKind};
use ww_link::{
    ApiHashPair, CompactVersion, DisconnectReason, FullVersion, Kind, LinkSetup, Message,
    PEER_TIMEOUT_MS, PING_INTERVAL_MS, UsbFraming,
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

/// The host's setup token in these tests.
const TOKEN: u64 = 0x5EED_0000_0000_0001;

fn get_device_info() -> Message<'static> {
    Message::GetDeviceInfo { token: TOKEN }
}

/// Transmits in a form that is easy to compare: decoded kind, or None for Flush
#[derive(Debug, PartialEq)]
enum Tx {
    NewSession,
    Msg(Kind, Vec<u8>),
    Flush,
}

fn drain(link: &mut DeviceLink<'_>, now: Instant) -> Vec<Tx> {
    let mut scratch = [0u8; 256];
    let mut out = vec![];
    while let Some(t) = link.poll_transmit(now, &mut scratch) {
        out.push(match t {
            Transmit::NewSession => Tx::NewSession,
            Transmit::Message { kind, bytes } => {
                Tx::Msg(Kind::from_repr(kind).unwrap(), bytes.to_vec())
            }
            Transmit::Flush => Tx::Flush,
        });
    }
    out
}

/// Kinds of the messages sent, None for a Flush; [Tx::NewSession] is left out (see `new_session_before_reply`).
fn kinds(tx: &[Tx]) -> Vec<Option<Kind>> {
    tx.iter()
        .filter_map(|t| match t {
            Tx::Msg(k, _) => Some(Some(*k)),
            Tx::Flush => Some(None),
            Tx::NewSession => None,
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
        token: TOKEN,
        client_meta: None,
    })
}

/// Link up at `now`, returns the transmits of link setup.
fn connect(link: &mut DeviceLink<'_>, now: Instant) -> Vec<Tx> {
    let mut scratch = [0u8; 256];
    link.on_transport_up(now);
    assert!(feed(link, now, &get_device_info(), &mut scratch).is_none());
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
    let Tx::Msg(_, info) = &sent[3] else { panic!() };
    let Message::DeviceInfo(info) = Message::decode(Kind::DeviceInfo as u8, info).unwrap() else {
        panic!()
    };
    assert_eq!(info.dev_max_message_len, 512);
    assert_eq!(info.packet_accumulation_time_us, 1000);
    assert_eq!(info.dev_link_version, ww_link::LINK_VERSION);
    assert_eq!(info.user_api_version.crate_id, "test_api");
    assert_eq!(info.token, TOKEN);
    let Tx::Msg(_, ready) = &sent[5] else {
        panic!()
    };
    assert!(matches!(
        Message::decode(Kind::LinkReady as u8, ready),
        Ok(Message::LinkReady { token: TOKEN })
    ));
    assert!(link.is_up());
    assert_eq!(link.remote_max_message_len(), 1000);
    assert_eq!(link.poll_timeout(), Some(now + PING));
}

#[test]
fn new_session_before_reply() {
    let now = Instant::from_millis(1);
    let mut link = DeviceLink::new(config(), 512);
    let mut scratch = [0u8; 256];
    link.on_transport_up(now);
    feed(&mut link, now, &get_device_info(), &mut scratch);
    // the framer starts a new session before anything of the reply, on every GetDeviceInfo
    let sent = drain(&mut link, now);
    assert_eq!(sent[0], Tx::NewSession);
    assert_eq!(sent.iter().filter(|t| **t == Tx::NewSession).count(), 1);
    feed(&mut link, now, &get_device_info(), &mut scratch);
    assert_eq!(drain(&mut link, now)[0], Tx::NewSession);
}

#[test]
fn hosts_without_a_token_or_with_a_stale_one() {
    let now = Instant::from_millis(1);
    let mut link = DeviceLink::new(config(), 512);
    let mut scratch = [0u8; 256];
    link.on_transport_up(now);
    let host = || FullVersion::new("test_api", Version::new(0, 3, 0));

    // a host built before the token: refused with IncompatibleVersion, no DeviceInfo
    feed(
        &mut link,
        now,
        &Message::GetDeviceInfo { token: 0 },
        &mut scratch,
    );
    let sent = drain(&mut link, now);
    assert_eq!(kinds(&sent), [Some(Kind::Disconnect), None]);
    let Tx::Msg(_, reason) = &sent[0] else {
        panic!()
    };
    assert!(matches!(
        Message::decode(Kind::Disconnect as u8, reason),
        Ok(Message::Disconnect(DisconnectReason::IncompatibleVersion))
    ));
    let old_setup = Message::LinkSetup(LinkSetup {
        host_user_version: host(),
        host_max_message_len: 1000,
        token: 0,
        client_meta: None,
    });
    feed(&mut link, now, &old_setup, &mut scratch);
    assert_eq!(
        kinds(&drain(&mut link, now)),
        [Some(Kind::Disconnect), None]
    );
    assert!(!link.is_up());

    // a LinkSetup of an earlier session (its host restarted and sent a new GetDeviceInfo since) is ignored
    feed(&mut link, now, &get_device_info(), &mut scratch);
    drain(&mut link, now);
    let stale = Message::LinkSetup(LinkSetup {
        host_user_version: host(),
        host_max_message_len: 1000,
        token: TOKEN + 1,
        client_meta: None,
    });
    feed(&mut link, now, &stale, &mut scratch);
    assert!(drain(&mut link, now).is_empty());
    assert!(!link.is_up());
    feed(&mut link, now, &link_setup(host()), &mut scratch);
    assert!(link.is_up());
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
    // LinkSetup is only taken with the token of a GetDeviceInfo before it
    feed(&mut link, now, &get_device_info(), &mut scratch);
    drain(&mut link, now);

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
    assert!(feed(&mut link, now, &get_device_info(), &mut scratch).is_none());
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
    feed(&mut link, now, &get_device_info(), &mut scratch);
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
    let reply = generic_error_reply(&[0x82, 0x2c, 0xAA], &mut scratch).unwrap();
    let event = Event::from_ww_bytes(reply).unwrap();
    assert_eq!(event.seq, UVlq32(300));
    let err = event.result.unwrap_err();
    let s = std::format!("{err:?}");
    assert!(s.contains("ResponseSerFailed"), "{s}");
    assert!(s.contains(&std::format!("{GENERIC_ERROR_SEQ}")), "{s}");

    assert!(
        generic_error_reply(&[0, 0xAA], &mut scratch).is_none(),
        "seq 0 = no reply expected"
    );
    assert!(generic_error_reply(&[0x81], &mut scratch).is_none());
    assert!(
        generic_error_reply(&[1], &mut [0u8; 2]).is_none(),
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
/// Requests starting with 0xEE first send an event with seq 0 and the medium as data through the handler's `out`.
struct EchoBackend;

impl WireWeaverApiBackend for EchoBackend {
    type Medium = u8;

    fn process_bytes<'a>(
        &mut self,
        out: &mut impl BlockingEventOut,
        medium: u8,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        let mut rd = BufReader::new(data);
        let seq = UVlq32(rd.read_uvlq32()?);
        let data = &data[data.len() - rd.bytes_left()..];
        if data.first() == Some(&0xFF) {
            return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
        }
        if data.first() == Some(&0xEE) {
            let event = Event {
                seq: UVlq32(0),
                result: Ok(EventKind::Value {
                    data: TailBytes(&[medium]),
                }),
            };
            out.send_event_blocking(&event).unwrap();
        }
        Event {
            seq,
            result: Ok(EventKind::Value {
                data: TailBytes(data),
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
    type Framing = UsbFraming;

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
        let mut tx = TxFramer::<UsbFraming>::new(&mut self.tx_buf);
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
        let mut rx = RxFramer::<UsbFraming>::new(&mut self.rx_buf);
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
    let mut event_scratch = [0u8; 128];
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
            &mut event_scratch,
        )
        .with_medium(7u8);
        assert_eq!(server.link().max_message_len(), 128);
        server.on_transport_up(now);
        let host_version = FullVersion::new("test_api", Version::new(0, 3, 0));
        let mut event = None;
        for p in host.packets(&[Message::Nop, get_device_info(), link_setup(host_version)]) {
            event = server.on_packet(now, &p, &mut backend).or(event);
        }
        assert_eq!(event, Some(LinkEvent::Up));
        assert_eq!(server.link().config().accumulation_time, ACC);

        // two requests in one go, the second one longer than a packet
        let long: Vec<u8> = [2u8].into_iter().chain(0..40).collect();
        for p in host.packets(&[
            Message::Data {
                channel: 0,
                bytes: &[1, 0xAA],
            },
            Message::Data {
                channel: 0,
                bytes: &long,
            },
            Message::Data {
                channel: 0,
                bytes: &[3, 0xFF],
            },
            // handler sends an event before replying
            Message::Data {
                channel: 0,
                bytes: &[4, 0xEE],
            },
        ]) {
            server.on_packet(now, &p, &mut backend);
        }
        // replies are held back until the accumulation window ends
        now = now + ACC;
        assert_eq!(server.poll_timeout(), Some(now));
        server.poll(now);

        // stream update from the main loop
        let update = Event {
            seq: UVlq32(0),
            result: Ok(EventKind::Value {
                data: TailBytes(&[0x55]),
            }),
        };
        server.sink(now).send_event_blocking(&update).unwrap();
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
            Kind::Data0,
            Kind::Data0,
            Kind::Disconnect
        ]
    );
    let events: Vec<Event<'_>> = received[3..9]
        .iter()
        .map(|(_, bytes)| Event::from_ww_bytes(bytes).unwrap())
        .collect();
    assert_eq!(events[0].seq, UVlq32(1));
    assert!(matches!(&events[0].result, Ok(EventKind::Value { data }) if data.0 == [0xAA]));
    assert_eq!(events[1].seq, UVlq32(2));
    assert!(matches!(&events[1].result, Ok(EventKind::Value { data }) if data.0.len() == 40));
    assert_eq!(
        events[2].seq,
        UVlq32(3),
        "generic error for a request the backend failed on"
    );
    assert!(matches!(
        &events[2].result,
        Err(e) if std::format!("{e:?}").contains("ResponseSerFailed")
    ));
    assert_eq!(events[3].seq, UVlq32(0), "event from the handler");
    assert!(
        matches!(&events[3].result, Ok(EventKind::Value { data }) if data.0 == [7]),
        "handler sees the medium"
    );
    assert_eq!(events[4].seq, UVlq32(4));
    assert!(matches!(&events[4].result, Ok(EventKind::Value { data }) if data.0 == [0xEE]));
    assert_eq!(events[5].seq, UVlq32(0), "event from the main loop");
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

/// Async server with in-memory message transports: the reply to a lone request is flushed at once, back to back
/// requests share a frame, events from the main loop keep the accumulation window.
mod async_server {
    use super::*;
    use crate::server::{Clock, Server};
    use crate::transport::{MessageRx, MessageTx};
    use core::cell::RefCell;
    use core::task::Poll;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use wire_weaver::{EventWriter, MessageSink, WireWeaverAsyncApiBackend};

    #[derive(Debug, PartialEq, Eq)]
    enum Out {
        Message(Kind),
        Flush,
    }

    type Queue = Rc<RefCell<VecDeque<(u8, Vec<u8>)>>>;
    type Log = Rc<RefCell<Vec<Out>>>;

    fn push(queue: &Queue, m: &Message<'_>) {
        let mut scratch = [0u8; 256];
        let (kind, payload) = m.encode(&mut scratch).unwrap();
        queue.borrow_mut().push_back((kind, payload.to_vec()));
    }

    struct Rx {
        queue: Queue,
        current: Option<(u8, Vec<u8>)>,
    }

    impl MessageRx for Rx {
        type Error = ();

        async fn wait_message(&mut self) -> Result<(), ()> {
            if self.current.is_none() {
                let next = self.queue.borrow_mut().pop_front();
                match next {
                    Some(m) => self.current = Some(m),
                    None => core::future::pending::<()>().await,
                }
            }
            Ok(())
        }

        fn message(&self) -> Option<(u8, &[u8])> {
            self.current.as_ref().map(|(k, m)| (*k, m.as_slice()))
        }

        fn consume(&mut self) {
            self.current = None;
        }

        async fn wait_connected(&mut self) {}

        fn reset(&mut self) {}

        fn max_message_len(&self) -> usize {
            128
        }
    }

    struct Tx(Log);

    impl MessageTx for Tx {
        type Error = ();

        async fn write_message(&mut self, kind: u8, _message: &[u8]) -> Result<(), ()> {
            self.0
                .borrow_mut()
                .push(Out::Message(Kind::from_repr(kind).unwrap()));
            Ok(())
        }

        async fn flush(&mut self) -> Result<(), ()> {
            self.0.borrow_mut().push(Out::Flush);
            Ok(())
        }

        fn reset(&mut self) {}
    }

    struct FixedClock(Instant);

    impl Clock for FixedClock {
        fn now(&self) -> Instant {
            self.0
        }

        async fn wait_until(&self, _at: Instant) {
            core::future::pending::<()>().await
        }
    }

    /// Echoes the request (seq and args) back as the reply.
    struct Echo;

    impl WireWeaverAsyncApiBackend for Echo {
        type Medium = ();

        async fn process_bytes<'a>(
            &mut self,
            _out: &mut EventWriter<'_, impl MessageSink>,
            _medium: (),
            data: &[u8],
            scratch: &'a mut [u8],
        ) -> Result<&'a [u8], ShrinkWrapError> {
            scratch[..data.len()].copy_from_slice(data);
            Ok(&scratch[..data.len()])
        }

        fn version(&self) -> FullVersion<'_> {
            FullVersion::new("test_api", Version::new(0, 3, 1))
        }
    }

    /// Handle everything that is ready now, without any timer firing.
    fn run_ready(server: &mut Server<'_, Tx, Rx, FixedClock>) {
        loop {
            let Poll::Ready(ready) = embassy_futures::poll_once(server.wait()) else {
                return;
            };
            embassy_futures::block_on(server.handle(ready, &mut Echo));
        }
    }

    #[test]
    fn replies_flushed_when_no_request_is_waiting() {
        let queue = Queue::default();
        let log = Log::default();
        let mut scratch = [0u8; 128];
        let mut event_scratch = [0u8; 128];
        let rx = Rx {
            queue: queue.clone(),
            current: None,
        };
        let mut server = Server::new(
            config(),
            Tx(log.clone()),
            rx,
            FixedClock(Instant::from_millis(10)),
            &mut scratch,
            &mut event_scratch,
        );
        push(&queue, &get_device_info());
        push(
            &queue,
            &link_setup(FullVersion::new("test_api", Version::new(0, 3, 0))),
        );
        run_ready(&mut server);
        assert!(server.is_up());
        log.borrow_mut().clear();

        // back to back requests: the first reply waits for the second, then both go out with one flush
        for bytes in [&[1u8, 0xAA][..], &[2, 0xBB]] {
            push(&queue, &Message::Data { channel: 0, bytes });
        }
        run_ready(&mut server);
        assert_eq!(
            *log.borrow(),
            [
                Out::Message(Kind::Data0),
                Out::Message(Kind::Data0),
                Out::Flush
            ]
        );
        log.borrow_mut().clear();

        // a lone request: flushed right away
        push(
            &queue,
            &Message::Data {
                channel: 0,
                bytes: &[3, 0xCC],
            },
        );
        run_ready(&mut server);
        assert_eq!(*log.borrow(), [Out::Message(Kind::Data0), Out::Flush]);
        log.borrow_mut().clear();

        // an event from the main loop waits for the accumulation window
        embassy_futures::block_on(server.send(&[0, 0x55])).unwrap();
        run_ready(&mut server);
        assert_eq!(*log.borrow(), [Out::Message(Kind::Data0)]);
    }

    /// A clock set by the test, whose timers fire once it has passed them.
    struct TestClock(Rc<core::cell::Cell<Instant>>);

    impl Clock for TestClock {
        fn now(&self) -> Instant {
            self.0.get()
        }

        async fn wait_until(&self, at: Instant) {
            core::future::poll_fn(|_| {
                if self.0.get() >= at {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await
        }
    }

    /// `wait_until` returns `Due` at the user's time, before the link's own deadline, and leaves the link alone
    #[test]
    fn wait_until_returns_due() {
        let queue = Queue::default();
        let log = Log::default();
        let mut scratch = [0u8; 128];
        let mut event_scratch = [0u8; 128];
        let now = Rc::new(core::cell::Cell::new(Instant::from_millis(10)));
        let rx = Rx {
            queue: queue.clone(),
            current: None,
        };
        let mut server = Server::new(
            config(),
            Tx(log.clone()),
            rx,
            TestClock(now.clone()),
            &mut scratch,
            &mut event_scratch,
        );
        push(&queue, &get_device_info());
        push(
            &queue,
            &link_setup(FullVersion::new("test_api", Version::new(0, 3, 0))),
        );
        while let Poll::Ready(ready) = embassy_futures::poll_once(server.wait()) {
            assert!(!ready.is_due());
            embassy_futures::block_on(server.handle(ready, &mut Echo));
        }
        assert!(server.is_up());
        log.borrow_mut().clear();

        let at = Instant::from_micros(10_500);
        assert!(embassy_futures::poll_once(server.wait_until(Some(at))).is_pending());
        now.set(at);
        let Poll::Ready(ready) = embassy_futures::poll_once(server.wait_until(Some(at))) else {
            panic!("not due");
        };
        assert!(ready.is_due());
        assert_eq!(
            embassy_futures::block_on(server.handle(ready, &mut Echo)),
            None
        );
        assert!(log.borrow().is_empty());
        assert!(server.is_up());

        // a time already past is due at once; None waits as wait() does
        assert!(
            embassy_futures::poll_once(server.wait_until(Some(Instant::from_millis(1)))).is_ready()
        );
        assert!(embassy_futures::poll_once(server.wait_until(None)).is_pending());
    }
}

#[test]
fn peer_meta_from_link_setup() {
    let now = Instant::from_millis(0);
    let mut link = DeviceLink::new(config(), 512);
    link.on_transport_up(now);
    let mut scratch = [0u8; 512];
    assert!(link.peer_meta().is_none());
    feed(&mut link, now, &get_device_info(), &mut scratch);
    drain(&mut link, now);
    let long_name = "a-program-with-a-very-long-name-that-gets-cut";
    let setup = Message::LinkSetup(LinkSetup {
        host_user_version: FullVersion::new("test_api", Version::new(0, 3, 0)),
        host_max_message_len: 1000,
        token: TOKEN,
        client_meta: Some(ww_link::ClientMeta {
            program: long_name,
            app_version: "1.2.3",
            label: "",
            kind: ww_link::ClientKind::Gui,
            pid: 99,
            user: "roman",
            host: "pc1",
            purpose: None,
            instance_id: 5,
        }),
    });
    feed(&mut link, now, &setup, &mut scratch);
    assert!(link.is_up());
    let meta = link.peer_meta().expect("host sent its meta");
    assert_eq!(
        meta.program.as_str(),
        &long_name[..crate::link::PeerMeta::STR_LEN]
    );
    assert_eq!(meta.user.as_str(), "roman");
    assert_eq!(meta.host.as_str(), "pc1");
    assert_eq!(meta.kind, Some(ww_link::ClientKind::Gui));
    assert_eq!((meta.pid, meta.instance_id), (99, 5));
}
