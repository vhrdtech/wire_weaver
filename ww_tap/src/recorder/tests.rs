use super::*;
use crate::*;
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use ww_tape::Record;

fn session() -> Session<'static> {
    Session {
        side: Side::Host,
        medium: Medium::InProcessUsb,
        framer: Framer::None,
        frame_size: UNib32(64),
        peer: "test",
        device_info: RefVec::Slice { slice: &[] },
    }
}

fn config() -> RecorderConfig {
    RecorderConfig {
        block_size: 1024,
        pool_blocks: 4,
        ..RecorderConfig::default()
    }
}

/// Data records of `chunk` as (track, item bytes).
fn data(chunk: &Chunk) -> Vec<(u32, Vec<u8>)> {
    let b = chunk.block();
    assert_eq!(b.kind, BlockKind::Data);
    b.records()
        .map(|r| {
            let Record { track, item, .. } = r.unwrap();
            (track, item.to_vec())
        })
        .collect()
}

#[test]
fn nothing_recorded_without_consumers() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let mut link = rec.link();
    assert!(!link.wants_messages());
    link.message(Dir::Tx, 4, b"hi");
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    // the describe block a new consumer starts with, nothing else is pending
    assert_eq!(sub.try_recv().unwrap().block().kind, BlockKind::Describe);
    rec.flush();
    assert!(sub.try_recv().is_none());
    // numbering went on while nobody listened
    link.message(Dir::Tx, 4, b"hi");
    rec.flush();
    let recs = data(&sub.try_recv().unwrap());
    let m = Message::from_ww_bytes(&recs[0].1).unwrap();
    assert_eq!(m.no, UVlq64(1));
    assert_eq!(recs[0].0, track_number(0, ids::TX_MESSAGES));
}

#[test]
fn flush_after_flush_time_on_the_fake_clock() {
    let (rec, t) = Recorder::with_fake_clock(config());
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    _ = sub.try_recv();
    let mut link = rec.link();
    link.message(Dir::Rx, 4, b"a");
    // a new link was added: its tracks are described before the data that uses them
    assert_eq!(sub.try_recv().unwrap().block().kind, BlockKind::Describe);
    rec.poll_flush();
    assert!(sub.try_recv().is_none(), "not due yet");
    *t.lock().unwrap() += DEFAULT_FLUSH_TIME / 2;
    link.message(Dir::Rx, 4, b"b");
    rec.poll_flush();
    assert!(sub.try_recv().is_none(), "due from the first record on");
    *t.lock().unwrap() += DEFAULT_FLUSH_TIME / 2;
    rec.poll_flush();
    let chunk = sub.try_recv().unwrap();
    assert_eq!(data(&chunk).len(), 2);
    // only the used bytes travel
    assert!(chunk.bytes().len() < 64);
    rec.poll_flush();
    assert!(sub.try_recv().is_none(), "no empty blocks");
}

#[test]
fn full_block_is_handed_over_and_the_record_goes_into_the_next() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let sub = rec.subscribe(Layers::ALL, 64);
    let mut link = rec.link();
    let frame = [0x55u8; 300];
    for _ in 0..7 {
        link.frame(Dir::Tx, &frame);
    }
    rec.flush();
    let mut frames = 0;
    let mut blocks = 0;
    while let Some(c) = sub.try_recv() {
        let b = c.block();
        assert!(b.len() <= 1024);
        if b.kind == BlockKind::Data {
            blocks += 1;
            frames += data(&c).len();
        }
    }
    assert_eq!(frames, 7);
    assert_eq!(blocks, 3, "three 300-byte frames fit a 1 KiB block");
}

#[test]
fn held_blocks_lose_records_counted_in_a_link_event() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    // never read: every block stays held
    let sub = rec.subscribe(Layers::ALL, 64);
    let mut link = rec.link();
    let frame = [1u8; 300];
    for _ in 0..20 {
        link.frame(Dir::Rx, &frame);
    }
    let mut held: Vec<Chunk> = Vec::new();
    while let Some(c) = sub.try_recv() {
        held.push(c);
    }
    // 4 blocks in the pool, all handed over (3 frames each) and held
    let delivered: usize = held
        .iter()
        .filter(|c| c.block().kind == BlockKind::Data)
        .map(|c| data(c).len())
        .sum();
    assert_eq!(delivered, 12);
    drop(held);
    // blocks are back: the next record opens with the count of the lost ones
    link.frame(Dir::Rx, &frame);
    rec.flush();
    let mut events = Vec::new();
    let mut frames = 0;
    while let Some(c) = sub.try_recv() {
        if c.block().kind != BlockKind::Data {
            continue;
        }
        for (track, item) in data(&c) {
            if track == track_number(0, ids::EVENTS) {
                events.push(LinkEventOwned::from_ww_bytes_owned(&item).unwrap());
            } else {
                frames += 1;
            }
        }
    }
    // 20 - 12 = 8 were lost
    assert_eq!(frames, 1);
    assert_eq!(events, [LinkEventOwned::Lost { records: UVlq64(8) }]);
}

#[test]
fn describe_carries_tracks_bundles_and_the_newest_session() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    rec.add_bundle(&[1, 2, 3], b"bundle bytes");
    let mut link = rec.link();
    link.session(&session());
    link.link_ready(&[9, 9]);
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    let d = sub.try_recv().unwrap();
    let b = d.block();
    assert_eq!(b.kind, BlockKind::Describe);
    let mut tracks = Vec::new();
    let mut bundles = 0;
    let mut sessions = Vec::new();
    for r in b.records() {
        let r = r.unwrap();
        if r.track == 0 {
            match Describe::from_ww_bytes(r.item).unwrap() {
                Describe::Track(t) => tracks.push(t.label.to_string()),
                Describe::Bundle { bytes, .. } => {
                    bundles += 1;
                    assert_eq!(
                        bytes.iter().map(|b| b.unwrap()).collect::<Vec<_>>(),
                        b"bundle bytes"
                    );
                }
                _ => {}
            }
        } else {
            assert_eq!(r.track, track_number(0, ids::SESSION));
            sessions.push(SessionOwned::from_ww_bytes_owned(r.item).unwrap());
        }
    }
    assert_eq!(bundles, 1);
    assert_eq!(tracks.len(), 7);
    assert_eq!(tracks[2], "link[0].tx_frames");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].device_info, vec![9, 9]);
}

#[test]
fn background_flush_hands_over_on_time() {
    let rec = Recorder::new(RecorderConfig {
        flush_time: Duration::from_millis(5),
        ..config()
    });
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    _ = sub.try_recv();
    let mut link = rec.link();
    link.message(Dir::Tx, 4, b"x");
    let mut got = 0;
    while got < 1 {
        let c = sub
            .recv_timeout(Duration::from_secs(2))
            .expect("flushed on time");
        if c.block().kind == BlockKind::Data {
            got += data(&c).len();
        }
    }
}

/// A bundle added while a block is being filled reaches a subscriber before that block: the block may already hold
/// the messages that need it, and a live reader decodes them as they come.
#[test]
fn bundle_added_mid_block_is_handed_over_at_once() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let mut link = rec.link();
    link.session(&session());
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    assert_eq!(sub.try_recv().unwrap().block().kind, BlockKind::Describe);
    link.link_ready(&[9, 9]);
    rec.add_bundle(&[1, 2, 3], b"bundle bytes");
    let mut kinds = Vec::new();
    while let Some(c) = sub.try_recv() {
        kinds.push(c.block().kind);
    }
    // the open block with the session and LinkReady, then the describe block carrying the bundle
    assert_eq!(kinds, [BlockKind::Data, BlockKind::Describe]);
}
