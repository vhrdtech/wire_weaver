//! [Recorder]: the std [Tap] that batches records into `ww_tape` blocks.
//!
//! Records go into a preallocated block (64 KiB by default) that is handed over when it is full or
//! [RecorderConfig::flush_time] after its first record, so a live monitor still feels live. A block carries only
//! its used bytes, and nothing is handed over when nothing is pending. Blocks come from a pool and go back to it once
//! every consumer dropped them, so the steady state allocates nothing. When every block is held by consumers that
//! fall behind, records are dropped and counted, and the next block opens with a [LinkEvent::Lost] on the link: a
//! gap is visible, never silent, and the link is never stalled.
//!
//! Definitions (the clocks, the tracks of every link, API bundles added with [Recorder::add_bundle]) are records on
//! track 0, held in the recorder's [Definitions]. A link's session track is a state track: its newest session is
//! put, never written in a records block. Each consumer is handed what its blocks need by the near rule
//! ([ww_tape::writer::near]) applied to its own stream: before a records block, the definitions and sessions the
//! block's records need that don't lie within `near` behind in that consumer's stream are put in front of it, as
//! blocks of puts ([ww_tape::FLAG_REPEAT]). When the puts and the block don't fit its queue, the consumer gets the
//! blocks of puts that fit and misses the block (a gap in seq): what it misses is counted ([ConsumerStats], on its
//! [Subscription] and [Recorder::consumers]), and its next block comes after a [LinkEvent::Lost] of each link it
//! lost records of. No queue is too small: the puts go a block at a time. A consumer that joins has nothing behind it and gets what each
//! block needs as blocks come; a new session is put to every consumer at once. A closed link's tracks are closed: a
//! consumer gets them no more. Nothing is written by time. The producer is not a definition here: a writer of the
//! stream names it ([RecorderConfig::name]).
//!
//! The definitions number the tracks and clocks ([Definitions::define_track], [Definitions::define_clock]): the
//! recorder keeps each link's track numbers, and a link's tracks keep them when it closes. A reader finds a link and
//! stream by the track's definition ([link_stream_of]), never by its number ([Recorder::track] gives the number of a
//! link's stream).
//!
//! Record times are nanoseconds of the recorder's own clock (origin the recorder's start); a definition maps it to
//! the wall clock read once at the start.

use crate::locked::Locked;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::{Duration, Instant, SystemTime};

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use ww_tape::{
    Block, BlockId, BlockKind, BlockState, BlockWriter, ClockMap, ClockNum, ClockOrigin, ClockSpec,
    ClockTime, DefinitionKind, Definitions, Describe, ItemHash, Magnitude, MapSegment, RecordRef,
    RecordTrack, Segment, SessionTag, Slot, StatePolicy, TrackDef, TrackNum, TrackRepeat,
    TrackSpec,
    writer::near::{Near, NearPolicy, Rule},
};

use crate::{Dir, Frame, LinkEvent, Message, Session, Tap, api, ids};

/// Settings of a [Recorder].
#[derive(Clone, Debug)]
pub struct RecorderConfig {
    /// Capacity of a block; a block carries only its used bytes.
    pub block_size: usize,
    /// Most blocks in the pool, together held by the recorder and its consumers; records are lost (and counted)
    /// beyond.
    pub pool_blocks: usize,
    /// How long a record may wait in a block before the block is handed over.
    pub flush_time: Duration,
    /// Flush on time from a background thread. Off for tests that drive [Recorder::poll_flush] themselves.
    pub background_flush: bool,
    /// What the producer is, for people (in its definition).
    pub name: String,
}

/// The smallest queue of a [Recorder::subscribe]: a block of [LinkEvent::Lost] and the block after it. The blocks
/// of puts in front of them go a block at a time, so a consumer that reads is never starved.
pub const QUEUE_MIN: usize = 2;

/// Default capacity of a block.
pub const DEFAULT_BLOCK_SIZE: usize = 64 * 1024;
/// Default flush time: short enough for a live monitor, long enough to batch.
pub const DEFAULT_FLUSH_TIME: Duration = Duration::from_millis(10);

impl Default for RecorderConfig {
    fn default() -> Self {
        RecorderConfig {
            block_size: DEFAULT_BLOCK_SIZE,
            pool_blocks: 32,
            flush_time: DEFAULT_FLUSH_TIME,
            background_flush: true,
            name: "ww link tap".into(),
        }
    }
}

/// Which records a consumer wants; sessions and link events always come.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Layers {
    pub frames: bool,
    pub messages: bool,
}

impl Layers {
    /// Messages and events, the default.
    pub const MESSAGES: Layers = Layers {
        frames: false,
        messages: true,
    };
    pub const ALL: Layers = Layers {
        frames: true,
        messages: true,
    };
}

impl Default for Layers {
    fn default() -> Self {
        Layers::MESSAGES
    }
}

/// A block handed over by a [Recorder], shared by every consumer and back in the pool once all dropped it.
#[derive(Clone)]
pub struct Chunk(Arc<ChunkBuf>);

struct ChunkBuf {
    bytes: Vec<u8>,
    len: usize,
}

impl Chunk {
    /// The block's bytes, as written to a file or sent on.
    pub fn bytes(&self) -> &[u8] {
        &self.0.bytes[..self.0.len]
    }

    pub fn block(&self) -> Block<'_> {
        Block::read(self.bytes()).expect("the recorder writes good blocks")
    }
}

impl core::fmt::Debug for Chunk {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let b = self.block();
        write!(
            f,
            "Chunk({:?} seq {} records {} len {})",
            b.kind,
            b.seq,
            b.record_count,
            b.len()
        )
    }
}

/// Where a consumer gets the blocks, see [Recorder::subscribe].
pub struct Subscription {
    rx: mpsc::Receiver<Chunk>,
    /// Blocks in the queue: sent and not yet taken
    queued: Arc<AtomicUsize>,
    missed: Arc<Missed>,
    id: u64,
    recorder: Weak<Inner>,
}

/// What a consumer missed so far, and whether it is behind ([Subscription::stats], [Recorder::consumers]).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ConsumerStats {
    /// The consumer, as [Subscription::id] names it.
    pub id: u64,
    /// Records blocks it missed: its queue had no room for them after the puts in front of them.
    pub blocks_missed: u64,
    /// Records in those blocks.
    pub records_lost: u64,
    /// Since when ([Recorder::now_ns]'s clock) it is behind: it missed a block or a put and hasn't had a block whole
    /// since. None when it is up to date.
    pub behind_since: Option<Duration>,
}

/// A consumer's misses, shared with its [Subscription].
#[derive(Default)]
struct Missed {
    blocks: AtomicU64,
    records: AtomicU64,
    /// ns of the recorder's clock it is behind since, plus one; 0 when it isn't behind
    behind_since: AtomicU64,
}

impl Missed {
    fn stats(&self, id: u64) -> ConsumerStats {
        let since = self.behind_since.load(Ordering::Relaxed);
        ConsumerStats {
            id,
            blocks_missed: self.blocks.load(Ordering::Relaxed),
            records_lost: self.records.load(Ordering::Relaxed),
            behind_since: since.checked_sub(1).map(Duration::from_nanos),
        }
    }

    /// Behind from `now` ns on, unless it is already.
    fn behind(&self, now: u64) {
        _ = self.behind_since.compare_exchange(
            0,
            now.saturating_add(1),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }
}

impl Subscription {
    /// Its id among the recorder's consumers ([ConsumerStats::id]).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// What it missed so far and whether it is behind now.
    pub fn stats(&self) -> ConsumerStats {
        self.missed.stats(self.id)
    }

    /// The next block, waiting for it; None once the recorder is gone.
    pub fn recv(&self) -> Option<Chunk> {
        self.taken(self.rx.recv().ok())
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Chunk, mpsc::RecvTimeoutError> {
        let c = self.rx.recv_timeout(timeout)?;
        self.queued.fetch_sub(1, Ordering::Relaxed);
        Ok(c)
    }

    pub fn try_recv(&self) -> Option<Chunk> {
        self.taken(self.rx.try_recv().ok())
    }

    fn taken(&self, c: Option<Chunk>) -> Option<Chunk> {
        if c.is_some() {
            self.queued.fetch_sub(1, Ordering::Relaxed);
        }
        c
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(inner) = self.recorder.upgrade() {
            inner.unsubscribe(self.id);
        }
    }
}

/// Producer side of the link tap, see the [module docs](self). Cheap to clone; [Recorder::link] gives the [Tap] of
/// one link.
#[derive(Clone)]
pub struct Recorder {
    inner: Arc<Inner>,
}

type SinkFn = Box<dyn FnMut(&Chunk) -> bool + Send>;

enum SinkKind {
    /// Bounded queue of `cap` blocks, `queued` of them in it; a full one skips the block for this consumer (it sees
    /// the gap in block seq).
    Queue {
        tx: mpsc::SyncSender<Chunk>,
        queued: Arc<AtomicUsize>,
        cap: usize,
    },
    /// Called with every block, under the recorder's lock: must be quick. False unsubscribes.
    Fn(SinkFn),
}

struct Sink {
    id: u64,
    layers: Layers,
    kind: SinkKind,
    /// Bytes of the blocks it took: where its stream is
    at: u64,
    /// The definitions and sessions put to it, where in its stream
    near: Near,
    missed: Arc<Missed>,
    /// Records of each link it lost since its last block, for the [LinkEvent::Lost] in front of its next one
    lost: std::collections::BTreeMap<u32, u64>,
    /// The newest block it missed
    last_missed: Option<BlockId>,
}

impl Sink {
    /// Whether `blocks` more fit its queue.
    fn fits(&self, blocks: usize) -> bool {
        match &self.kind {
            SinkKind::Queue { queued, cap, .. } => {
                queued.load(Ordering::Relaxed).saturating_add(blocks) <= *cap
            }
            SinkKind::Fn(_) => true,
        }
    }
}

/// What became of a block handed to a consumer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Delivery {
    Taken,
    /// Its queue was full: it misses the block
    Missed,
    /// It is gone
    Gone,
}

struct Inner {
    state: Mutex<State>,
    config: RecorderConfig,
    frames: AtomicBool,
    messages: AtomicBool,
    clock: Clock,
    flusher: Mutex<Option<std::thread::Thread>>,
}

#[derive(Clone)]
enum Clock {
    Real(Instant),
    Fake(Arc<Mutex<Duration>>),
}

impl Clock {
    fn now(&self) -> Duration {
        match self {
            Clock::Real(t0) => t0.elapsed(),
            Clock::Fake(t) => *t.locked(),
        }
    }
}

impl ww_tape::Clock for Clock {
    fn now_ms(&self) -> u64 {
        self.now().as_millis() as u64
    }
}

struct Current {
    buf: Arc<ChunkBuf>,
    state: BlockState,
    first_at: Duration,
}

struct LinkState {
    /// Newest session record: its time, bytes and hash
    session: Option<(ClockTime, Arc<[u8]>, ItemHash)>,
    lost: u64,
    /// The number of each stream's track, by stream id ([ids])
    tracks: [TrackNum; STREAMS],
}

struct State {
    pool: Vec<Arc<ChunkBuf>>,
    current: Option<Current>,
    /// Session and seq of the next block: a random session per recorder
    next: BlockId,
    links: Vec<LinkState>,
    sinks: Vec<Sink>,
    next_sink: u64,
    /// The open definitions
    defs: Definitions,
    /// The recorder's clock, ns since it started: record times count it
    host: ClockNum,
    /// Wall clock at the recorder's start, ns since the Unix epoch: what the host clock maps to
    wall_ns_at_zero: u64,
    /// Link id and stream id of each track number the links were given
    by_track: std::collections::BTreeMap<TrackNum, (u32, u32)>,
    scratch: Vec<u8>,
}

/// Streams of a link, each a track: the ids of [ids] from 0 on.
const STREAMS: usize = 7;

/// Stable key of a link's stream, the [ww_tape::track_key] of its name path `link[link].<stream>` in [LinkTap](crate::LinkTap).
///
/// # Panics
///
/// When `stream` is not one of [ids]' streams.
pub fn track_key(link: u32, stream: u32) -> ww_tape::TrackKey {
    let name = ids::stream_name(stream).expect("stream is one of ids' streams");
    ww_tape::track_key(
        ww_tape::api_root_id(api().crate_id),
        &[
            Segment::Name("link"),
            Segment::Index(link),
            Segment::Name(name),
        ],
    )
}

/// Link id and stream id of the tap track `def` defines: its path `link[link].<stream>` in
/// [LinkTap](crate::LinkTap), with its key from [track_key]. None for any other track. How a reader finds a link's
/// stream: the number is the definitions', not the link's.
pub fn link_stream_of(def: &TrackDef<'_>) -> Option<(u32, u32)> {
    let ww_tape::Items::WireWeaver { path, .. } = &def.items else {
        return None;
    };
    let mut path = path.iter();
    let (Some(Ok(UNib32(ids::LINK))), Some(Ok(UNib32(link))), Some(Ok(UNib32(stream))), None) =
        (path.next(), path.next(), path.next(), path.next())
    else {
        return None;
    };
    ids::stream_name(stream)?;
    (def.key == track_key(link, stream)).then_some((link, stream))
}

/// Link id and stream id of tap track `track` held in `defs`, see [link_stream_of].
pub fn link_stream_in(defs: &Definitions, track: TrackNum) -> Option<(u32, u32)> {
    let item = defs.get(&Slot::Track { number: track })?;
    match Describe::from_ww_bytes(item) {
        Ok(Describe::Track(t)) => link_stream_of(&t),
        _ => None,
    }
}

/// Define in `defs` the recorder's clock and the wall clock it maps to, read `wall_ns_at_zero` at its start; the
/// recorder's clock's number.
fn define_clocks(defs: &mut Definitions, wall_ns_at_zero: u64) -> ClockNum {
    let api_root = ww_tape::api_root_id(api().crate_id);
    let host = defs
        .define_clock(ClockSpec {
            // a session of the recorder is a clock of its own
            key: ww_tape::clock_key(api_root, &[], wall_ns_at_zero),
            rate: Magnitude::pow10(9).into(),
            resolution: Magnitude::pow10(-9).into(),
            origin: ClockOrigin::SessionStart,
            label: "tap",
        })
        .expect("the recorder's clock definition is valid");
    let wall = defs
        .define_clock(ClockSpec {
            key: ww_tape::UNIX_CLOCK_KEY,
            rate: Magnitude::pow10(9).into(),
            resolution: Magnitude::pow10(-9).into(),
            origin: ClockOrigin::UnixEpoch,
            label: "wall",
        })
        .expect("the wall clock definition is valid");
    let segments = [MapSegment::offset(0, wall_ns_at_zero as i64)];
    defs.define(&Describe::Map(ClockMap {
        from: host,
        to: wall,
        segments: RefVec::Slice { slice: &segments },
    }))
    .expect("the map between two held clocks is valid");
    host
}

impl Default for Recorder {
    fn default() -> Self {
        Recorder::new(RecorderConfig::default())
    }
}

impl Recorder {
    pub fn new(config: RecorderConfig) -> Self {
        Self::with_clock(config, Clock::Real(Instant::now()))
    }

    /// A recorder on a clock the caller moves with the returned handle (tests), without the background flush.
    pub fn with_fake_clock(config: RecorderConfig) -> (Self, Arc<Mutex<Duration>>) {
        let t = Arc::new(Mutex::new(Duration::ZERO));
        let config = RecorderConfig {
            background_flush: false,
            ..config
        };
        (Self::with_clock(config, Clock::Fake(t.clone())), t)
    }

    fn with_clock(config: RecorderConfig, clock: Clock) -> Self {
        let wall_ns_at_zero = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
            .saturating_sub(clock.now().as_nanos() as u64);
        let mut defs = Definitions::new();
        let host = define_clocks(&mut defs, wall_ns_at_zero);
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                pool: Vec::new(),
                current: None,
                next: BlockId::new(SessionTag::random(), 0),
                links: Vec::new(),
                sinks: Vec::new(),
                next_sink: 0,
                defs,
                host,
                wall_ns_at_zero,
                by_track: std::collections::BTreeMap::new(),
                scratch: Vec::new(),
            }),
            frames: AtomicBool::new(false),
            messages: AtomicBool::new(false),
            clock,
            flusher: Mutex::new(None),
            config,
        });
        if inner.config.background_flush {
            let weak = Arc::downgrade(&inner);
            let flush_time = inner.config.flush_time;
            let handle = std::thread::Builder::new()
                .name("ww-tap-flush".into())
                .spawn(move || flush_thread(weak, flush_time))
                .expect("spawn the tap flush thread");
            *inner.flusher.locked() = Some(handle.thread().clone());
        }
        Recorder { inner }
    }

    pub fn config(&self) -> &RecorderConfig {
        &self.inner.config
    }

    /// The recorder's clock now, ns: what record times count.
    pub fn now_ns(&self) -> u64 {
        self.inner.clock.now().as_nanos() as u64
    }

    /// The recorder's clock as a [ww_tape::TapeWriter]'s ([ww_tape::TapeWriter::set_clock]): a writer storing the
    /// recorder's blocks syncs and checkpoints on the time its records count, the fake one in tests.
    pub fn tape_clock(&self) -> ww_tape::ClockRef {
        ww_tape::ClockRef::new(self.inner.clock.clone())
    }

    /// A new link: its tracks are added, records go through the returned [Tap].
    pub fn link(&self) -> RecorderLink {
        let mut st = self.inner.state.locked();
        let id = st.links.len() as u32;
        let tracks = Inner::define_link(&mut st, id);
        st.links.push(LinkState {
            session: None,
            lost: 0,
            tracks,
        });
        RecorderLink {
            recorder: self.clone(),
            link: id,
            shared: Arc::new(LinkShared {
                no: Default::default(),
                recorder: Arc::downgrade(&self.inner),
                link: id,
            }),
        }
    }

    /// Define an API bundle (the tap's own types, a device's API), by its hash; once per hash.
    pub fn add_bundle(&self, hash: &[u8], bytes: &[u8]) {
        let mut st = self.inner.state.locked();
        if st.defs.bundle(hash).is_some() {
            return;
        }
        // every records block needs the bundles: it is put in front of the block being filled, with the rest
        Inner::define(
            &mut st,
            &Describe::Bundle {
                hash: RefVec::Slice { slice: hash },
                bytes: RefVec::Slice { slice: bytes },
            },
        );
    }

    /// Blocks into a bounded queue of `queue` blocks, at least [QUEUE_MIN]: a consumer that falls further behind
    /// misses records blocks (counted in [Subscription::stats]), never a records block without the puts in front of
    /// it.
    pub fn subscribe(&self, layers: Layers, queue: usize) -> Subscription {
        let cap = queue.max(QUEUE_MIN);
        let (tx, rx) = mpsc::sync_channel(cap);
        let queued = Arc::new(AtomicUsize::new(0));
        let missed = Arc::new(Missed::default());
        let id = self.inner.add_sink(
            layers,
            SinkKind::Queue {
                tx,
                queued: queued.clone(),
                cap,
            },
            missed.clone(),
        );
        Subscription {
            rx,
            queued,
            missed,
            id,
            recorder: Arc::downgrade(&self.inner),
        }
    }

    /// What each consumer missed so far and whether it is behind now.
    pub fn consumers(&self) -> Vec<ConsumerStats> {
        let st = self.inner.state.locked();
        st.sinks.iter().map(|s| s.missed.stats(s.id)).collect()
    }

    /// Every block to `f`, called under the recorder's lock (keep it short); `f` returns false to unsubscribe.
    pub fn subscribe_fn(&self, layers: Layers, f: impl FnMut(&Chunk) -> bool + Send + 'static) {
        self.inner
            .add_sink(layers, SinkKind::Fn(Box::new(f)), Arc::default());
    }

    /// Whether anybody wants frames or messages; sessions and events are recorded either way.
    pub fn wants(&self) -> Layers {
        Layers {
            frames: self.inner.frames.load(Ordering::Relaxed),
            messages: self.inner.messages.load(Ordering::Relaxed),
        }
    }

    /// Hand over the current block now, if it has records.
    pub fn flush(&self) {
        let mut st = self.inner.state.locked();
        self.inner.flush_current(&mut st);
    }

    /// Hand over the current block if its flush time is up; the background thread's job, for tests on a fake clock.
    pub fn poll_flush(&self) {
        self.inner.poll_flush();
    }

    /// The number of the track of link `link`'s `stream` ([ids]); None for a link or stream there isn't. A closed
    /// link's tracks keep their numbers.
    pub fn track(&self, link: u32, stream: u32) -> Option<TrackNum> {
        let st = self.inner.state.locked();
        st.links
            .get(link as usize)?
            .tracks
            .get(stream as usize)
            .copied()
    }

    fn record<T: SerializeShrinkWrap>(&self, link: u32, stream: u32, item: &T) {
        self.inner.record(link, stream, item);
    }
}

fn flush_thread(inner: Weak<Inner>, flush_time: Duration) {
    loop {
        let wait = match inner.upgrade() {
            Some(inner) => inner.poll_flush(),
            None => return,
        };
        std::thread::park_timeout(wait.unwrap_or(flush_time * 10));
    }
}

impl Inner {
    fn now(&self) -> u64 {
        self.clock.now().as_nanos() as u64
    }

    fn add_sink(&self, layers: Layers, kind: SinkKind, missed: Arc<Missed>) -> u64 {
        let mut st = self.state.locked();
        let id = st.next_sink;
        st.next_sink += 1;
        st.sinks.push(Sink {
            id,
            layers,
            kind,
            at: 0,
            near: Near::default(),
            missed,
            lost: Default::default(),
            last_missed: None,
        });
        self.update_layers(&st);
        id
    }

    fn unsubscribe(&self, id: u64) {
        let mut st = self.state.locked();
        st.sinks.retain(|s| s.id != id);
        self.update_layers(&st);
    }

    fn update_layers(&self, st: &State) {
        let frames = st.sinks.iter().any(|s| s.layers.frames);
        let messages = st.sinks.iter().any(|s| s.layers.messages);
        self.frames.store(frames, Ordering::Relaxed);
        self.messages.store(messages, Ordering::Relaxed);
    }

    /// Returns how long until the next flush is due, None if nothing is pending.
    fn poll_flush(&self) -> Option<Duration> {
        let mut st = self.state.locked();
        let now = self.clock.now();
        let first_at = st.current.as_ref().map(|c| c.first_at)?;
        let due = first_at + self.config.flush_time;
        if now >= due {
            self.flush_current(&mut st);
            None
        } else {
            Some(due - now)
        }
    }

    /// A free block from the pool (or a new one while the pool may grow); None when all are held.
    fn take_block(&self, st: &mut State) -> Option<Arc<ChunkBuf>> {
        if let Some(i) = st.pool.iter().position(|b| Arc::strong_count(b) == 1) {
            return Some(st.pool.swap_remove(i));
        }
        let held = st.pool.len() + st.current.is_some() as usize;
        if held >= self.config.pool_blocks {
            return None;
        }
        Some(Arc::new(ChunkBuf {
            bytes: vec![0; self.config.block_size],
            len: 0,
        }))
    }

    /// Start a data block if there is none. False when no block is free.
    fn open(&self, st: &mut State, now: u64) -> bool {
        if st.current.is_some() {
            return true;
        }
        let Some(mut buf) = self.take_block(st) else {
            return false;
        };
        let id = st.next;
        st.next = id.next();
        let b = Arc::get_mut(&mut buf).expect("free block");
        let w = BlockWriter::new(&mut b.bytes, BlockKind::Records, id, &[st.host])
            .expect("a block buffer holds the header and the one clock");
        st.current = Some(Current {
            state: w.into_state(),
            buf,
            first_at: self.clock.now(),
        });
        if let Some(t) = self.flusher.locked().as_ref() {
            t.unpark();
        }
        // records lost while no block was free, on their links
        for link in 0..st.links.len() {
            let lost = st.links[link].lost;
            if lost > 0 {
                st.links[link].lost = 0;
                let ev = LinkEvent::Lost {
                    records: UVlq64(lost),
                };
                let track = st.links[link].tracks[ids::EVENTS as usize];
                _ = self.push(st, track, now, &ev);
            }
        }
        true
    }

    /// Serialize `item` into the scratch buffer and push it into the current block; its length in the scratch.
    fn push<T: SerializeShrinkWrap>(
        &self,
        st: &mut State,
        track: TrackNum,
        now: u64,
        item: &T,
    ) -> Result<usize, ()> {
        let bytes = encode(&mut st.scratch, item).map_err(|_| ())?;
        let len = bytes.len();
        let at = ClockTime::new(st.host, now as i64);
        let cur = st.current.as_mut().ok_or(())?;
        let b = Arc::get_mut(&mut cur.buf).expect("current block is not shared");
        let mut w = BlockWriter::from_state(&mut b.bytes, cur.state);
        let r = w.push(track, at, &[&st.scratch[..len]]);
        cur.state = w.into_state();
        r.map(|()| len).map_err(|_| ())
    }

    /// Record `item` on link `link`'s `stream`.
    fn record<T: SerializeShrinkWrap>(&self, link: u32, stream: u32, item: &T) {
        let mut st = self.state.locked();
        let st = &mut *st;
        let now = self.now();
        let Some(&track) = st
            .links
            .get(link as usize)
            .and_then(|l| l.tracks.get(stream as usize))
        else {
            tracing::warn!("tap record of link {link} stream {stream}: no such track, dropped");
            return;
        };
        // the newest session of a link is kept whether anyone listens or not, and put: after the records before it
        if stream == ids::SESSION {
            let host = st.host;
            if let Ok(bytes) = encode(&mut st.scratch, item)
                && let Some(l) = st.links.get_mut(link as usize)
            {
                let at = ClockTime::new(host, now as i64);
                l.session = Some((at, Arc::from(bytes), ItemHash::of(bytes)));
            }
            self.flush_current(st);
            let id = BlockId::new(st.next.session, st.next.seq.saturating_sub(1));
            self.hand(st, &[track].into(), id, None);
            return;
        }
        if st.sinks.is_empty() {
            return;
        }
        for attempt in 0..2 {
            if !self.open(st, now) {
                break;
            }
            match self.push(st, track, now, item) {
                Ok(_) => return,
                Err(()) if attempt == 0 => self.flush_current(st),
                Err(()) => {
                    tracing::warn!("tap record does not fit an empty block, dropped");
                    return;
                }
            }
        }
        st.links[link as usize].lost += 1;
    }

    fn flush_current(&self, st: &mut State) {
        let Some(mut cur) = st.current.take() else {
            return;
        };
        if cur.state.records() == 0 {
            st.pool.push(cur.buf);
            return;
        }
        let b = Arc::get_mut(&mut cur.buf).expect("current block is not shared");
        let w = BlockWriter::from_state(&mut b.bytes, cur.state);
        b.len = w.finish();
        let chunk = Chunk(cur.buf.clone());
        st.pool.push(cur.buf);
        let block = chunk.block();
        let tracks = block
            .records()
            .filter_map(|r| r.ok()?.track.data())
            .collect();
        let id = BlockId::new(block.session, block.seq.saturating_sub(1));
        self.hand(st, &tracks, id, Some(&chunk));
    }

    /// Hold `d` in the open definitions; one they refuse is left out, and logged.
    fn define(st: &mut State, d: &Describe<'_>) {
        if let Err(e) = st.defs.define(d) {
            tracing::warn!("tap definition left out: {e}");
        }
    }

    /// Close the tracks of link `link`: no consumer gets them any more. They keep their numbers.
    fn close_link(st: &mut State, link: u32) {
        if let Some(l) = st.links.get_mut(link as usize) {
            l.session = None;
            for t in l.tracks {
                st.defs.close(t);
            }
        }
    }

    /// Define the tracks of link `link` and give their numbers, by stream id. The clocks come back first when the
    /// last link's tracks took them with them ([Definitions::close]).
    fn define_link(st: &mut State, link: u32) -> [TrackNum; STREAMS] {
        if !st.defs.has_clock(st.host) {
            st.host = define_clocks(&mut st.defs, st.wall_ns_at_zero);
        }
        let api = api();
        core::array::from_fn(|stream| {
            let stream = stream as u32;
            let path = [UNib32(ids::LINK), UNib32(link), UNib32(stream)];
            let label = format!(
                "link[{link}].{}",
                ids::stream_name(stream).expect("one of ids' streams")
            );
            let track = st
                .defs
                .define_track(TrackSpec {
                    key: track_key(link, stream),
                    clock: st.host,
                    dimensions: RefVec::Slice { slice: &[] },
                    items: ww_tape::Items::WireWeaver {
                        api,
                        bundle: RefVec::Slice { slice: &[] },
                        path: RefVec::Slice { slice: &path },
                        keys: RefVec::Slice { slice: &[] },
                        item: None,
                    },
                    label: &label,
                    repeat: if stream == ids::SESSION {
                        TrackRepeat::STATE
                    } else {
                        TrackRepeat::Stream
                    },
                    kind: ww_tape::TrackKind::Events(ww_tape::EventsDef::INSTANTS),
                    // zeros: the owner of the piece; the link's tracks belong to the capture itself
                    owner: [0; 32],
                    relations: RefVec::Slice { slice: &[] },
                })
                .expect("a link's track definition is valid and its clock held");
            st.by_track.insert(track, (link, stream));
            track
        })
    }

    /// Hand each consumer what records on `tracks` need that it doesn't have near ([Needs]), as blocks of puts
    /// numbered `id` (the seq of the block before), then `data`, after a [LinkEvent::Lost] of each link it lost
    /// records of since its last block. A consumer whose queue can't take them all gets the blocks of puts that fit
    /// and misses `data`, counted ([ConsumerStats]).
    fn hand(
        &self,
        st: &mut State,
        tracks: &std::collections::BTreeSet<TrackNum>,
        id: BlockId,
        data: Option<&Chunk>,
    ) {
        let block_size = self.config.block_size;
        let now = self.now();
        let needs = Needs::of(st, tracks.iter().copied());
        // a consumer that lost records gets a block of Lost events on those links' events tracks: their needs too
        let lost_links: std::collections::BTreeSet<u32> = st
            .sinks
            .iter()
            .flat_map(|s| s.lost.keys().copied())
            .collect();
        let events = |link: &u32| Some(st.links.get(*link as usize)?.tracks[ids::EVENTS as usize]);
        let with_lost = (data.is_some() && !lost_links.is_empty()).then(|| {
            let events = lost_links.iter().filter_map(events);
            Needs::of(st, tracks.iter().copied().chain(events))
        });
        let records = data
            .map(|c| records_by_link(&st.by_track, &c.block()))
            .unwrap_or_default();
        let events: std::collections::BTreeMap<u32, TrackNum> = lost_links
            .iter()
            .filter_map(|l| Some((*l, events(l)?)))
            .collect();
        let host = st.host;
        st.sinks.retain_mut(|sink| {
            let lost = match data {
                Some(_) if !sink.lost.is_empty() => lost_block(sink, &events, host, now),
                _ => None,
            };
            let needs = match (&lost, &with_lost) {
                (Some(_), Some(n)) => n,
                _ => &needs,
            };
            let served = serve(sink, needs, block_size, id, lost.as_ref(), data);
            match served {
                Served::Gone => return false,
                Served::Whole => {
                    if data.is_some() {
                        sink.lost.clear();
                        sink.last_missed = None;
                        sink.missed.behind_since.store(0, Ordering::Relaxed);
                    }
                }
                Served::Missed => {
                    sink.missed.behind(now);
                    if let Some(c) = data {
                        let b = c.block();
                        sink.missed.blocks.fetch_add(1, Ordering::Relaxed);
                        let n: u64 = records.values().sum();
                        sink.missed.records.fetch_add(n, Ordering::Relaxed);
                        for (link, n) in &records {
                            *sink.lost.entry(*link).or_default() += n;
                        }
                        sink.last_missed = Some(BlockId::new(b.session, b.seq));
                    }
                }
            }
            true
        });
        self.update_layers(st);
    }
}

/// What became of what a consumer was handed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Served {
    /// It took all of it (nothing, when nothing was due)
    Whole,
    /// Its queue had no room for all of it: it took the blocks of puts that fit, or none
    Missed,
    /// It is gone
    Gone,
}

/// Hand `sink` the blocks of puts of `needs` due to it, then `lost` and `data`; when its queue can't take them all,
/// the blocks of puts that fit, a block at a time, so the next block needs fewer. Its queue is looked at before
/// anything is made.
fn serve(
    sink: &mut Sink,
    needs: &Needs,
    block_size: usize,
    id: BlockId,
    lost: Option<&Chunk>,
    data: Option<&Chunk>,
) -> Served {
    let others = usize::from(lost.is_some()) + usize::from(data.is_some());
    let due = needs.due(sink, sink.at).len();
    if due == 0 && others == 0 {
        return Served::Whole;
    }
    if !sink.fits(1) {
        return Served::Missed;
    }
    let blocks = if due == 0 {
        Vec::new()
    } else {
        needs.puts(sink, block_size, id)
    };
    let whole = sink.fits(blocks.len() + others);
    for bytes in blocks {
        if !whole && !sink.fits(1) {
            return Served::Missed;
        }
        let chunk = chunk_of(bytes);
        match deliver_to(sink, &chunk) {
            Delivery::Taken => took_puts(sink, &chunk),
            Delivery::Missed => return Served::Missed,
            Delivery::Gone => return Served::Gone,
        }
    }
    if !whole {
        return Served::Missed;
    }
    for chunk in lost.into_iter().chain(data) {
        match deliver_to(sink, chunk) {
            Delivery::Taken => sink.at = sink.at.saturating_add(chunk.bytes().len() as u64),
            Delivery::Missed => return Served::Missed,
            Delivery::Gone => return Served::Gone,
        }
    }
    Served::Whole
}

/// Note the block of puts `chunk` as taken by `sink` where its stream is: each record of it written there.
fn took_puts(sink: &mut Sink, chunk: &Chunk) {
    let at = sink.at;
    let block = chunk.block();
    for r in block.records().flatten() {
        if let Some(slot) = Slot::of(r.track, r.item) {
            sink.near
                .wrote(slot, Some(ItemHash::of(r.item)), at, &NearPolicy::default());
        }
    }
    sink.at = sink.at.saturating_add(chunk.bytes().len() as u64);
}

/// Records of each link in `block`, the links' tracks by number in `by_track`.
fn records_by_link(
    by_track: &std::collections::BTreeMap<TrackNum, (u32, u32)>,
    block: &Block<'_>,
) -> std::collections::BTreeMap<u32, u64> {
    let mut out = std::collections::BTreeMap::new();
    for r in block.records().flatten() {
        if let Some((link, _)) = r.track.data().and_then(|t| by_track.get(&t)) {
            *out.entry(*link).or_default() += 1;
        }
    }
    out
}

/// A records block of a [LinkEvent::Lost] on the events track (by link in `events`) of each link `sink` lost
/// records of, at `now` ns of clock `host`, numbered as the newest block it missed (so its stream keeps the gap in
/// seq).
fn lost_block(
    sink: &Sink,
    events: &std::collections::BTreeMap<u32, TrackNum>,
    host: ClockNum,
    now: u64,
) -> Option<Chunk> {
    let id = sink.last_missed?;
    let items: Vec<(TrackNum, Vec<u8>)> = sink
        .lost
        .iter()
        .filter_map(|(link, n)| {
            let ev = LinkEvent::Lost {
                records: UVlq64(*n),
            };
            let mut scratch = Vec::new();
            let bytes = encode(&mut scratch, &ev).ok()?.to_vec();
            Some((*events.get(link)?, bytes))
        })
        .collect();
    let at = ClockTime::new(host, now as i64);
    let records: Vec<RecordRef<'_>> = items
        .iter()
        .map(|(track, item)| RecordRef::data(*track, at, item))
        .collect();
    let mut blocks = blocks_of(&records, usize::MAX >> 1, 0, || id);
    (blocks.len() == 1).then(|| chunk_of(blocks.remove(0)))
}

/// What a block's records need: the definitions of their tracks, the bundles, and the newest session of their links.
struct Needs {
    records: Vec<Needed>,
}

/// A definition or session a block needs.
struct Needed {
    slot: Slot,
    track: RecordTrack,
    at: Option<ClockTime>,
    /// Shared with the definitions or the link: nothing is copied or hashed until it is put
    item: Arc<[u8]>,
    hash: ItemHash,
    rule: Rule,
}

/// How the definitions are put to a consumer: the default policy, no copies. A consumer's stream isn't a medium:
/// whoever stores it puts by its own rule ([ww_tape::TapeWriter::put_block]).
const DEFINITIONS: StatePolicy = StatePolicy::DEFAULT;

/// The rule of `policy` in a consumer's stream: no copies.
fn rule(policy: StatePolicy) -> Rule {
    Rule {
        near: exact_units::Bytes::new(u64::from(policy.near.get())),
        copies: exact_units::Count::new(0),
        spacing: exact_units::Bytes::new(u64::MAX),
    }
}

impl Needs {
    /// What records on `tracks` need, each once, in the order they are put: what the others name first.
    fn of(st: &State, tracks: impl IntoIterator<Item = TrackNum>) -> Needs {
        let tracks: std::collections::BTreeSet<TrackNum> = tracks.into_iter().collect();
        let links: std::collections::BTreeSet<u32> = tracks
            .iter()
            .filter_map(|t| Some(st.by_track.get(t)?.0))
            .collect();
        // a link's session is put with its track's definitions
        let sessions = links.iter().filter_map(|link| {
            let l = st.links.get(*link as usize)?;
            l.session.as_ref().map(|_| l.tracks[ids::SESSION as usize])
        });
        let defined: std::collections::BTreeSet<TrackNum> =
            tracks.iter().copied().chain(sessions).collect();
        let mut slots = Vec::new();
        for t in &defined {
            for s in st.defs.needs(*t) {
                if !slots.contains(&s) {
                    slots.push(s);
                }
            }
        }
        let bundles = st.defs.definitions().map(|(s, _)| *s).filter(|s| {
            matches!(
                s,
                Slot::Own {
                    kind: DefinitionKind::Bundle,
                    ..
                }
            )
        });
        for s in bundles {
            if !slots.contains(&s) {
                slots.push(s);
            }
        }
        slots.sort_by_key(put_order);
        let mut records: Vec<Needed> = slots
            .into_iter()
            .filter_map(|slot| {
                let held = st.defs.held(&slot)?;
                Some(Needed {
                    slot,
                    track: RecordTrack::Definitions,
                    at: None,
                    hash: held.hash,
                    item: held.item.clone(),
                    rule: rule(DEFINITIONS),
                })
            })
            .collect();
        for link in links {
            let Some(l) = st.links.get(link as usize) else {
                continue;
            };
            let track = l.tracks[ids::SESSION as usize];
            if let Some(policy) = st.defs.policy(track)
                && let Some((at, bytes, hash)) = &l.session
            {
                records.push(Needed {
                    slot: Slot::State { track },
                    track: RecordTrack::Data(track),
                    at: Some(*at),
                    item: bytes.clone(),
                    hash: *hash,
                    rule: rule(policy),
                });
            }
        }
        Needs { records }
    }

    /// The records due to `sink` with a block landing `pos` bytes on in its stream.
    fn due(&self, sink: &Sink, pos: u64) -> Vec<RecordRef<'_>> {
        self.records
            .iter()
            .filter(|n| sink.near.writes(&n.slot, n.hash, pos, n.rule))
            .map(|n| RecordRef {
                track: n.track,
                at: n.at,
                item: &n.item,
            })
            .collect()
    }

    /// The blocks of puts `sink` is due before the next block: near is measured to where that block lands, after
    /// the puts, so more may fall due as they grow.
    fn puts(&self, sink: &Sink, block_size: usize, id: BlockId) -> Vec<Vec<u8>> {
        let mut blocks = Vec::new();
        let mut due = 0;
        loop {
            let len: u64 = blocks.iter().map(|b: &Vec<u8>| b.len() as u64).sum();
            let records = self.due(sink, sink.at.saturating_add(len));
            if records.len() == due {
                return blocks;
            }
            due = records.len();
            blocks = blocks_of(&records, block_size, ww_tape::FLAG_REPEAT, || id);
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

/// `records` as record blocks of at most `block_size` bytes with header flags `flags`, numbered by `next_id`; a
/// record that fails to go into a block is left out, and logged.
fn blocks_of(
    records: &[RecordRef<'_>],
    block_size: usize,
    flags: u16,
    next_id: impl FnMut() -> BlockId,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let written = ww_tape::open::write_records(records, block_size, flags, next_id, |b| {
        out.push(b.to_vec());
        Ok::<(), ww_tape::DescribeError>(())
    });
    if let Err(e) = written {
        tracing::warn!("tap definitions not written: {e}");
    }
    out
}

fn chunk_of(bytes: Vec<u8>) -> Chunk {
    let len = bytes.len();
    Chunk(Arc::new(ChunkBuf { bytes, len }))
}

/// Hand `chunk` to `sink`.
fn deliver_to(sink: &mut Sink, chunk: &Chunk) -> Delivery {
    match &mut sink.kind {
        SinkKind::Queue { tx, queued, .. } => {
            // counted before it can be taken, so the count never goes below what the queue holds
            queued.fetch_add(1, Ordering::Relaxed);
            let d = match tx.try_send(chunk.clone()) {
                Ok(()) => return Delivery::Taken,
                Err(mpsc::TrySendError::Full(_)) => Delivery::Missed,
                Err(mpsc::TrySendError::Disconnected(_)) => Delivery::Gone,
            };
            queued.fetch_sub(1, Ordering::Relaxed);
            d
        }
        SinkKind::Fn(f) => {
            if f(chunk) {
                Delivery::Taken
            } else {
                Delivery::Gone
            }
        }
    }
}

fn encode<'s, T: SerializeShrinkWrap>(
    scratch: &'s mut Vec<u8>,
    item: &T,
) -> Result<&'s [u8], wire_weaver::shrink_wrap::Error> {
    if scratch.len() < 1024 {
        scratch.resize(1024, 0);
    }
    // grow until it fits; the scratch is kept, so the steady state doesn't allocate
    let len = loop {
        let r = item.to_ww_bytes(scratch).map(|b| b.len());
        match r {
            Ok(len) => break len,
            Err(_) if scratch.len() < (64 << 20) => {
                let n = scratch.len() * 2;
                scratch.resize(n, 0);
            }
            Err(e) => return Err(e),
        }
    };
    Ok(&scratch[..len])
}

/// The [Tap] of one link of a [Recorder]; clones share the link (e.g. a transport's tx and rx halves).
#[derive(Clone)]
pub struct RecorderLink {
    recorder: Recorder,
    link: u32,
    shared: Arc<LinkShared>,
}

/// Frame and message numbers per direction, shared by the clones of a [RecorderLink].
struct LinkShared {
    /// frames tx, rx, messages tx, rx
    no: [AtomicU64; 4],
    recorder: Weak<Inner>,
    link: u32,
}

impl Drop for LinkShared {
    fn drop(&mut self) {
        if let Some(inner) = self.recorder.upgrade() {
            // the stream says so, in a block handed over while the link's tracks are still defined
            inner.record(self.link, ids::EVENTS, &LinkEvent::Closed);
            let mut st = inner.state.locked();
            inner.flush_current(&mut st);
            Inner::close_link(&mut st, self.link);
        }
    }
}

/// Drop from `defs` the tracks of each link a [LinkEvent::Closed] in the records block `block` closes, as a relay of
/// the stream does once it has passed the block on, so its definitions don't grow with every link ever seen. A
/// link's tracks are found by their definitions in `defs` ([link_stream_in]). How many links were closed.
pub fn drop_closed_links(defs: &mut Definitions, block: &Block<'_>) -> usize {
    let mut closed = 0;
    for r in block.records().flatten() {
        let Some(track) = r.track.data() else {
            continue;
        };
        let Some((link, ids::EVENTS)) = link_stream_in(defs, track) else {
            continue;
        };
        if matches!(LinkEvent::from_ww_bytes(r.item), Ok(LinkEvent::Closed)) {
            let tracks: Vec<TrackNum> = defs
                .tracks()
                .filter(|t| link_stream_in(defs, *t).is_some_and(|(l, _)| l == link))
                .collect();
            for t in tracks {
                defs.close(t);
            }
            closed += 1;
        }
    }
    closed
}

impl core::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Recorder")
            .field("config", &self.inner.config)
            .finish_non_exhaustive()
    }
}

impl core::fmt::Debug for RecorderLink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RecorderLink({})", self.link)
    }
}

impl RecorderLink {
    pub fn id(&self) -> u32 {
        self.link
    }

    pub fn recorder(&self) -> &Recorder {
        &self.recorder
    }

    fn next_no(&self, dir: Dir, frame: bool) -> u64 {
        let i = if frame { 0 } else { 2 } + dir as usize;
        self.shared.no[i].fetch_add(1, Ordering::Relaxed)
    }
}

impl Tap for RecorderLink {
    fn wants_frames(&self) -> bool {
        self.recorder.inner.frames.load(Ordering::Relaxed)
    }

    fn wants_messages(&self) -> bool {
        self.recorder.inner.messages.load(Ordering::Relaxed)
    }

    fn session(&mut self, session: &Session<'_>) {
        self.recorder.record(self.link, ids::SESSION, session);
    }

    fn frame(&mut self, dir: Dir, bytes: &[u8]) {
        // numbered also when not recorded, so a consumer joining later sees the true frame numbers
        let no = self.next_no(dir, true);
        if !self.wants_frames() {
            return;
        }
        let f = Frame {
            no: UVlq64(no),
            bytes: RefVec::Slice { slice: bytes },
        };
        let stream = match dir {
            Dir::Tx => ids::TX_FRAMES,
            Dir::Rx => ids::RX_FRAMES,
        };
        self.recorder.record(self.link, stream, &f);
    }

    fn message(&mut self, dir: Dir, kind: u8, bytes: &[u8]) {
        let no = self.next_no(dir, false);
        if !self.wants_messages() {
            return;
        }
        let m = Message {
            no: UVlq64(no),
            kind,
            bytes: RefVec::Slice { slice: bytes },
        };
        let stream = match dir {
            Dir::Tx => ids::TX_MESSAGES,
            Dir::Rx => ids::RX_MESSAGES,
        };
        self.recorder.record(self.link, stream, &m);
    }

    fn link_event(&mut self, event: &LinkEvent<'_>) {
        self.recorder.record(self.link, ids::EVENTS, event);
    }
}

/// Set the `device_info` of a link's newest session once link setup is done, and record it.
impl RecorderLink {
    pub fn link_ready(&mut self, device_info: &[u8]) {
        let session = {
            let st = self.recorder.inner.state.locked();
            st.links[self.link as usize]
                .session
                .as_ref()
                .map(|(_, bytes, _)| bytes.clone())
        };
        if let Some(bytes) = session
            && let Ok(s) = Session::from_ww_bytes(&bytes)
        {
            let s = Session {
                device_info: RefVec::Slice { slice: device_info },
                ..s
            };
            self.session(&s);
        }
        self.link_event(&LinkEvent::LinkReady);
    }
}

#[cfg(test)]
mod tests;
