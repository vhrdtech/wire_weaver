use std::time::Duration;

use wire_weaver::shrink_wrap::RefVec;
use ww_tap::{Framer, Medium};
use ww_tape::{BlockKind, BodyCodec, FileBlocks, SyncPolicy, TapeWriter};

use super::*;
use crate::tap::{Layers, LinkEvent, Recorder, RecorderConfig, Tap, host_session};

/// A writer puts a session again past its near and copies it after its first write. A reader that read on to such
/// a record has seen that session: it is not a third one. A reader that starts at it takes it.
#[test]
fn a_session_put_again_is_not_a_new_session() {
    let (recorder, clock) = Recorder::with_fake_clock(RecorderConfig::default());
    let sub = recorder.subscribe(Layers::ALL, 64);
    let mut link = recorder.link();
    link.session(&host_session(Medium::InProcessUsb, Framer::None, 64, "dev"));
    // a bundle hands over a block of definitions at once
    recorder.add_bundle(b"hash", b"bundle");
    *clock.lock().unwrap() += Duration::from_millis(1);
    let mut set_up = host_session(Medium::InProcessUsb, Framer::None, 64, "dev");
    set_up.device_info = RefVec::Slice { slice: &[1, 2, 3] };
    link.session(&set_up);
    for _ in 0..8 {
        link.link_event(&LinkEvent::Disconnected {
            reason: "done",
            keep_streams: false,
        });
        recorder.flush();
    }

    let dir = std::env::temp_dir().join(format!("ww_decode_backup_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.wwtape");
    let _ = std::fs::remove_file(&path);
    let mut writer = TapeWriter::create(&path, SyncPolicy::None).unwrap();
    // copies every few blocks, so the session is copied in this short capture
    writer.set_near_policy(ww_tape::NearPolicy {
        copy_spacing: 256,
        ..ww_tape::NearPolicy::default()
    });
    while let Some(chunk) = sub.try_recv() {
        writer.write_block(chunk.bytes()).unwrap();
    }
    writer.finish().unwrap();

    let mut decoder = Decoder::default().with_resolver(crate::test_resolver());
    let mut blocks = FileBlocks::new(std::fs::File::open(&path).unwrap());
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
    // a reader that starts at the session's copy never read its first write: the copy is news
    let bytes = std::fs::read(&path).unwrap();
    let mut blocks = FileBlocks::new(&bytes[..]);
    let mut all = vec![];
    while let Some(b) = blocks.next_good(|e| panic!("{e}")) {
        all.push(b.bytes().to_vec());
    }
    let session_track = ww_tap::recorder::track_number(0, ww_tap::ids::SESSION);
    let holds_session = |b: &Vec<u8>| {
        let b = ww_tape::Block::read(b).unwrap();
        b.is_repeat() && b.records().any(|r| r.unwrap().track == session_track)
    };
    let last = all.iter().rposition(holds_session).unwrap();
    let first_write = all.iter().position(holds_session).unwrap();
    assert!(last > first_write + 1, "the session is copied");
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
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A records block whose lz4 body doesn't decode is reported, not taken for an empty block.
#[test]
fn a_bad_coded_body_is_reported() {
    let (recorder, _clock) = Recorder::with_fake_clock(RecorderConfig::default());
    let sub = recorder.subscribe(Layers::ALL, 64);
    let mut link = recorder.link();
    link.session(&host_session(Medium::InProcessUsb, Framer::None, 64, "dev"));
    for _ in 0..20 {
        link.link_event(&LinkEvent::Disconnected {
            reason: "again and again",
            keep_streams: false,
        });
    }
    recorder.flush();
    let mut records = None;
    while let Some(chunk) = sub.try_recv() {
        let b = chunk.block();
        if b.kind == BlockKind::Records && b.first_track() != Some(0) {
            records = Some(chunk.bytes().to_vec());
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

/// A capture carries the tape's and the tap's own bundles (`ww capture`): both crates' current versions have a
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
