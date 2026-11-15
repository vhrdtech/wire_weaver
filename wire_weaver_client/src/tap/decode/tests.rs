use std::time::Duration;

use wire_weaver::shrink_wrap::RefVec;
use ww_tap::{Framer, Medium};
use ww_tape::{FileBlocks, SyncPolicy, TapeWriter};

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
    std::fs::remove_dir_all(&dir).unwrap();
}
