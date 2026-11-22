//! A capture for the decoder's tests: the [Tap] of each link writes what it is told as `ww_tape` blocks, on the
//! tracks and the tap clock of [ww_tap::tape]. Every flush puts the definitions and each link's newest session in a
//! block of puts ([ww_tape::FLAG_REPEAT]) before the records block, so a reader may start at any block of puts.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use wire_weaver::shrink_wrap::{RefVec, SerializeShrinkWrap, UVlq64};
use ww_tap::{Dir, Frame, LinkEvent, LinkIdentity, Message, Session, Tap, ids};
use ww_tape::{
    BlockId, BlockKind, BlockState, BlockWriter, ClockNum, ClockTime, Definitions, Describe,
    RecordRef, RecordTrack, SessionTag, Slot, TrackNum,
};

const BLOCK_SIZE: usize = 64 * 1024;

/// Writes the links it hands out into blocks, taken with [TestTape::blocks]. Clones share the capture.
#[derive(Clone)]
pub(crate) struct TestTape(Arc<Mutex<Inner>>);

enum Clock {
    Real(Instant),
    Fake(Arc<Mutex<Duration>>),
}

struct Link {
    tracks: Option<[TrackNum; ww_tap::STREAMS]>,
    identity: Option<LinkIdentity>,
    /// The newest session: time and bytes
    session: Option<(i64, Vec<u8>)>,
    /// frames tx, rx, messages tx, rx
    no: [u64; 4],
}

struct Inner {
    clock: Clock,
    defs: Definitions,
    tap_clock: ClockNum,
    links: Vec<Link>,
    current: Option<(Vec<u8>, BlockState)>,
    next: BlockId,
    blocks: Vec<Vec<u8>>,
}

impl TestTape {
    /// A capture on the real clock.
    pub fn new() -> Self {
        Self::with_clock(Clock::Real(Instant::now()))
    }

    /// A capture on a clock the test moves with the returned handle.
    pub fn with_fake_clock() -> (Self, Arc<Mutex<Duration>>) {
        let t = Arc::new(Mutex::new(Duration::ZERO));
        (Self::with_clock(Clock::Fake(t.clone())), t)
    }

    fn with_clock(clock: Clock) -> Self {
        let mut defs = Definitions::new();
        let tap_clock = ww_tap::define_clocks(&mut defs, 1_700_000_000_000_000_000);
        TestTape(Arc::new(Mutex::new(Inner {
            clock,
            defs,
            tap_clock,
            links: Vec::new(),
            current: None,
            next: BlockId::new(SessionTag::writable(7), 0),
            blocks: Vec::new(),
        })))
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap()
    }

    /// A new link; its tracks are defined at its first session, or numbered at its first record before one.
    pub fn link(&self) -> TestLink {
        let mut st = self.inner();
        st.links.push(Link {
            tracks: None,
            identity: None,
            session: None,
            no: [0; 4],
        });
        TestLink {
            tape: self.clone(),
            link: u32::try_from(st.links.len() - 1).unwrap(),
        }
    }

    /// A [crate::tap::TapSource] that hands out a new link of this capture each time.
    pub fn source(&self) -> crate::tap::TapSource {
        let tape = self.clone();
        crate::tap::TapSource::new(move || Box::new(tape.link()))
    }

    /// Define an API bundle by its hash.
    pub fn add_bundle(&self, hash: &[u8], bytes: &[u8]) {
        let mut st = self.inner();
        if st.defs.bundle(hash).is_none() {
            st.defs
                .define(&Describe::Bundle {
                    hash: RefVec::Slice { slice: hash },
                    bytes: RefVec::Slice { slice: bytes },
                })
                .unwrap();
        }
    }

    /// Write what is pending, after a block of puts.
    pub fn flush(&self) {
        self.inner().flush();
    }

    /// The number of link `link`'s `stream` track.
    pub fn track(&self, link: u32, stream: u32) -> Option<TrackNum> {
        Some(self.inner().links.get(link as usize)?.tracks?[stream as usize])
    }

    /// The blocks written so far, in order.
    pub fn blocks(&self) -> Vec<Vec<u8>> {
        self.inner().blocks.clone()
    }

    /// The blocks written so far back to back: a capture file.
    pub fn file(&self) -> Vec<u8> {
        self.blocks().concat()
    }
}

impl Inner {
    fn now(&self) -> i64 {
        let d = match &self.clock {
            Clock::Real(start) => start.elapsed(),
            Clock::Fake(t) => *t.lock().unwrap(),
        };
        i64::try_from(d.as_nanos()).unwrap()
    }

    fn define(&mut self, link: u32, identity: LinkIdentity) {
        let tracks = ww_tap::define_link_tracks(&mut self.defs, self.tap_clock, &identity);
        let l = &mut self.links[link as usize];
        l.tracks = Some(tracks);
        l.identity = Some(identity);
    }

    fn record(&mut self, link: u32, stream: u32, item: &impl SerializeShrinkWrap) {
        if self.links[link as usize].tracks.is_none() {
            self.define(link, LinkIdentity::numbered(link));
        }
        let mut buf = vec![0u8; BLOCK_SIZE];
        let bytes = item.to_ww_bytes(&mut buf).unwrap().to_vec();
        let at = ClockTime::new(self.tap_clock, self.now());
        let track = self.links[link as usize].tracks.unwrap()[stream as usize];
        if stream == ids::SESSION {
            // a state track: its newest item is put, never written in a records block
            self.links[link as usize].session = Some((at.ticks, bytes));
            self.flush();
            return;
        }
        for _ in 0..2 {
            let (mut buf, state) = match self.current.take() {
                Some(c) => c,
                None => {
                    let mut buf = vec![0u8; BLOCK_SIZE];
                    let w = BlockWriter::new(
                        &mut buf,
                        BlockKind::Records,
                        self.next,
                        &[self.tap_clock],
                    )
                    .unwrap();
                    let state = w.into_state();
                    (buf, state)
                }
            };
            let mut w = BlockWriter::from_state(&mut buf, state);
            let pushed = w.push(track, at, &[&bytes]).is_ok();
            let state = w.into_state();
            self.current = Some((buf, state));
            if pushed {
                return;
            }
            self.flush();
        }
        panic!("a record does not fit an empty block");
    }

    fn flush(&mut self) {
        let puts: Vec<(RecordTrack, Option<ClockTime>, Vec<u8>)> = {
            let mut slots: Vec<Slot> = self.defs.definitions().map(|(s, _)| *s).collect();
            slots.sort_by_key(put_order);
            let defs = slots.iter().filter_map(|s| {
                let held = self.defs.held(s)?;
                Some((RecordTrack::Definitions, None, held.item.to_vec()))
            });
            let sessions = self.links.iter().filter_map(|l| {
                let (t, bytes) = l.session.as_ref()?;
                let track = l.tracks?[ids::SESSION as usize];
                Some((
                    RecordTrack::Data(track),
                    Some(ClockTime::new(self.tap_clock, *t)),
                    bytes.clone(),
                ))
            });
            defs.chain(sessions).collect()
        };
        let records: Vec<RecordRef<'_>> = puts
            .iter()
            .map(|(track, at, item)| RecordRef {
                track: *track,
                at: *at,
                item,
            })
            .collect();
        // a block of puts takes the seq of the block before it
        let id = BlockId::new(self.next.session, self.next.seq.saturating_sub(1));
        let blocks = &mut self.blocks;
        ww_tape::open::write_records(
            &records,
            BLOCK_SIZE,
            ww_tape::FLAG_REPEAT,
            || id,
            |b| {
                blocks.push(b.to_vec());
                Ok::<(), ww_tape::DescribeError>(())
            },
        )
        .unwrap();
        if let Some((mut buf, state)) = self.current.take()
            && state.records() > 0
        {
            let len = BlockWriter::from_state(&mut buf, state).finish();
            buf.truncate(len);
            self.blocks.push(buf);
            self.next = self.next.next();
        }
    }
}

/// Where a definition goes in a block of puts: what the others name first.
fn put_order(slot: &Slot) -> u8 {
    match slot {
        Slot::Producer | Slot::Layout => 0,
        Slot::Clock { .. } => 1,
        Slot::Map { .. } => 2,
        Slot::Own { .. } => 3,
        _ => 4,
    }
}

/// The [Tap] of one link of a [TestTape].
pub(crate) struct TestLink {
    tape: TestTape,
    link: u32,
}

impl TestLink {
    fn next_no(&self, i: usize) -> u64 {
        let mut st = self.tape.inner();
        let no = &mut st.links[self.link as usize].no[i];
        *no += 1;
        *no - 1
    }
}

impl Tap for TestLink {
    fn session(&mut self, session: &Session<'_>) {
        let mut st = self.tape.inner();
        if st.links[self.link as usize].tracks.is_none() {
            let named = LinkIdentity::of(session, None);
            let taken = st.links.iter().any(|l| l.identity.as_ref() == Some(&named));
            let identity = if session.peer.is_empty() || taken {
                LinkIdentity::of(session, Some(self.link))
            } else {
                named
            };
            st.define(self.link, identity);
        }
        st.record(self.link, ids::SESSION, session);
    }

    fn frame(&mut self, dir: Dir, bytes: &[u8]) {
        let no = self.next_no(dir as usize);
        let f = Frame {
            no: UVlq64(no),
            bytes: RefVec::Slice { slice: bytes },
        };
        let stream = match dir {
            Dir::Tx => ids::TX_FRAMES,
            Dir::Rx => ids::RX_FRAMES,
        };
        self.tape.inner().record(self.link, stream, &f);
    }

    fn message(&mut self, dir: Dir, kind: u8, bytes: &[u8]) {
        let no = self.next_no(2 + dir as usize);
        let m = Message {
            no: UVlq64(no),
            kind,
            bytes: RefVec::Slice { slice: bytes },
        };
        let stream = match dir {
            Dir::Tx => ids::TX_MESSAGES,
            Dir::Rx => ids::RX_MESSAGES,
        };
        self.tape.inner().record(self.link, stream, &m);
    }

    fn link_event(&mut self, event: &LinkEvent<'_>) {
        self.tape.inner().record(self.link, ids::EVENTS, event);
    }
}
