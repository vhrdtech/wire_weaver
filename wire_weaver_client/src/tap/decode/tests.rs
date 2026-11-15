use std::time::Duration;

use wire_weaver::shrink_wrap::RefVec;
use ww_tap::{Framer, Medium};
use ww_tape::{BlockKind, BodyCodec, FileBlocks, SyncPolicy, TapeWriter};

use super::*;
use crate::tap::{Layers, LinkEvent, Recorder, RecorderConfig, Tap, host_session};

/// A capture's describe backup ([TapeWriter::finish]) is a copy of the newest describe run, with the session it held
/// then. A session set up after that run and a disconnect come before the backup: read on to it, the copy is not a
/// third session.
#[test]
fn describe_backup_is_not_a_new_session() {
    let (recorder, clock) = Recorder::with_fake_clock(RecorderConfig::default());
    let sub = recorder.subscribe(Layers::ALL, 64);
    let mut link = recorder.link();
    link.session(&host_session(Medium::InProcessUsb, Framer::None, 64, "dev"));
    // a bundle hands over a describe block at once: the newest describe run, with the first session
    recorder.add_bundle(b"hash", b"bundle");
    *clock.lock().unwrap() += Duration::from_millis(1);
    let mut set_up = host_session(Medium::InProcessUsb, Framer::None, 64, "dev");
    set_up.device_info = RefVec::Slice { slice: &[1, 2, 3] };
    link.session(&set_up);
    link.link_event(&LinkEvent::Disconnected {
        reason: "done",
        keep_streams: false,
    });
    recorder.flush();

    let dir = std::env::temp_dir().join(format!("ww_decode_backup_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.wwtape");
    let _ = std::fs::remove_file(&path);
    let mut writer = TapeWriter::create(&path, SyncPolicy::None).unwrap();
    while let Some(chunk) = sub.try_recv() {
        writer.write_block(chunk.bytes()).unwrap();
    }
    writer.finish().unwrap();

    let mut decoder = Decoder::default();
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
    assert_eq!(seen, ["session 0", "session 3", "disconnected"]);
    // a reader that starts at the backup never read the run's own describe blocks: the copy's session is news
    let bytes = std::fs::read(&path).unwrap();
    let mut blocks = FileBlocks::new(&bytes[..]);
    let mut all = vec![];
    while let Some(b) = blocks.next_good(|e| panic!("{e}")) {
        all.push(b.bytes().to_vec());
    }
    let copy = |b: &Vec<u8>| ww_tape::Block::read(b).unwrap().is_copy();
    let last = all.iter().rposition(copy).unwrap();
    let first = all[..=last].iter().rposition(|b| !copy(b)).unwrap() + 1;
    let mut decoder = Decoder::default();
    for b in &all[first..] {
        decoder.feed(&ww_tape::Block::read(b).unwrap());
    }
    decoder.finish();
    let mut seen = vec![];
    while let Some(d) = decoder.next() {
        if let What::Session(s) = d.what {
            seen.push(s.device_info.len());
        }
    }
    assert_eq!(seen, [0]);
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
        if chunk.block().kind == BlockKind::Records {
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
    let mut decoder = Decoder::default();
    decoder.feed(&block);
    let d = decoder.next().unwrap();
    assert!(
        matches!(d.what, What::BadBody { seq, .. } if seq == block.seq),
        "{:?}",
        d.what
    );
    assert_eq!(decoder.stats.blocks_bad, 1);
}
