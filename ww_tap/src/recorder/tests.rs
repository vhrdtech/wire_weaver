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

/// What a block the recorder hands over holds.
#[derive(Debug, PartialEq, Eq)]
enum Held {
    /// Puts: open definitions and sessions a consumer doesn't have
    Puts,
    /// New definitions
    Definitions,
    /// Records of links
    Data,
}

fn held(chunk: &Chunk) -> Held {
    let b = chunk.block();
    assert_eq!(b.kind, BlockKind::Records);
    if b.is_repeat() {
        Held::Puts
    } else if b.first_track() == Some(0) {
        Held::Definitions
    } else {
        Held::Data
    }
}

/// The blocks of puts a new consumer starts with, taken off `sub`: everything handed to it on joining.
fn puts(sub: &Subscription) -> Vec<Chunk> {
    let mut out = Vec::new();
    while let Some(c) = sub.try_recv() {
        assert_eq!(held(&c), Held::Puts);
        out.push(c);
    }
    assert!(!out.is_empty(), "a consumer starts with puts");
    out
}

/// The records of `chunks` as (track, item bytes).
fn records(chunks: &[Chunk]) -> Vec<(u32, Vec<u8>)> {
    chunks
        .iter()
        .flat_map(|c| {
            c.block()
                .records()
                .map(|r| {
                    let r = r.unwrap();
                    (r.track, r.item.to_vec())
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The labels of the tracks defined in `chunks`.
fn track_labels(chunks: &[Chunk]) -> Vec<String> {
    records(chunks)
        .into_iter()
        .filter(|(track, _)| *track == 0)
        .filter_map(|(_, item)| match Describe::from_ww_bytes(&item).unwrap() {
            Describe::Track(t) => Some(t.label.to_owned()),
            _ => None,
        })
        .collect()
}

/// Data records of `chunk` as (track, item bytes).
fn data(chunk: &Chunk) -> Vec<(u32, Vec<u8>)> {
    assert_eq!(held(chunk), Held::Data);
    let b = chunk.block();
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
    // the puts a new consumer starts with, nothing else is pending
    puts(&sub);
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
    puts(&sub);
    let mut link = rec.link();
    link.message(Dir::Rx, 4, b"a");
    // a new link was added: its tracks are defined before the data that uses them
    assert_eq!(held(&sub.try_recv().unwrap()), Held::Definitions);
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
        if held(&c) == Held::Data {
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
    let mut kept: Vec<Chunk> = Vec::new();
    while let Some(c) = sub.try_recv() {
        kept.push(c);
    }
    // 4 blocks in the pool, all handed over (3 frames each) and held
    let delivered: usize = kept
        .iter()
        .filter(|c| held(c) == Held::Data)
        .map(|c| data(c).len())
        .sum();
    assert_eq!(delivered, 12);
    drop(kept);
    // blocks are back: the next record opens with the count of the lost ones
    link.frame(Dir::Rx, &frame);
    rec.flush();
    let mut events = Vec::new();
    let mut frames = 0;
    let mut puts = 0;
    while let Some(c) = sub.try_recv() {
        match held(&c) {
            Held::Data => {}
            Held::Puts => {
                puts += 1;
                continue;
            }
            Held::Definitions => continue,
        }
        for (track, item) in data(&c) {
            if track == track_number(0, ids::EVENTS) {
                events.push(LinkEventOwned::from_ww_bytes_owned(&item).unwrap());
            } else {
                frames += 1;
            }
        }
    }
    // 20 - 12 = 8 records were lost, no block: the consumer has everything, nothing is put again
    assert_eq!(puts, 0);
    assert_eq!(frames, 1);
    assert_eq!(events, [LinkEventOwned::Lost { records: UVlq64(8) }]);
}

#[test]
fn a_consumer_joining_is_put_tracks_bundles_and_the_newest_session() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    rec.add_bundle(&[1, 2, 3], b"bundle bytes");
    let mut link = rec.link();
    // the newest session is kept with nobody listening
    link.session(&session());
    link.link_ready(&[9, 9]);
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    let chunks = puts(&sub);
    let mut bundles = 0;
    let mut sessions = Vec::new();
    for (track, item) in records(&chunks) {
        if track == 0 {
            if let Describe::Bundle { bytes, .. } = Describe::from_ww_bytes(&item).unwrap() {
                bundles += 1;
                assert_eq!(
                    bytes.iter().map(|b| b.unwrap()).collect::<Vec<_>>(),
                    b"bundle bytes"
                );
            }
        } else {
            assert_eq!(track, track_number(0, ids::SESSION));
            sessions.push(SessionOwned::from_ww_bytes_owned(&item).unwrap());
        }
    }
    let tracks = track_labels(&chunks);
    assert_eq!(bundles, 1);
    assert_eq!(tracks.len(), 7);
    assert!(tracks.contains(&"link[0].tx_frames".to_owned()));
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].device_info, vec![9, 9]);
}

/// A new session is put to every consumer at once, never in a records block; the same session again isn't put.
#[test]
fn a_new_session_is_put_to_every_consumer() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let mut link = rec.link();
    let a = rec.subscribe(Layers::MESSAGES, 8);
    puts(&a);
    let b = rec.subscribe(Layers::MESSAGES, 8);
    puts(&b);
    link.session(&session());
    for sub in [&a, &b] {
        let c = sub.try_recv().unwrap();
        assert_eq!(held(&c), Held::Puts);
        assert_eq!(records(&[c])[0].0, track_number(0, ids::SESSION));
    }
    link.session(&session());
    link.message(Dir::Tx, 4, b"x");
    rec.flush();
    for sub in [&a, &b] {
        let recs = data(&sub.try_recv().unwrap());
        assert!(
            recs.iter()
                .all(|(t, _)| *t != track_number(0, ids::SESSION))
        );
        assert!(sub.try_recv().is_none());
    }
}

/// A consumer that misses a block (its queue full) has nothing behind it: every open definition and the newest
/// session are put to it before its next block; a consumer that missed nothing gets none of that.
#[test]
fn a_consumer_that_misses_a_block_is_put_everything_again() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let mut link = rec.link();
    link.session(&session());
    let slow = rec.subscribe(Layers::MESSAGES, 1);
    let fast = rec.subscribe(Layers::MESSAGES, 64);
    // the slow consumer's queue holds its puts: the block after them is missed
    link.message(Dir::Tx, 4, b"lost");
    rec.flush();
    puts(&slow);
    assert_eq!(held(&fast.try_recv().unwrap()), Held::Puts);
    assert_eq!(held(&fast.try_recv().unwrap()), Held::Data);
    link.message(Dir::Tx, 4, b"next");
    rec.flush();
    let again = slow.try_recv().unwrap();
    assert_eq!(held(&again), Held::Puts);
    assert_eq!(track_labels(std::slice::from_ref(&again)).len(), 7);
    assert!(
        records(&[again])
            .iter()
            .any(|(t, _)| *t == track_number(0, ids::SESSION))
    );
    assert_eq!(held(&fast.try_recv().unwrap()), Held::Data);
    assert!(fast.try_recv().is_none());
}

/// A closed link's tracks are closed: a consumer that joins after doesn't get them.
#[test]
fn a_closed_links_tracks_are_not_put() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let first = rec.link();
    let _second = rec.link();
    drop(first);
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    let tracks = track_labels(&puts(&sub));
    assert_eq!(tracks.len(), 7);
    assert!(tracks.iter().all(|t| t.starts_with("link[1].")));
}

#[test]
fn background_flush_hands_over_on_time() {
    let rec = Recorder::new(RecorderConfig {
        flush_time: Duration::from_millis(5),
        ..config()
    });
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    puts(&sub);
    let mut link = rec.link();
    link.message(Dir::Tx, 4, b"x");
    let mut got = 0;
    while got < 1 {
        let c = sub
            .recv_timeout(Duration::from_secs(2))
            .expect("flushed on time");
        if held(&c) == Held::Data {
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
    puts(&sub);
    link.link_ready(&[9, 9]);
    rec.add_bundle(&[1, 2, 3], b"bundle bytes");
    let mut kinds = Vec::new();
    while let Some(c) = sub.try_recv() {
        kinds.push(held(&c));
    }
    // the new session put, the open block with LinkReady, then the block of definitions carrying the bundle
    assert_eq!(kinds, [Held::Puts, Held::Data, Held::Definitions]);
}
