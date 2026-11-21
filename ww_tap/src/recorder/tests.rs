use super::*;
use crate::*;
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use ww_tape::{Record, RecordTrack};

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
        assert_ne!(
            b.first_track(),
            Some(RecordTrack::Definitions),
            "definitions come only as puts"
        );
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

/// The number of the track of `link`'s `stream`.
fn track(rec: &Recorder, link: u32, stream: u32) -> TrackNum {
    rec.track(link, stream).expect("the link's track")
}

/// The link id and stream id of track `track` of `rec`.
fn link_stream(rec: &Recorder, track: TrackNum) -> Option<(u32, u32)> {
    rec.inner
        .state
        .lock()
        .unwrap()
        .by_track
        .get(&track)
        .copied()
}

/// The records of `chunks` as (track, item bytes).
fn records(chunks: &[Chunk]) -> Vec<(RecordTrack, Vec<u8>)> {
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
        .filter(|(track, _)| track.is_definitions())
        .filter_map(|(_, item)| match Describe::from_ww_bytes(&item).unwrap() {
            Describe::Track(t) => Some(t.label.to_owned()),
            _ => None,
        })
        .collect()
}

/// Check that every data record of `stream` (a consumer's blocks in order) has its track's definitions and its
/// link's session put before it, once one was recorded. Links are found as a reader finds them, by the tracks'
/// definitions.
fn assert_whole(stream: &[Chunk]) {
    let mut defs = Definitions::new();
    let mut sessions = std::collections::BTreeSet::new();
    for c in stream {
        let b = c.block();
        if held(c) == Held::Puts {
            defs.take_block(&b).unwrap();
            for (track, _) in records(std::slice::from_ref(c)) {
                if let Some((link, ids::SESSION)) =
                    track.data().and_then(|t| link_stream_in(&defs, t))
                {
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
            let (link, _) = link_stream_in(&defs, track).unwrap();
            assert!(
                sessions.contains(&link),
                "link {link}'s session put before its data"
            );
        }
    }
}

/// Data records of `chunk` as (track, item bytes).
fn data(chunk: &Chunk) -> Vec<(TrackNum, Vec<u8>)> {
    assert_eq!(held(chunk), Held::Data);
    let b = chunk.block();
    b.records()
        .map(|r| {
            let Record { track, item, .. } = r.unwrap();
            (track.data().expect("a data track"), item.to_vec())
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
    assert_eq!(recs[0].0, track(&rec, 0, ids::TX_MESSAGES));
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

/// A writer storing the recorder's blocks reads the recorder's clock: syncs on the fake clock in tests.
#[test]
fn tape_clock_is_the_recorders() {
    let (rec, t) = Recorder::with_fake_clock(config());
    let clock = rec.tape_clock();
    let start = clock.now_ms().expect("a clock");
    *t.lock().unwrap() += Duration::from_millis(1500);
    assert_eq!(clock.now_ms(), Some(start + 1500));
    let mut w = ww_tape::TapeWriter::create_on(
        ww_tape::MemSink::new(),
        ww_tape::SyncPolicy::Interval(Duration::from_secs(1)),
    );
    w.set_clock(rec.tape_clock());
    assert_eq!(w.clock().now_ms(), Some(start + 1500));
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
            if track == self::track(&rec, 0, ids::EVENTS) {
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
        if track.is_definitions() {
            if let Describe::Bundle { bytes, .. } = Describe::from_ww_bytes(&item).unwrap() {
                bundles += 1;
                assert_eq!(
                    bytes.iter().map(|b| b.unwrap()).collect::<Vec<_>>(),
                    b"bundle bytes"
                );
            }
        } else {
            assert_eq!(track, RecordTrack::Data(self::track(&rec, 0, ids::SESSION)));
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
        assert_eq!(
            recs.last().unwrap().0,
            RecordTrack::Data(track(&rec, 0, ids::SESSION))
        );
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
                .all(|(t, _)| *t != RecordTrack::Data(track(&rec, 0, ids::SESSION)))
        );
    }
}

/// The [LinkEvent::Lost] events of `chunk` of `rec`: (link, records lost).
fn lost_events(rec: &Recorder, chunk: &Chunk) -> Vec<(u32, u64)> {
    data(chunk)
        .into_iter()
        .filter_map(|(track, item)| {
            let (link, ids::EVENTS) = link_stream(rec, track)? else {
                return None;
            };
            match LinkEventOwned::from_ww_bytes_owned(&item).ok()? {
                LinkEventOwned::Lost { records } => Some((link, records.0)),
                _ => None,
            }
        })
        .collect()
}

/// A consumer whose queue can't take a block with the puts in front of it gets the puts that fit and misses the
/// block: it never gets a records block without what the block needs; the miss is counted, and its next block comes
/// after a Lost event of the link (reading side review 3, point 1).
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
    // puts and a block on link 0: one place left, the second link's block needs two: the puts go alone
    second.message(Dir::Tx, 4, b"lost");
    rec.flush();
    let mut stream = drain(&sub);
    assert_eq!(kinds(&stream), [Held::Puts, Held::Data, Held::Puts]);
    assert!(track_labels(&stream[2..]).contains(&"link[1].tx_messages".to_owned()));
    let stats = sub.stats();
    assert_eq!((stats.blocks_missed, stats.records_lost), (1, 1));
    assert!(stats.behind_since.is_some());
    assert_eq!(rec.consumers(), [stats]);
    second.message(Dir::Tx, 4, b"next");
    rec.flush();
    let next = drain(&sub);
    assert_eq!(kinds(&next), [Held::Puts, Held::Data, Held::Data]);
    assert_eq!(lost_events(&rec, &next[1]), [(1, 1)]);
    assert_eq!(sub.stats().behind_since, None, "up to date again");
    stream.extend(next);
    assert_whole(&stream);
}

/// A consumer that misses a block now and then is not put everything again: what it took stays behind it, so the
/// next block it takes needs no puts but the events track its Lost event goes on; the Lost block takes the missed
/// block's seq.
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
    assert_eq!(kinds(&next), [Held::Puts, Held::Data, Held::Data]);
    assert_eq!(track_labels(&next[..1]), ["link[0].events"]);
    assert_eq!(lost_events(&rec, &next[1]), [(0, 1)]);
    let seqs: Vec<u64> = stream
        .iter()
        .chain(&next)
        .filter(|c| held(c) == Held::Data)
        .map(|c| c.block().seq)
        .collect();
    assert_eq!(
        seqs[2] - seqs[1],
        1,
        "the Lost block in the missed block's place"
    );
    stream.extend(next);
    assert_whole(&stream);
    let all = drain(&fast);
    assert_eq!(
        kinds(&all),
        [Held::Puts, Held::Data, Held::Data, Held::Data, Held::Data]
    );
    assert_eq!(fast.stats().blocks_missed, 0);
}

/// No queue is too small: a consumer asking for a queue of one block (it gets [QUEUE_MIN]), behind definitions
/// longer than a block, gets the puts a block at a time and then its data, as long as it reads (reading side review
/// 3, point 1).
#[test]
fn a_small_queue_is_never_starved() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    rec.add_bundle(b"tape", &[1; 3000]);
    rec.add_bundle(b"tap", &[2; 3000]);
    let mut link = rec.link();
    link.session(&session());
    let sub = rec.subscribe(Layers::MESSAGES, 1);
    let mut stream = Vec::new();
    for k in 0..16u8 {
        link.message(Dir::Tx, 4, &[k]);
        rec.flush();
        stream.extend(drain(&sub));
        if stream.iter().any(|c| {
            held(c) == Held::Data && !data(c).is_empty() && lost_events(&rec, c).is_empty()
        }) {
            break;
        }
    }
    assert!(
        stream
            .iter()
            .any(|c| held(c) == Held::Data && lost_events(&rec, c).is_empty()),
        "a block of data came: {:?}",
        kinds(&stream)
    );
    assert!(sub.stats().blocks_missed > 0, "the misses are counted");
    assert_whole(&stream);
}

/// A consumer that reads nothing is behind and its misses count up, for it and for the recorder; nothing is made
/// for it while its queue is full: the blocks of puts aren't made (reading side review 3, points 1 and 2).
#[test]
fn a_consumer_that_reads_nothing_is_counted_behind() {
    let (rec, t) = Recorder::with_fake_clock(config());
    let mut link = rec.link();
    link.session(&session());
    let sub = rec.subscribe(Layers::MESSAGES, 2);
    *t.lock().unwrap() += Duration::from_millis(5);
    for k in 0..10u8 {
        link.message(Dir::Tx, 4, &[k]);
        rec.flush();
    }
    let stats = sub.stats();
    assert!(stats.blocks_missed >= 8, "{stats:?}");
    assert_eq!(stats.records_lost, stats.blocks_missed);
    assert!(stats.behind_since.is_some());
    // the definitions a block needs are shared with the recorder's, not copied
    let st = rec.inner.state.lock().unwrap();
    let needs = Needs::of(&st, [st.links[0].tracks[ids::TX_MESSAGES as usize]]);
    for n in needs.records.iter().filter(|n| n.track.is_definitions()) {
        let held = st.defs.held(&n.slot).unwrap();
        assert!(Arc::ptr_eq(&n.item, &held.item));
    }
}

/// A link that goes says so in the stream, in a block whose tracks are still put: a relay of the stream drops the
/// link's tracks from its definitions, which don't grow with every link ever seen (reading side review 3, point 7).
#[test]
fn a_closed_link_is_said_in_the_stream_and_a_relay_drops_it() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let sub = rec.subscribe(Layers::MESSAGES, 64);
    let mut link = rec.link();
    link.session(&session());
    link.message(Dir::Tx, 4, b"x");
    drop(link);
    let stream = drain(&sub);
    assert_whole(&stream);
    let closed = stream
        .iter()
        .filter(|c| held(c) == Held::Data)
        .flat_map(data)
        .any(|(track, item)| {
            track == self::track(&rec, 0, ids::EVENTS)
                && LinkEventOwned::from_ww_bytes_owned(&item) == Ok(LinkEventOwned::Closed)
        });
    assert!(closed, "the stream says the link closed");
    let mut w = ww_tape::TapeWriter::create_on(ww_tape::MemSink::new(), ww_tape::SyncPolicy::None);
    let mut defs = Definitions::new();
    let mut dropped = 0;
    for c in &stream {
        w.relay_block(&mut defs, c.bytes()).unwrap();
        dropped += drop_closed_links(&mut defs, &c.block());
    }
    assert_eq!(dropped, 1);
    assert!(
        (0..7).all(|s| defs.track(track(&rec, 0, s)).is_none()),
        "the link's tracks are dropped"
    );
    assert_eq!(defs.tracks().count(), 0, "no other link was open");
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

/// A link that comes after every link closed is defined whole: its clocks, which the last link's tracks took with
/// them, come back with their numbers. A closed link's tracks keep their numbers; the new link's are others.
#[test]
fn a_link_after_every_link_closed_is_defined_whole() {
    let (rec, _t) = Recorder::with_fake_clock(config());
    let first = rec.link();
    let first_tracks: Vec<TrackNum> = (0..7).map(|s| track(&rec, 0, s)).collect();
    let clock = rec.inner.state.lock().unwrap().host;
    drop(first);
    let mut second = rec.link();
    second.session(&session());
    let sub = rec.subscribe(Layers::MESSAGES, 8);
    second.message(Dir::Tx, 4, b"x");
    rec.flush();
    let got = drain(&sub);
    assert_eq!(kinds(&got), [Held::Puts, Held::Data]);
    assert_whole(&got);
    assert_eq!(rec.inner.state.lock().unwrap().host, clock);
    assert_eq!(
        (0..7).map(|s| track(&rec, 0, s)).collect::<Vec<_>>(),
        first_tracks
    );
    assert!((0..7).all(|s| !first_tracks.contains(&track(&rec, 1, s))));
    assert_eq!(rec.track(2, 0), None);
    assert_eq!(rec.track(0, 7), None);
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
    assert!(records(&got[..1]).iter().any(|(t, item)| t.is_definitions()
        && matches!(
            Describe::from_ww_bytes(item).unwrap(),
            Describe::Bundle { .. }
        )));
}
