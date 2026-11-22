use std::time::Duration;

use wire_weaver::prelude::{SerializeShrinkWrap, SerializeShrinkWrapOwned};
use wire_weaver::shrink_wrap::RefVec;
use ww_tap::{Framer, Medium};
use ww_tape::{BlockKind, BodyCodec, FileBlocks};

use super::*;
use crate::tap::test_tape::TestTape;
use crate::tap::{LinkEvent, Tap, host_session};

/// A link's set-up session is put after its first one. A reader that read on to such
/// a record has seen that session: it is not a third one. A reader that starts at it takes it.
#[test]
fn a_session_put_again_is_not_a_new_session() {
    let (tape, clock) = TestTape::with_fake_clock();
    let mut link = tape.link();
    link.session(&host_session(Medium::InProcessUsb, Framer::None, 64, "dev"));
    tape.add_bundle(b"hash", b"bundle");
    *clock.lock().unwrap() += Duration::from_millis(1);
    let mut set_up = host_session(Medium::InProcessUsb, Framer::None, 64, "dev");
    set_up.device_info = RefVec::Slice { slice: &[1, 2, 3] };
    link.session(&set_up);
    for _ in 0..8 {
        link.link_event(&LinkEvent::Disconnected {
            reason: "done",
            keep_streams: false,
        });
        tape.flush();
    }

    let file = tape.file();
    let mut decoder = Decoder::default().with_resolver(crate::test_resolver());
    let mut blocks = FileBlocks::new(&file[..]);
    while let Some(b) = blocks.next_good(|e| panic!("{e}")) {
        decoder.feed(&b);
    }
    decoder.finish();
    let mut seen = vec![];
    while let Some(d) = decoder.next() {
        seen.push(match d.what {
            What::Session(s) => format!("session {}", s.device_info.len()),
            What::LinkEvent(LinkEventOwned::Disconnected { .. }) => "disconnected".into(),
            other => format!("{other:?}"),
        });
    }
    let mut want = vec!["session 0".to_owned(), "session 3".to_owned()];
    want.extend(std::iter::repeat_n("disconnected".to_owned(), 8));
    assert_eq!(seen, want);
    // a reader that starts at the session's last put never read its first write: the put is news
    let all = tape.blocks();
    let session_track = tape.track(0, ww_tap::ids::SESSION).unwrap();
    let holds_session = |b: &Vec<u8>| {
        let b = ww_tape::Block::read(b).unwrap();
        b.is_repeat()
            && b.records()
                .any(|r| r.unwrap().track == ww_tape::RecordTrack::Data(session_track))
    };
    let last = all.iter().rposition(holds_session).unwrap();
    let first_write = all.iter().position(holds_session).unwrap();
    assert!(last > first_write, "the session is put again");
    let mut decoder = Decoder::default().with_resolver(crate::test_resolver());
    for b in &all[last..] {
        decoder.feed(&ww_tape::Block::read(b).unwrap());
    }
    decoder.finish();
    let mut seen = vec![];
    while let Some(d) = decoder.next() {
        if let What::Session(s) = d.what {
            seen.push(s.device_info.len());
        }
    }
    assert_eq!(seen, [3]);
}

/// A records block whose lz4 body doesn't decode is reported, not taken for an empty block.
#[test]
fn a_bad_coded_body_is_reported() {
    let (tape, _clock) = TestTape::with_fake_clock();
    let mut link = tape.link();
    link.session(&host_session(Medium::InProcessUsb, Framer::None, 64, "dev"));
    for _ in 0..20 {
        link.link_event(&LinkEvent::Disconnected {
            reason: "again and again",
            keep_streams: false,
        });
    }
    tape.flush();
    let mut records = None;
    for bytes in tape.blocks() {
        let b = ww_tape::Block::read(&bytes).unwrap();
        if b.kind == BlockKind::Records
            && b.first_track() != Some(ww_tape::RecordTrack::Definitions)
        {
            records = Some(bytes.clone());
        }
    }
    let plain = records.unwrap();
    let mut coded = vec![0u8; ww_tape::coded_len_max(plain.len())];
    let n = ww_tape::code_body(&plain, BodyCodec::LZ4, &mut coded).unwrap();
    coded.truncate(n);
    // the decoded length after the codec byte: another one than the lz4 bytes give, the CRC made good again
    coded[33] = 0x7f;
    let crc = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI).checksum(&coded[..n - 4]);
    coded[n - 4..].copy_from_slice(&crc.to_le_bytes());
    let block = ww_tape::Block::read(&coded).unwrap();
    let mut decoder = Decoder::default().with_resolver(crate::test_resolver());
    decoder.feed(&block);
    let d = decoder.next().unwrap();
    assert!(
        matches!(d.what, What::BadBody { seq, .. } if seq == block.seq),
        "{:?}",
        d.what
    );
    assert_eq!(decoder.stats.blocks_bad, 1);
}

/// A capture carries the tape's and the tap's own bundles: both crates' current versions have a
/// snapshot, so a version bump without `just save-snapshots` fails here.
#[test]
fn the_tape_and_tap_bundles_are_in_the_snapshots() {
    for api in [ww_tape::api(), ww_tap::api()] {
        let bundle = crate::test_resolver()
            .snapshot(&api.make_owned())
            .expect("a snapshot of the current version");
        assert_eq!(bundle.ext_crates[0], api.make_owned());
    }
}

/// Records fed one by one: a session put again is not news, a gap is given out and counted.
#[test]
fn record_input_sessions_and_gaps() {
    fn bytes(x: &impl SerializeShrinkWrap) -> Vec<u8> {
        let mut buf = [0u8; 256];
        x.to_ww_bytes(&mut buf).unwrap().to_vec()
    }
    let mut d = Decoder::default().with_resolver(crate::test_resolver());
    let session = |peer: &str| {
        let s = host_session(Medium::InProcessUsb, Framer::None, 64, peer);
        SessionOwned::from_ww_bytes_owned(&bytes(&s)).unwrap()
    };
    d.session(0, 10, session("dev"));
    d.record(
        0,
        ww_tap::ids::SESSION,
        20,
        &session("dev").to_ww_bytes_owned().unwrap(),
    );
    d.gap(0, 5);
    let event = LinkEvent::Disconnected {
        reason: "done",
        keep_streams: false,
    };
    d.record(0, ww_tap::ids::EVENTS, 30, &bytes(&event));
    d.session(1, 40, session("other"));
    let mut seen = vec![];
    while let Some(x) = d.next() {
        seen.push(match x.what {
            What::Session(s) => format!("{:?} {} session {}", x.link, x.t, s.peer),
            What::Gap { items } => format!("{:?} {} gap {items}", x.link, x.t),
            What::LinkEvent(_) => format!("{:?} {} event", x.link, x.t),
            other => format!("{other:?}"),
        });
    }
    assert_eq!(
        seen,
        [
            "Some(0) 10 session dev",
            "Some(0) 20 gap 5",
            "Some(0) 30 event",
            "Some(1) 40 session other"
        ]
    );
    let stats = d.stats.take();
    assert_eq!(stats.records_missed, 5);
    assert_eq!((stats.from_ns, stats.to_ns), (Some(10), 40));
    assert!(stats.report().contains("5 records missing from the input"));
}

/// Two devices of one API in one capture: the decoder numbers their links by identity, and a reader picks a device
/// by its serial ([ww_tap::dims::PEER]), whichever link number the producer gave it.
#[test]
fn a_reader_picks_a_device_by_its_serial() {
    let (tape, _clock) = TestTape::with_fake_clock();
    let mut a = tape.link();
    let mut b = tape.link();
    b.session(&host_session(Medium::Usb, Framer::None, 64, "SN-B"));
    a.session(&host_session(Medium::Usb, Framer::None, 64, "SN-A"));
    a.link_event(&LinkEvent::Error { what: "on A" });
    b.link_event(&LinkEvent::Error { what: "on B" });
    tape.flush();
    let mut decoder = Decoder::default().with_resolver(crate::test_resolver());
    for bytes in tape.blocks() {
        decoder.feed(&ww_tape::Block::read(&bytes).unwrap());
    }
    decoder.finish();
    let link = decoder
        .find_link(ww_tap::dims::PEER, "SN-B")
        .expect("SN-B's link");
    let identity = decoder.link_identity(link).unwrap();
    assert_eq!(identity.to_string(), "medium=usb peer=SN-B");
    assert_eq!(decoder.find_link(ww_tap::dims::PEER, "SN-C"), None);
    let mut of_b = vec![];
    while let Some(d) = decoder.next() {
        if d.link == Some(link)
            && let What::LinkEvent(LinkEventOwned::Error { what }) = d.what
        {
            of_b.push(what);
        }
    }
    assert_eq!(of_b, ["on B"]);
}
