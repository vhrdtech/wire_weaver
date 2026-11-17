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
    /// Puts: definitions and sessions the next block needs
    Puts,
    /// Records of links
    Data,
}

fn held(chunk: &Chunk) -> Held {
    let b = chunk.block();
    assert_eq!(b.kind, BlockKind::Records);
    if b.is_repeat() {
        Held::Puts
    } else {
        assert_ne!(b.first_track(), Some(0), "definitions come only as puts");
        Held::Data
    }
}

/// Everything in `sub`'s queue.
fn drain(sub: &Subscription) -> Vec<Chunk> {
    std::iter::from_fn(|| sub.try_recv()).collect()
}

/// The kinds of `chunks`.
fn kinds(chunks: &[Chunk]) -> Vec<Held> {
    chunks.iter().map(held).collect()
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

/// Check that every data record of `stream` (a consumer's blocks in order) has its track's definitions and its
/// link's session put before it, once one was recorded.
fn assert_whole(stream: &[Chunk]) {
    let mut defs = Definitions::new();
    let mut sessions = std::collections::BTreeSet::new();
    for c in stream {
        let b = c.block();
        if held(c) == Held::Puts {
            defs.take_block(&b).unwrap();
            for (track, _) in records(std::slice::from_ref(c)) {
                if let Some((link, ids::SESSION)) = track_of(track) {
                    sessions.insert(link);
                }
            }
            continue;
        }
        for (track, _) in data(c) {
            for slot in defs.needs(track) {
                assert!(
                    defs.get(&slot).is_some(),
                    "{slot:?} put before track {track}"
                );
            }
            assert!(
                defs.track(track).is_some(),
                "track {track} put before its data"
            );
            let (link, _) = track_of(track).unwrap();
            assert!(
                sessions.contains(&link),
                "link {link}'s session put before its data"
            );
        }
    }
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
    link.session(&session());
    assert!(!link.wants_messages());
    link.message(Dir::Tx, 4, b"hi");
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    // a consumer that joins is handed nothing until a block needs it
    rec.flush();
    assert!(sub.try_recv().is_none());
    // numbering went on while nobody listened
    link.message(Dir::Tx, 4, b"hi");
    rec.flush();
    let got = drain(&sub);
    assert_eq!(kinds(&got), [Held::Puts, Held::Data]);
    let recs = data(&got[1]);
    let m = Message::from_ww_bytes(&recs[0].1).unwrap();
    assert_eq!(m.no, UVlq64(1));
    assert_eq!(recs[0].0, track_number(0, ids::TX_MESSAGES));
    assert_whole(&got);
}

#[test]
fn flush_after_flush_time_on_the_fake_clock() {
    let (rec, t) = Recorder::with_fake_clock(config());
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    let mut link = rec.link();
    link.message(Dir::Rx, 4, b"a");
    rec.poll_flush();
    assert!(sub.try_recv().is_none(), "not due yet");
    *t.lock().unwrap() += DEFAULT_FLUSH_TIME / 2;
    link.message(Dir::Rx, 4, b"b");
    rec.poll_flush();
    assert!(sub.try_recv().is_none(), "due from the first record on");
    *t.lock().unwrap() += DEFAULT_FLUSH_TIME / 2;
    rec.poll_flush();
    // the definitions the block needs are put in front of it
    let got = drain(&sub);
    assert_eq!(kinds(&got), [Held::Puts, Held::Data]);
    assert_eq!(data(&got[1]).len(), 2);
    // only the used bytes travel
    assert!(got[1].bytes().len() < 64);
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
    let mut put = Vec::new();
    while let Some(c) = sub.try_recv() {
        if held(&c) == Held::Puts {
            put.extend(track_labels(std::slice::from_ref(&c)));
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
    // 20 - 12 = 8 records were lost, no block: only the events track, new to the consumer, is put
    assert_eq!(put, ["link[0].events"]);
    assert_eq!(frames, 1);
    assert_eq!(events, [LinkEventOwned::Lost { records: UVlq64(8) }]);
}

/// A consumer that joins is put, in front of each block, what that block needs and it lacks: the bundles, the
/// clocks, the tracks of the block's records and their link's newest session, never every open definition.
#[test]
fn a_consumer_joining_is_put_what_each_block_needs() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    rec.add_bundle(&[1, 2, 3], b"bundle bytes");
    let mut link = rec.link();
    let mut other = rec.link();
    // the newest session is kept with nobody listening
    link.session(&session());
    link.link_ready(&[9, 9]);
    other.session(&session());
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    link.message(Dir::Tx, 4, b"x");
    rec.flush();
    let got = drain(&sub);
    assert_eq!(kinds(&got), [Held::Puts, Held::Data]);
    let mut bundles = 0;
    let mut sessions = Vec::new();
    for (track, item) in records(&got[..1]) {
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
    assert_eq!(bundles, 1);
    assert_eq!(
        track_labels(&got[..1]),
        ["link[0].session", "link[0].tx_messages"]
    );
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].device_info, vec![9, 9]);
    // the next block on the same tracks needs nothing more
    link.message(Dir::Tx, 4, b"y");
    rec.flush();
    let next = drain(&sub);
    assert_eq!(kinds(&next), [Held::Data]);
    assert_whole(&[got, next].concat());
}

/// A new session is put to every consumer at once, never in a records block; the same session again isn't put.
#[test]
fn a_new_session_is_put_to_every_consumer() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let mut link = rec.link();
    let a = rec.subscribe(Layers::MESSAGES, 8);
    let b = rec.subscribe(Layers::MESSAGES, 8);
    link.session(&session());
    for sub in [&a, &b] {
        let c = sub.try_recv().unwrap();
        assert_eq!(held(&c), Held::Puts);
        let recs = records(&[c]);
        assert_eq!(recs.last().unwrap().0, track_number(0, ids::SESSION));
        assert!(sub.try_recv().is_none());
    }
    link.session(&session());
    link.message(Dir::Tx, 4, b"x");
    rec.flush();
    for sub in [&a, &b] {
        let got = drain(sub);
        // the message track's definition is put; the session, the same, isn't
        assert_eq!(kinds(&got), [Held::Puts, Held::Data]);
        assert!(
            records(&got)
                .iter()
                .all(|(t, _)| *t != track_number(0, ids::SESSION))
        );
    }
}

/// A consumer whose queue can't take a block with the puts in front of it misses both: it never gets a records
/// block without what the block needs, and gets the puts with the next block it can take.
#[test]
fn a_consumer_without_room_for_the_puts_misses_the_block() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let mut first = rec.link();
    let mut second = rec.link();
    first.session(&session());
    second.session(&session());
    let sub = rec.subscribe(Layers::MESSAGES, 3);
    first.message(Dir::Tx, 4, b"a");
    rec.flush();
    // puts and a block on link 0: one place left, the second link's block needs two
    second.message(Dir::Tx, 4, b"lost");
    rec.flush();
    let mut stream = drain(&sub);
    assert_eq!(kinds(&stream), [Held::Puts, Held::Data]);
    second.message(Dir::Tx, 4, b"next");
    rec.flush();
    let next = drain(&sub);
    assert_eq!(kinds(&next), [Held::Puts, Held::Data]);
    assert!(track_labels(&next[..1]).contains(&"link[1].tx_messages".to_owned()));
    stream.extend(next);
    assert_whole(&stream);
}

/// A consumer that misses a block now and then is not put everything again: what it took stays behind it, so the
/// next block it takes needs no puts.
#[test]
fn a_consumer_that_misses_a_block_is_not_put_everything_again() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let mut link = rec.link();
    link.session(&session());
    let slow = rec.subscribe(Layers::MESSAGES, 3);
    let fast = rec.subscribe(Layers::MESSAGES, 64);
    for m in [b"one", b"two", b"3rd"] {
        link.message(Dir::Tx, 4, m);
        rec.flush();
    }
    // puts, one, two; three is missed
    let mut stream = drain(&slow);
    assert_eq!(kinds(&stream), [Held::Puts, Held::Data, Held::Data]);
    link.message(Dir::Tx, 4, b"next");
    rec.flush();
    let next = drain(&slow);
    assert_eq!(kinds(&next), [Held::Data]);
    let seqs: Vec<u64> = stream.iter().chain(&next).map(|c| c.block().seq).collect();
    assert_eq!(seqs[3] - seqs[2], 2, "the missed block is a gap in seq");
    stream.extend(next);
    assert_whole(&stream);
    let all = drain(&fast);
    assert_eq!(
        kinds(&all),
        [Held::Puts, Held::Data, Held::Data, Held::Data, Held::Data]
    );
}

/// A closed link's tracks are closed: a consumer is put the tracks of the open links only.
#[test]
fn a_closed_links_tracks_are_not_put() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let first = rec.link();
    let mut second = rec.link();
    second.session(&session());
    drop(first);
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    second.message(Dir::Tx, 4, b"x");
    rec.flush();
    let got = drain(&sub);
    let tracks = track_labels(&got);
    assert!(!tracks.is_empty());
    assert!(tracks.iter().all(|t| t.starts_with("link[1].")));
    assert_whole(&got);
}

#[test]
fn background_flush_hands_over_on_time() {
    let rec = Recorder::new(RecorderConfig {
        flush_time: Duration::from_millis(5),
        ..config()
    });
    let sub = rec.subscribe(Layers::MESSAGES, 8);
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

/// A bundle added while a block is being filled is put in front of that block: the block may already hold the
/// messages that need it, and a live reader decodes them as they come.
#[test]
fn bundle_added_mid_block_is_put_before_it() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let mut link = rec.link();
    link.session(&session());
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    link.message(Dir::Tx, 4, b"x");
    rec.add_bundle(&[1, 2, 3], b"bundle bytes");
    rec.flush();
    let got = drain(&sub);
    assert_eq!(kinds(&got), [Held::Puts, Held::Data]);
    assert!(records(&got[..1]).iter().any(|(t, item)| *t == 0
        && matches!(
            Describe::from_ww_bytes(item).unwrap(),
            Describe::Bundle { .. }
        )));
}
