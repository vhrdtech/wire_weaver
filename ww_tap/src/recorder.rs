//! [Recorder]: the std [Tap] that batches records into `ww_tape` blocks.
//!
//! Records go into a preallocated block (64 KiB by default) that is handed over when it is full or
//! [RecorderConfig::flush_time] after its first record, so a live monitor still feels live. A block carries only
//! its used bytes, and nothing is handed over when nothing is pending. Blocks come from a pool and go back to it once
//! every consumer dropped them, so the steady state allocates nothing. When every block is held by consumers that
//! fall behind, records are dropped and counted, and the next block opens with a [LinkEvent::Lost] on the link: a
//! gap is visible, never silent, and the link is never stalled.
//!
//! Describe blocks (the producer, the track of every link, API bundles added with [Recorder::add_bundle] and the
//! newest session of every link) are written before the first data block, when links are added, when a consumer
//! subscribes, after records were lost and every [RecorderConfig::describe_every], so a consumer joining late
//! decodes from its first block on.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::{Duration, Instant, SystemTime};

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use ww_tape::{
    Block, BlockKind, BlockState, BlockWriter, Describe, FullVersion, Producer, TrackDef, Version,
};

use crate::{Dir, Frame, LinkEvent, Message, Session, Tap, ids};

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
    /// How often describe blocks are repeated while there are consumers.
    pub describe_every: Duration,
    /// Flush on time from a background thread. Off for tests that drive [Recorder::poll_flush] themselves.
    pub background_flush: bool,
    /// What the producer is, for people (in the describe block).
    pub name: String,
}

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
            describe_every: Duration::from_secs(1),
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
    id: u64,
    recorder: Weak<Inner>,
}

impl Subscription {
    /// The next block, waiting for it; None once the recorder is gone.
    pub fn recv(&self) -> Option<Chunk> {
        self.rx.recv().ok()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Chunk, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }

    pub fn try_recv(&self) -> Option<Chunk> {
        self.rx.try_recv().ok()
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
    /// Bounded queue; a full one skips the block for this consumer (it sees the gap in block seq).
    Queue(mpsc::SyncSender<Chunk>),
    /// Called with every block, under the recorder's lock: must be quick. False unsubscribes.
    Fn(SinkFn),
}

struct Sink {
    id: u64,
    layers: Layers,
    kind: SinkKind,
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
            Clock::Fake(t) => *t.lock().unwrap(),
        }
    }
}

struct Current {
    buf: Arc<ChunkBuf>,
    state: BlockState,
    first_at: Duration,
}

struct LinkState {
    /// Newest session record bytes
    session: Option<Vec<u8>>,
    lost: u64,
    /// Every [RecorderLink] of it is gone: left out of describe blocks
    closed: bool,
}

struct State {
    pool: Vec<Arc<ChunkBuf>>,
    current: Option<Current>,
    seq: u64,
    links: Vec<LinkState>,
    bundles: Vec<(Vec<u8>, Vec<u8>)>,
    sinks: Vec<Sink>,
    next_sink: u64,
    describe_due: bool,
    last_describe: Option<Duration>,
    scratch: Vec<u8>,
    wall_ns_at_zero: u64,
}

/// Track number of a link's stream: links take 7 numbers each, from 1 on.
pub fn track_number(link: u32, stream: u32) -> u32 {
    1 + link * 7 + stream
}

/// Link id and stream id of a track number, the reverse of [track_number].
pub fn track_of(number: u32) -> Option<(u32, u32)> {
    let n = number.checked_sub(1)?;
    Some((n / 7, n % 7))
}

/// The API the tap's tracks are in: this crate.
pub fn api() -> FullVersion<'static> {
    FullVersion::new(
        env!("CARGO_PKG_NAME"),
        Version::new(
            const_parse(env!("CARGO_PKG_VERSION_MAJOR")),
            const_parse(env!("CARGO_PKG_VERSION_MINOR")),
            const_parse(env!("CARGO_PKG_VERSION_PATCH")),
        ),
    )
}

const fn const_parse(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut i = 0;
    let mut v = 0;
    while i < b.len() {
        v = v * 10 + (b[i] - b'0') as u32;
        i += 1;
    }
    v
}

const STREAM_NAMES: [&str; 7] = [
    "session",
    "events",
    "tx_frames",
    "rx_frames",
    "tx_messages",
    "rx_messages",
    "client_map",
];

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
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                pool: Vec::new(),
                current: None,
                seq: 0,
                links: Vec::new(),
                bundles: Vec::new(),
                sinks: Vec::new(),
                next_sink: 0,
                describe_due: true,
                last_describe: None,
                scratch: Vec::new(),
                wall_ns_at_zero,
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
            *inner.flusher.lock().unwrap() = Some(handle.thread().clone());
        }
        Recorder { inner }
    }

    pub fn config(&self) -> &RecorderConfig {
        &self.inner.config
    }

    /// Producer clock now, ns: the time base of the blocks.
    pub fn now_ns(&self) -> u64 {
        self.inner.clock.now().as_nanos() as u64
    }

    /// A new link: its tracks are added, records go through the returned [Tap].
    pub fn link(&self) -> RecorderLink {
        let mut st = self.inner.state.lock().unwrap();
        let id = st.links.len() as u32;
        st.links.push(LinkState {
            session: None,
            lost: 0,
            closed: false,
        });
        st.describe_due = true;
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

    /// Add an API bundle to the describe blocks (the tap's own types, a device's API), by its hash; once per hash.
    pub fn add_bundle(&self, hash: &[u8], bytes: &[u8]) {
        let mut st = self.inner.state.lock().unwrap();
        if st.bundles.iter().any(|(h, _)| h == hash) {
            return;
        }
        st.bundles.push((hash.to_vec(), bytes.to_vec()));
        st.describe_due = true;
        // handed over now, not with the next block: the block being filled may already hold the first
        // messages that need this bundle, and a describe block after it would come too late for a live reader
        if !st.sinks.is_empty() {
            self.inner.flush_current(&mut st);
            self.inner.describe(&mut st);
        }
    }

    /// Blocks into a bounded queue of `queue` blocks: a consumer that falls further behind misses blocks (a gap in
    /// their seq). Starts with a describe block.
    pub fn subscribe(&self, layers: Layers, queue: usize) -> Subscription {
        let (tx, rx) = mpsc::sync_channel(queue.max(1));
        let id = self.inner.add_sink(layers, SinkKind::Queue(tx));
        Subscription {
            rx,
            id,
            recorder: Arc::downgrade(&self.inner),
        }
    }

    /// Every block to `f`, called under the recorder's lock (keep it short); `f` returns false to unsubscribe.
    /// Starts with a describe block.
    pub fn subscribe_fn(&self, layers: Layers, f: impl FnMut(&Chunk) -> bool + Send + 'static) {
        self.inner.add_sink(layers, SinkKind::Fn(Box::new(f)));
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
        let mut st = self.inner.state.lock().unwrap();
        self.inner.flush_current(&mut st);
    }

    /// Hand over the current block if its flush time is up; the background thread's job, for tests on a fake clock.
    pub fn poll_flush(&self) {
        self.inner.poll_flush();
    }

    fn record<T: SerializeShrinkWrap>(&self, link: u32, stream: u32, item: &T) {
        self.inner.record(track_number(link, stream), item);
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

    fn add_sink(&self, layers: Layers, kind: SinkKind) -> u64 {
        let mut st = self.state.lock().unwrap();
        // what the others had pending goes to them, the new one starts with a description
        self.flush_current(&mut st);
        let id = st.next_sink;
        st.next_sink += 1;
        st.sinks.push(Sink { id, layers, kind });
        self.update_layers(&st);
        st.describe_due = true;
        self.describe(&mut st);
        id
    }

    fn unsubscribe(&self, id: u64) {
        let mut st = self.state.lock().unwrap();
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
        let mut st = self.state.lock().unwrap();
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
        if st.describe_due && !st.sinks.is_empty() {
            self.describe(st);
        }
        let Some(mut buf) = self.take_block(st) else {
            return false;
        };
        let seq = st.seq;
        st.seq += 1;
        let b = Arc::get_mut(&mut buf).expect("free block");
        let w = BlockWriter::new(&mut b.bytes, BlockKind::Data, seq, now);
        st.current = Some(Current {
            state: w.into_state(),
            buf,
            first_at: self.clock.now(),
        });
        if let Some(t) = self.flusher.lock().unwrap().as_ref() {
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
                let track = track_number(link as u32, ids::EVENTS);
                _ = self.push(st, track, now, &ev);
                st.describe_due = true;
            }
        }
        true
    }

    /// Serialize `item` into the scratch buffer and push it into the current block.
    fn push<T: SerializeShrinkWrap>(
        &self,
        st: &mut State,
        track: u32,
        now: u64,
        item: &T,
    ) -> Result<(), ()> {
        let bytes = encode(&mut st.scratch, item).map_err(|_| ())?;
        let len = bytes.len();
        let cur = st.current.as_mut().ok_or(())?;
        let b = Arc::get_mut(&mut cur.buf).expect("current block is not shared");
        let mut w = BlockWriter::from_state(&mut b.bytes, cur.state);
        let r = w.push(track, now, &[&st.scratch[..len]]);
        cur.state = w.into_state();
        r.map_err(|_| ())
    }

    fn record<T: SerializeShrinkWrap>(&self, track: u32, item: &T) {
        let mut st = self.state.lock().unwrap();
        let st = &mut *st;
        if st.sinks.is_empty() {
            return;
        }
        let now = self.now();
        for attempt in 0..2 {
            if !self.open(st, now) {
                break;
            }
            match self.push(st, track, now, item) {
                Ok(()) => return,
                Err(()) if attempt == 0 => self.flush_current(st),
                Err(()) => {
                    tracing::warn!("tap record does not fit an empty block, dropped");
                    return;
                }
            }
        }
        if let Some((link, _)) = track_of(track) {
            st.links[link as usize].lost += 1;
        }
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
        deliver(&mut st.sinks, &chunk);
        self.update_layers(st);
        if let Some(last) = st.last_describe
            && self.clock.now().saturating_sub(last) >= self.config.describe_every
        {
            st.describe_due = true;
        }
    }

    /// Write and hand over describe blocks (not pooled: rare, and bundles can be large).
    fn describe(&self, st: &mut State) {
        st.describe_due = false;
        st.last_describe = Some(self.clock.now());
        let now = self.now();
        let mut records: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut scratch = std::mem::take(&mut st.scratch);
        let mut add = |item: &Describe<'_>| {
            if let Ok(b) = encode(&mut scratch, item) {
                records.push((0, b.to_vec()));
            }
        };
        let producer = Describe::Producer(Producer {
            name: &self.config.name,
            wall_ns_at_zero: st.wall_ns_at_zero,
        });
        add(&producer);
        for (hash, bytes) in &st.bundles {
            let d = Describe::Bundle {
                hash: RefVec::Slice { slice: hash },
                bytes: RefVec::Slice { slice: bytes },
            };
            add(&d);
        }
        let api = api();
        let api_root = ww_tape::api_root_id(api.crate_id, api.version.major.0);
        for link in 0..st.links.len() as u32 {
            if st.links[link as usize].closed {
                continue;
            }
            for stream in 0..7u32 {
                let path = [UNib32(ids::LINK), UNib32(link), UNib32(stream)];
                let label = format!("link[{link}].{}", STREAM_NAMES[stream as usize]);
                let d = Describe::Track(TrackDef {
                    number: UNib32(track_number(link, stream)),
                    key: ww_tape::track_key(api_root, &[ids::LINK, link, stream]),
                    api,
                    bundle: RefVec::Slice { slice: &[] },
                    path: RefVec::Slice { slice: &path },
                    label: &label,
                    sticky: stream == ids::SESSION,
                });
                add(&d);
            }
        }
        for (link, l) in st.links.iter().enumerate() {
            if let Some(s) = &l.session
                && !l.closed
            {
                records.push((track_number(link as u32, ids::SESSION), s.clone()));
            }
        }
        st.scratch = scratch;
        // as many blocks as needed, each sized to what it carries
        let mut i = 0;
        while i < records.len() {
            let mut size = ww_tape::HEADER_LEN + ww_tape::CRC_LEN;
            let mut j = i;
            while j < records.len() {
                let r = records[j].1.len() + ww_tape::MAX_RECORD_HEAD;
                if j > i && size + r > self.config.block_size {
                    break;
                }
                size += r;
                j += 1;
            }
            let mut bytes = vec![0u8; size];
            let seq = st.seq;
            st.seq += 1;
            let mut w = BlockWriter::new(&mut bytes, BlockKind::Describe, seq, now);
            for (track, item) in &records[i..j] {
                w.push(*track, now, &[item]).expect("sized to fit");
            }
            let len = w.finish();
            let chunk = Chunk(Arc::new(ChunkBuf { bytes, len }));
            deliver(&mut st.sinks, &chunk);
            i = j;
        }
        self.update_layers(st);
    }
}

fn deliver(sinks: &mut Vec<Sink>, chunk: &Chunk) {
    sinks.retain_mut(|s| match &mut s.kind {
        SinkKind::Queue(tx) => match tx.try_send(chunk.clone()) {
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => true,
            Err(mpsc::TrySendError::Disconnected(_)) => false,
        },
        SinkKind::Fn(f) => f(chunk),
    });
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
            let mut st = inner.state.lock().unwrap();
            if let Some(l) = st.links.get_mut(self.link as usize) {
                l.closed = true;
                l.session = None;
            }
        }
    }
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
        let mut scratch = Vec::new();
        if let Ok(bytes) = encode(&mut scratch, session) {
            let bytes = bytes.to_vec();
            let mut st = self.recorder.inner.state.lock().unwrap();
            st.links[self.link as usize].session = Some(bytes);
        }
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
            let st = self.recorder.inner.state.lock().unwrap();
            st.links[self.link as usize].session.clone()
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
