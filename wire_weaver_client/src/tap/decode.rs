//! The tap's decoder (TR-14): `ww_tape` blocks from a [Recorder](ww_tap::Recorder) or a capture file in, one
//! [Decoded] per frame, message and link event out, with
//! - frame ↔ message linkage: frames are walked into their pieces ([ww_framer::pieces()]) and reassembled with the
//!   framer's own rules, so every message knows the frame bytes it was built from or went out in, and every frame
//!   the messages in it;
//! - a cross-check of every reassembled message against the message the transport recorded: a difference is a framer
//!   bug, flagged on the message ([Check::Mismatch]);
//! - the link layer (`ww_link` kinds) and the API layer: requests and events explained with the device's API bundle
//!   ([crate::envelope]), replies paired with their request by seq, so a return value is explained by its method
//!   without being told which one;
//! - [Stats] of the framer and the link.
//!
//! Live and saved data go through the same code: [Decoder::feed] takes a block, wherever it came from.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use wire_weaver::shrink_wrap::{
    BufReader, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, UVlq32Backfill,
};
use ww_client_server::{Event, PathKind, Request};
use ww_link::DeviceInfoOwned;
use ww_self::ApiBundleOwned;
use ww_tap::{Dir, FrameOwned, LinkEventOwned, MessageOwned, SessionOwned, ids, track_of};
use ww_tape::{Block, BlockKind, Describe};

use super::reassembly::{FrameOutcome, FramePart, Reassembled, Reassembler, walker};
use super::stats::Stats;
use crate::DynResource;
use crate::envelope::{Envelope, event_variant, explain_event, explain_request, request_variant};

/// Settings of a [Decoder].
#[derive(Clone, Debug)]
pub struct DecoderConfig {
    /// How long (producer time) a message record waits for the frames it goes out in, or a reassembled message for
    /// its record, before it is given out without them.
    pub link_wait_ns: u64,
    /// Explain requests and events with the API bundle (the expensive step); off: titles only.
    pub explain: bool,
    /// The recorder gave frames as well as messages. Off (messages only), a framed link's messages are given out at
    /// once instead of waiting [DecoderConfig::link_wait_ns] for frames that were not recorded.
    pub frames: bool,
}

impl Default for DecoderConfig {
    fn default() -> Self {
        DecoderConfig {
            link_wait_ns: 200_000_000,
            explain: true,
            frames: true,
        }
    }
}

/// One thing the decoder found, in producer time order per link and direction.
#[derive(Clone, Debug)]
pub struct Decoded {
    /// None for what concerns no link (lost blocks).
    pub link: Option<u32>,
    /// Producer clock, ns.
    pub t: u64,
    pub what: What,
}

#[derive(Clone, Debug)]
pub enum What {
    Session(SessionOwned),
    LinkEvent(LinkEventOwned),
    Frame(FrameInfo),
    Message(MessageInfo),
    /// Blocks missing by their seq: records in them are not seen.
    BlocksLost {
        blocks: u64,
    },
}

#[derive(Clone, Debug)]
pub struct FrameInfo {
    pub dir: Dir,
    pub no: u64,
    pub bytes: Vec<u8>,
    /// Frames missing before this one by their numbers.
    pub gap: u64,
    pub outcome: FrameOutcome,
}

/// How a message compares with its frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// The transport's record and the framer's message are the same.
    Match,
    /// They differ: a framer bug; the framer's bytes are here, the record's in [MessageInfo::bytes].
    Mismatch {
        framer_kind: u8,
        framer_bytes: Vec<u8>,
    },
    /// No frames for it (a medium without frames, frames not recorded, or they didn't come in time).
    NoFrames,
    /// The framer gave it back, but the transport recorded nothing (messages not recorded, or lost).
    NoRecord,
}

#[derive(Clone, Debug)]
pub struct MessageInfo {
    pub dir: Dir,
    /// The record's number, None for [Check::NoRecord].
    pub no: Option<u64>,
    pub kind: u8,
    pub bytes: Vec<u8>,
    /// The frame bytes it was built from or went out in.
    pub frames: Vec<FramePart>,
    pub check: Check,
    /// `ww_link` kind name.
    pub link_kind: &'static str,
    /// A data message read as a request or an event.
    pub api: Option<ApiInfo>,
}

#[derive(Clone)]
pub struct ApiInfo {
    /// What it is, e.g. `Request Call motor.set_speed`, `Event ReturnValue motor.get_speed`.
    pub title: String,
    pub seq: u32,
    /// For a reply: the request's message number and the time since it went out.
    pub reply_to: Option<(u64, u64)>,
    /// Field by field, when the decoder explains and the API bundle is known.
    pub envelope: Option<Arc<Envelope>>,
}

impl std::fmt::Debug for ApiInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiInfo")
            .field("title", &self.title)
            .field("seq", &self.seq)
            .field("reply_to", &self.reply_to)
            .finish_non_exhaustive()
    }
}

/// Looks an API bundle up by its hash pair when no capture carried it (the local cache).
pub type BundleLookup = Box<dyn Fn(&ww_version::ApiHashPairOwned) -> Option<ApiBundleOwned> + Send>;

struct Pending {
    no: u64,
    t: u64,
    kind: u8,
    bytes: Vec<u8>,
}

struct Built {
    t: u64,
    m: Reassembled,
}

#[derive(Default)]
struct DirState {
    reassembler: Option<Reassembler>,
    frames_seen: bool,
    next_frame_no: Option<u64>,
    /// Message records waiting for their frames
    records: VecDeque<Pending>,
    /// Reassembled messages waiting for their record
    built: VecDeque<Built>,
}

struct RequestSent {
    no: u64,
    t: u64,
    chain: Option<Vec<u32>>,
    what: String,
}

#[derive(Default)]
struct LinkState {
    session: Option<SessionOwned>,
    framer_error: Option<String>,
    device: Option<DeviceInfoOwned>,
    root: Option<DynResource>,
    root_tried: bool,
    /// The session came from a describe block, its record may still follow
    described: bool,
    tx: DirState,
    rx: DirState,
    requests: HashMap<u32, RequestSent>,
}

impl LinkState {
    fn dir(&mut self, dir: Dir) -> &mut DirState {
        match dir {
            Dir::Tx => &mut self.tx,
            Dir::Rx => &mut self.rx,
        }
    }
}

/// See the [module docs](self).
pub struct Decoder {
    config: DecoderConfig,
    links: BTreeMap<u32, LinkState>,
    /// API bundles by the hash bytes of their `ApiHashPair::no_docs`, from describe blocks or added
    bundles: HashMap<Vec<u8>, Arc<ApiBundleOwned>>,
    lookup: Option<BundleLookup>,
    next_seq: Option<u64>,
    wall_ns_at_zero: Option<u64>,
    out: VecDeque<Decoded>,
    pub stats: Stats,
}

impl Default for Decoder {
    fn default() -> Self {
        Decoder::new(DecoderConfig::default())
    }
}

impl Decoder {
    pub fn new(config: DecoderConfig) -> Self {
        Decoder {
            config,
            links: BTreeMap::new(),
            bundles: HashMap::new(),
            lookup: None,
            next_seq: None,
            wall_ns_at_zero: None,
            out: VecDeque::new(),
            stats: Stats::default(),
        }
    }

    /// Look device API bundles up here when no describe block carries them (e.g. the local cache).
    pub fn with_lookup(mut self, lookup: BundleLookup) -> Self {
        self.lookup = Some(lookup);
        self
    }

    /// Look bundles up in the local cache (`~/.wire_weaver`), where a client keeps what devices sent.
    pub fn with_local_cache(self) -> Self {
        self.with_lookup(Box::new(crate::local_registry::load))
    }

    /// Wall clock (ns since the Unix epoch) at producer time 0, once a describe block said it.
    pub fn wall_ns_at_zero(&self) -> Option<u64> {
        self.wall_ns_at_zero
    }

    /// The device API bundles the decoder knows, by hash, e.g. to write into a capture.
    pub fn bundles(&self) -> impl Iterator<Item = (&[u8], &Arc<ApiBundleOwned>)> {
        self.bundles.iter().map(|(h, b)| (h.as_slice(), b))
    }

    /// The device API of a link, once its bundle was found (by its first data message after the bundle came).
    pub fn link_api(&self, link: u32) -> Option<&DynResource> {
        self.links.get(&link)?.root.as_ref()
    }

    /// Decode a block; what it gives is taken with [Decoder::next].
    pub fn feed(&mut self, block: &Block<'_>) {
        self.stats.blocks += 1;
        if let Some(next) = self.next_seq
            && block.seq > next
        {
            let lost = block.seq - next;
            self.stats.blocks_lost += lost;
            self.out.push_back(Decoded {
                link: None,
                t: block.t_base,
                what: What::BlocksLost { blocks: lost },
            });
        }
        self.next_seq = Some(block.seq + 1);
        // a block stores its records track by track: back in time order, so a reply comes after its request
        let mut records: Vec<_> = block.records().filter_map(Result::ok).collect();
        records.sort_by_key(|r| r.t);
        for r in records {
            self.stats.at(r.t);
            if r.track == ww_tape::DESCRIBE_TRACK {
                if block.kind == BlockKind::Describe {
                    self.describe(r.item);
                }
                continue;
            }
            let Some((link, stream)) = track_of(r.track) else {
                continue;
            };
            // describe blocks repeat sessions: only a changed one is news
            if block.kind == BlockKind::Describe && stream == ids::SESSION {
                if let Ok(s) = SessionOwned::from_ww_bytes_owned(r.item)
                    && self.links.get(&link).and_then(|l| l.session.as_ref()) != Some(&s)
                {
                    self.session(link, r.t, s);
                    self.links.entry(link).or_default().described = true;
                }
                continue;
            }
            self.record(link, stream, r.t, r.item);
            self.expire(link, r.t);
        }
    }

    /// Give out what is still waiting (messages for frames that will not come), at the end of a capture.
    pub fn finish(&mut self) {
        let links: Vec<u32> = self.links.keys().copied().collect();
        for link in links {
            self.expire(link, u64::MAX);
        }
    }

    /// The next decoded thing, None when everything fed so far is out.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Decoded> {
        self.out.pop_front()
    }

    fn describe(&mut self, item: &[u8]) {
        let Ok(d) = Describe::from_ww_bytes(item) else {
            return;
        };
        match d {
            Describe::Producer(p) => self.wall_ns_at_zero = Some(p.wall_ns_at_zero),
            Describe::Bundle { hash, bytes } => {
                let hash = hash.as_slice().to_vec();
                if self.bundles.contains_key(&hash) {
                    return;
                }
                if let Ok(mut b) = ApiBundleOwned::from_ww_bytes_owned(bytes.as_slice())
                    && ww_self::generics::prepare(&mut b).is_ok()
                {
                    self.bundles.insert(hash, Arc::new(b));
                    self.retry_roots();
                }
            }
            _ => {}
        }
    }

    /// Add a device API bundle by the hash bytes of its `ApiHashPair::no_docs`.
    pub fn add_bundle(&mut self, hash: &[u8], mut bundle: ApiBundleOwned) {
        if ww_self::generics::prepare(&mut bundle).is_ok() {
            self.bundles.insert(hash.to_vec(), Arc::new(bundle));
            self.retry_roots();
        }
    }

    /// A new bundle may be the one a link didn't find before: look again at its next data message.
    fn retry_roots(&mut self) {
        for l in self.links.values_mut() {
            if l.root.is_none() {
                l.root_tried = false;
            }
        }
    }

    fn session(&mut self, link: u32, t: u64, s: SessionOwned) {
        let l = self.links.entry(link).or_default();
        if l.session.as_ref().map(|o| &o.framer) != Some(&s.framer) {
            for dir in [Dir::Tx, Dir::Rx] {
                l.dir(dir).reassembler = None;
            }
            match walker(&s.framer) {
                Ok(Some((w, stream))) => {
                    l.tx.reassembler = Some(Reassembler::new(w, stream));
                    let (w, _) = walker(&s.framer).ok().flatten().expect("same framer");
                    l.rx.reassembler = Some(Reassembler::new(w, stream));
                }
                Ok(None) => {}
                Err(e) => l.framer_error = Some(e),
            }
        }
        if !s.device_info.is_empty() {
            l.device = DeviceInfoOwned::from_ww_bytes_owned(&s.device_info).ok();
            l.root = None;
            l.root_tried = false;
        }
        l.session = Some(s.clone());
        self.out.push_back(Decoded {
            link: Some(link),
            t,
            what: What::Session(s),
        });
    }

    fn record(&mut self, link: u32, stream: u32, t: u64, item: &[u8]) {
        match stream {
            ids::SESSION => {
                if let Ok(s) = SessionOwned::from_ww_bytes_owned(item) {
                    let l = self.links.entry(link).or_default();
                    // a new link's describe block already gave its first session
                    if std::mem::take(&mut l.described) && l.session.as_ref() == Some(&s) {
                        return;
                    }
                    self.session(link, t, s);
                }
            }
            ids::EVENTS => {
                if let Ok(e) = LinkEventOwned::from_ww_bytes_owned(item) {
                    if let LinkEventOwned::Lost { records } = &e {
                        self.stats.records_lost += records.0;
                    }
                    self.stats.link(link).link_events += 1;
                    self.out.push_back(Decoded {
                        link: Some(link),
                        t,
                        what: What::LinkEvent(e),
                    });
                }
            }
            ids::TX_FRAMES | ids::RX_FRAMES => {
                let dir = if stream == ids::TX_FRAMES {
                    Dir::Tx
                } else {
                    Dir::Rx
                };
                if let Ok(f) = FrameOwned::from_ww_bytes_owned(item) {
                    self.frame(link, dir, t, f);
                }
            }
            ids::TX_MESSAGES | ids::RX_MESSAGES => {
                let dir = if stream == ids::TX_MESSAGES {
                    Dir::Tx
                } else {
                    Dir::Rx
                };
                if let Ok(m) = MessageOwned::from_ww_bytes_owned(item) {
                    self.message(link, dir, t, m);
                }
            }
            _ => {}
        }
    }

    fn frame(&mut self, link: u32, dir: Dir, t: u64, f: FrameOwned) {
        let l = self.links.entry(link).or_default();
        let frame_size = l
            .session
            .as_ref()
            .map(|s| s.frame_size.0 as usize)
            .unwrap_or(0);
        let d = l.dir(dir);
        d.frames_seen = true;
        let no = f.no.0;
        let gap = d.next_frame_no.map_or(0, |n| no.saturating_sub(n));
        d.next_frame_no = Some(no + 1);
        let outcome = match d.reassembler.as_mut() {
            Some(r) => r.feed(no, &f.bytes),
            None => FrameOutcome {
                error: Some((
                    0,
                    l.framer_error
                        .clone()
                        .unwrap_or_else(|| "no session with a framer for this link".into()),
                )),
                ..FrameOutcome::default()
            },
        };
        let ds = self.stats.link(link).dir(dir);
        ds.frame(f.bytes.len(), frame_size, &outcome);
        ds.frame_gaps += gap;
        let completed = outcome.completed.clone();
        self.out.push_back(Decoded {
            link: Some(link),
            t,
            what: What::Frame(FrameInfo {
                dir,
                no,
                bytes: f.bytes,
                gap,
                outcome,
            }),
        });
        let l = self.links.get_mut(&link).expect("added above");
        let d = l.dir(dir);
        for m in completed {
            d.built.push_back(Built { t, m });
        }
        self.pair(link, dir);
    }

    fn message(&mut self, link: u32, dir: Dir, t: u64, m: MessageOwned) {
        let kind_name = kind_name(m.kind);
        *self
            .stats
            .link(link)
            .dir(dir)
            .kinds
            .entry(kind_name)
            .or_default() += 1;
        let l = self.links.entry(link).or_default();
        let framed = l
            .session
            .as_ref()
            .is_some_and(|s| !matches!(s.framer, ww_tap::Framer::None));
        let d = l.dir(dir);
        d.records.push_back(Pending {
            no: m.no.0,
            t,
            kind: m.kind,
            bytes: m.bytes,
        });
        if !framed || !self.config.frames || (!d.frames_seen && dir == Dir::Rx) {
            // rx frames come before their messages: none seen means none will be
            let p = d.records.pop_front().expect("pushed");
            self.give_message(link, dir, p.t, Some(p), None);
            return;
        }
        self.pair(link, dir);
    }

    /// Match records with reassembled messages, in order.
    fn pair(&mut self, link: u32, dir: Dir) {
        loop {
            let l = self.links.get_mut(&link).expect("known link");
            let d = l.dir(dir);
            if d.records.is_empty() || d.built.is_empty() {
                return;
            }
            let p = d.records.pop_front().expect("checked");
            let b = d.built.pop_front().expect("checked");
            if dir == Dir::Tx {
                self.stats
                    .link(link)
                    .dir(dir)
                    .latency(b.t.saturating_sub(p.t));
            }
            let t = p.t.min(b.t);
            self.give_message(link, dir, t, Some(p), Some(b.m));
        }
    }

    /// Give out what waited longer than [DecoderConfig::link_wait_ns].
    fn expire(&mut self, link: u32, now: u64) {
        let wait = self.config.link_wait_ns;
        for dir in [Dir::Tx, Dir::Rx] {
            loop {
                let l = self.links.get_mut(&link).expect("known link");
                let d = l.dir(dir);
                let record_due = d
                    .records
                    .front()
                    .is_some_and(|p| p.t.saturating_add(wait) < now);
                let built_due = d
                    .built
                    .front()
                    .is_some_and(|b| b.t.saturating_add(wait) < now);
                // older first
                let take_record = match (record_due, built_due) {
                    (true, true) => d.records[0].t <= d.built[0].t,
                    (r, _) => r,
                };
                if take_record {
                    let p = d.records.pop_front().expect("due");
                    self.give_message(link, dir, p.t, Some(p), None);
                } else if built_due {
                    let b = d.built.pop_front().expect("due");
                    self.give_message(link, dir, b.t, None, Some(b.m));
                } else {
                    break;
                }
            }
        }
    }

    fn give_message(
        &mut self,
        link: u32,
        dir: Dir,
        t: u64,
        record: Option<Pending>,
        built: Option<Reassembled>,
    ) {
        let (no, kind, bytes, frames, check) = match (record, built) {
            (Some(p), Some(b)) => {
                let check = if b.kind == p.kind && b.bytes == p.bytes {
                    Check::Match
                } else {
                    self.stats.link(link).dir(dir).mismatches += 1;
                    Check::Mismatch {
                        framer_kind: b.kind,
                        framer_bytes: b.bytes,
                    }
                };
                (Some(p.no), p.kind, p.bytes, b.parts, check)
            }
            (Some(p), None) => (Some(p.no), p.kind, p.bytes, vec![], Check::NoFrames),
            (None, Some(b)) => (None, b.kind, b.bytes, b.parts, Check::NoRecord),
            (None, None) => return,
        };
        let api = if kind <= ww_link::Kind::Data2 as u8 && !bytes.is_empty() {
            self.api(link, dir, t, no.unwrap_or(u64::MAX), &bytes)
        } else {
            None
        };
        self.out.push_back(Decoded {
            link: Some(link),
            t,
            what: What::Message(MessageInfo {
                dir,
                no,
                kind,
                bytes,
                frames,
                check,
                link_kind: kind_name(kind),
                api,
            }),
        });
    }

    /// The device API of a link, once its DeviceInfo is known.
    fn root(&mut self, link: u32) -> Option<DynResource> {
        let l = self.links.get_mut(&link)?;
        if l.root.is_none() && !l.root_tried {
            let device = l.device.as_ref()?;
            l.root_tried = true;
            let key = device.hash.no_docs.hash.clone();
            let bundle = match self.bundles.get(&key) {
                Some(b) => Some(b.clone()),
                None => self
                    .lookup
                    .as_ref()
                    .and_then(|f| f(&device.hash))
                    .and_then(|mut b| {
                        ww_self::generics::prepare(&mut b).ok()?;
                        let b = Arc::new(b);
                        self.bundles.insert(key, b.clone());
                        Some(b)
                    }),
            };
            l.root = bundle.map(DynResource::root);
        }
        l.root.clone()
    }

    fn api(&mut self, link: u32, dir: Dir, t: u64, no: u64, bytes: &[u8]) -> Option<ApiInfo> {
        let root = self.root(link);
        let explain = self.config.explain;
        match dir {
            Dir::Tx => {
                // the seq alone pairs a reply, also when the rest doesn't read as a request
                let seq = BufReader::new(bytes).read::<UVlq32Backfill>().ok()?.0;
                let r = Request::from_ww_bytes(bytes).ok();
                let chain: Option<Vec<u32>> = match r.as_ref().map(|r| &r.path_kind) {
                    Some(PathKind::Absolute { path }) => {
                        path.iter().map(|p| p.ok().map(|p| p.0)).collect()
                    }
                    _ => None,
                };
                let envelope = root
                    .as_ref()
                    .filter(|_| explain && r.is_some())
                    .map(|root| Arc::new(explain_request(bytes, root)));
                let title = match (&envelope, &r) {
                    (Some(e), _) => e.title.clone(),
                    (None, Some(r)) => format!(
                        "Request {} {}",
                        request_variant(&r.kind),
                        chain.as_deref().map(ids_str).unwrap_or_default()
                    ),
                    (None, None) => "Request (not a ww_client_server request)".to_string(),
                };
                let what = title.strip_prefix("Request ").unwrap_or(&title).to_string();
                self.stats
                    .link(link)
                    .requests
                    .entry(what.clone())
                    .or_default()
                    .count += 1;
                if seq != 0 {
                    let l = self.links.get_mut(&link).expect("known");
                    l.requests.insert(seq, RequestSent { no, t, chain, what });
                }
                Some(ApiInfo {
                    title,
                    seq,
                    reply_to: None,
                    envelope,
                })
            }
            Dir::Rx => {
                let e = Event::from_ww_bytes(bytes).ok()?;
                let seq = e.seq.0;
                let sent = if seq != 0 {
                    self.links
                        .get_mut(&link)
                        .and_then(|l| l.requests.remove(&seq))
                } else {
                    None
                };
                let reply_to = sent.as_ref().map(|s| (s.no, t.saturating_sub(s.t)));
                if let (Some(s), Some((_, dt))) = (&sent, reply_to) {
                    let p = self
                        .stats
                        .link(link)
                        .requests
                        .entry(s.what.clone())
                        .or_default();
                    p.replies += 1;
                    p.reply_sum_ns += dt;
                    p.reply_max_ns = p.reply_max_ns.max(dt);
                }
                let for_resource = match (&root, sent.as_ref().and_then(|s| s.chain.as_ref())) {
                    (Some(root), Some(chain)) => root.descend(chain).ok(),
                    _ => None,
                };
                let envelope = root
                    .as_ref()
                    .filter(|_| explain)
                    .map(|root| Arc::new(explain_event(bytes, root, for_resource.as_ref())));
                let title = match &envelope {
                    Some(e) => e.title.clone(),
                    None => match &e.result {
                        Ok(k) => format!("Event {}", event_variant(k)),
                        Err(_) => "Event error".to_string(),
                    },
                };
                if seq == 0 {
                    *self
                        .stats
                        .link(link)
                        .events
                        .entry(title.clone())
                        .or_default() += 1;
                }
                Some(ApiInfo {
                    title,
                    seq,
                    reply_to,
                    envelope,
                })
            }
        }
    }
}

fn ids_str(chain: &[u32]) -> String {
    let ids: Vec<String> = chain.iter().map(|i| i.to_string()).collect();
    format!("[{}]", ids.join(", "))
}

/// `ww_link` kind name.
pub fn kind_name(kind: u8) -> &'static str {
    use ww_link::Kind::*;
    match ww_link::Kind::from_repr(kind) {
        Some(Data0) => "Data0",
        Some(Data1) => "Data1",
        Some(Data2) => "Data2",
        Some(Nop) => "Nop",
        Some(GetDeviceInfo) => "GetDeviceInfo",
        Some(DeviceInfo) => "DeviceInfo",
        Some(LinkSetup) => "LinkSetup",
        Some(LinkReady) => "LinkReady",
        Some(Ping) => "Ping",
        Some(GetStats) => "GetStats",
        Some(Stats) => "Stats",
        Some(Loopback) => "Loopback",
        Some(Disconnect) => "Disconnect",
        None => "unknown",
    }
}
