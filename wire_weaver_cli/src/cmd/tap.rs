//! The link tap in the CLI (TOOL-14, on TR-14): `ww monitor` prints what crosses the link as it happens, `ww stats`
//! counts it, `ww capture -o FILE` saves it and `ww explain FILE` / `ww stats --file FILE` read a capture back. Live
//! and saved data go through the same [Monitor]: a capture prints exactly as the live session did.
//!
//! Where blocks come from is a [Feed]. Today that is the CLI's own connection ([LiveFeed]: the recorder of the client
//! this process opened) or a file ([FileFeed]); a ww server's tap of every client's link (SRV-13) is one more feed.

use crate::cmd::explain;
use crate::util::StopSignal;
use crate::value_fmt::fmt_value;
use anyhow::{Context, Result, anyhow};
use clap::{Args, ValueEnum, ValueHint};
use clap_complete::{ArgValueCandidates, CompletionCandidate};
use console::style;
use human_repr::HumanCount;
use indicatif::{ProgressBar, ProgressStyle};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wire_weaver_client::DynClient;
use wire_weaver_client::tap::{
    Check, Decoded, Decoder, DecoderConfig, Dir, FrameInfo, FrameOutcome, FrameReport, Layers,
    MessageInfo, PieceOutcome, Recorder, RecorderConfig, Subscription, What, decode::kind_name,
    stats::ns,
};
use wire_weaver_client::ww_self::ApiBundleOwned;
use wire_weaver_client::ww_self::explain::PartKind;
use wire_weaver_client::ww_tap::{LinkEventOwned, SessionOwned};
use wire_weaver_client::ww_tape::{
    Block, FileBlocks, MAGIC, Recovery, SyncPolicy, TapeWriter, WriteError,
};

mod flags;
pub(crate) use flags::{Attached, TapFlags};

/// How deep to look.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum Layer {
    /// Every frame (USB packet, UDP datagram, RTT chunk) with its pieces, and the messages built from them
    Frame,
    /// Every ww_link message (data, link setup, pings) and link event
    #[default]
    Link,
    /// The device's API: requests, replies paired with their request, events, with their values
    Api,
}

impl Layer {
    /// What the recorder needs to give for this layer.
    fn layers(self) -> Layers {
        match self {
            Layer::Frame => Layers::ALL,
            Layer::Link | Layer::Api => Layers::MESSAGES,
        }
    }
}

#[derive(Args)]
pub(crate) struct MonitorArgs {
    #[command(flatten)]
    view: ViewArgs,

    /// Stop after this many seconds (default: until Ctrl+C)
    #[arg(long, value_name = "S")]
    duration: Option<f64>,
}

/// How decoded things are printed, the same for `ww monitor` and `ww explain FILE`.
#[derive(Args, Clone, Copy)]
pub(crate) struct ViewArgs {
    /// How deep to look
    #[arg(long, value_enum, default_value_t)]
    pub layer: Layer,

    /// Also print the bytes of every frame and message, in hex
    #[arg(long)]
    pub bytes: bool,

    /// API layer: every message with its bit map and field table, as `ww explain request|event HEX` shows one
    #[arg(long)]
    pub bits: bool,
}

impl MonitorArgs {
    pub(crate) fn layer(&self) -> Layer {
        self.view.layer
    }
}

impl CaptureArgs {
    pub(crate) fn layer(&self) -> Layer {
        self.layer
    }
}

#[derive(Args)]
pub(crate) struct StatsArgs {
    /// Print the statistics this often, in seconds, each time for the time since the last print
    #[arg(long, value_name = "S", default_value = "1")]
    every: f64,

    /// Stop after this many seconds (default: until Ctrl+C)
    #[arg(long, value_name = "S")]
    duration: Option<f64>,

    /// Statistics of a capture (`ww capture -o FILE`) instead of the device, over the whole file
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub file: Option<PathBuf>,

    /// With --file: read on past damaged blocks (to the next block start) instead of stopping at the first one
    #[arg(long, requires = "file")]
    resync: bool,
}

/// `--sync` of `ww capture`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sync(SyncPolicy);

impl std::str::FromStr for Sync {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let bad = || format!("'{s}': none, blocks:N or an interval like 1s or 250ms");
        if s == "none" {
            return Ok(Sync(SyncPolicy::None));
        }
        if let Some(n) = s.strip_prefix("blocks:") {
            let n: u32 = n.parse().map_err(|_| bad())?;
            return Ok(Sync(SyncPolicy::EveryBlocks(n.max(1))));
        }
        let (num, scale) = if let Some(ms) = s.strip_suffix("ms") {
            (ms, 1e-3)
        } else if let Some(sec) = s.strip_suffix('s') {
            (sec, 1.0)
        } else {
            return Err(bad());
        };
        let v: f64 = num.parse().map_err(|_| bad())?;
        if !(v.is_finite() && v > 0.0) {
            return Err(bad());
        }
        Ok(Sync(SyncPolicy::Interval(Duration::from_secs_f64(
            v * scale,
        ))))
    }
}

fn sync_candidates() -> Vec<CompletionCandidate> {
    [
        (
            "1s",
            "at most 1 s apart (default): a crash loses at most the last second",
        ),
        ("100ms", "at most 100 ms apart"),
        ("blocks:1", "after every block, the safest and slowest"),
        (
            "none",
            "only at the end: a crash may lose everything not yet written by the OS",
        ),
    ]
    .into_iter()
    .map(|(v, help)| CompletionCandidate::new(v).help(Some(help.into())))
    .collect()
}

#[derive(Args)]
pub(crate) struct CaptureArgs {
    /// File to write; an existing file is appended to (a capture is blocks back to back)
    #[arg(short, long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    output: PathBuf,

    /// What to record: frame records frames as well, link and api only messages and link events
    #[arg(long, value_enum, default_value_t = Layer::Frame)]
    layer: Layer,

    /// Add to an existing capture instead of refusing to overwrite it (a new one is written as FILE.partial and
    /// renamed when done)
    #[arg(long)]
    append: bool,

    /// When the data is synced to the disk, bounding what a crash can lose: `none` (only at the end), `blocks:N`
    /// (every N blocks) or an interval (`1s`, `250ms`)
    #[arg(long, value_name = "POLICY", default_value = "1s", add = ArgValueCandidates::new(sync_candidates))]
    sync: Sync,

    /// Stop after this many seconds (default: until Ctrl+C)
    #[arg(long, value_name = "S")]
    duration: Option<f64>,
}

/// A live tap on the CLI's own connection: the recorder its client records into and a subscription to it, made
/// before connecting so the capture starts with the link setup.
pub(crate) struct LiveTap {
    pub recorder: Recorder,
    subscription: Subscription,
}

impl LiveTap {
    pub(crate) fn new(layer: Layer) -> Self {
        let recorder = Recorder::new(RecorderConfig {
            name: "ww".into(),
            ..RecorderConfig::default()
        });
        // ~64 KiB blocks: a few seconds of a busy link
        let subscription = recorder.subscribe(layer.layers(), 256);
        LiveTap {
            recorder,
            subscription,
        }
    }

    pub(crate) fn add_bundles(&self, device: &DynClient) {
        add_bundles(&self.recorder, device);
    }
}

/// Put the API bundles a capture needs into the describe blocks: the tap's own (its records are ordinary
/// wire_weaver types) and the device's as it sent it (the decoder puts the snapshot-known traits back), so a capture is explained without the device or a local cache.
fn add_bundles(recorder: &Recorder, device: &DynClient) {
    let tap = wire_weaver_client::ww_tap::api().make_owned();
    if let Some(bundle) = wire_weaver_client::snapshots::get(&tap)
        && let Ok(bytes) =
            wire_weaver::shrink_wrap::SerializeShrinkWrapOwned::to_ww_bytes_owned(bundle)
    {
        let hash = wire_weaver_client::ww_self::signature::api_hash(&bytes);
        recorder.add_bundle(&hash, &bytes);
    }
    if let Some(i) = device.device_introspect()
        && let Ok(bytes) = wire_weaver::shrink_wrap::SerializeShrinkWrapOwned::to_ww_bytes_owned(
            &*i.sent_api_bundle,
        )
    {
        recorder.add_bundle(&i.api_hash.no_docs.hash, &bytes);
    }
}

/// What a [Feed] gives.
pub(crate) enum Next<'a> {
    Block(Block<'a>),
    /// Nothing within the wait.
    Idle,
    /// No more blocks will come.
    End,
}

/// Where blocks come from: a live recorder, a file, later a ww server's tap (SRV-13).
pub(crate) trait Feed {
    fn next(&mut self, wait: Duration) -> Result<Next<'_>>;
}

pub(crate) struct LiveFeed {
    subscription: Subscription,
    current: Option<wire_weaver_client::tap::Chunk>,
}

impl LiveFeed {
    pub(crate) fn new(tap: LiveTap) -> (Self, Recorder) {
        (
            LiveFeed {
                subscription: tap.subscription,
                current: None,
            },
            tap.recorder,
        )
    }
}

impl Feed for LiveFeed {
    fn next(&mut self, wait: Duration) -> Result<Next<'_>> {
        use std::sync::mpsc::RecvTimeoutError;
        match self.subscription.recv_timeout(wait) {
            Ok(chunk) => {
                let chunk = self.current.insert(chunk);
                Ok(Next::Block(chunk.block()))
            }
            Err(RecvTimeoutError::Timeout) => Ok(Next::Idle),
            Err(RecvTimeoutError::Disconnected) => Ok(Next::End),
        }
    }
}

pub(crate) struct FileFeed<R> {
    blocks: FileBlocks<R>,
}

impl FileFeed<BufReader<File>> {
    /// `resync`: read on past damage instead of stopping at it ([Recovery]).
    pub(crate) fn open(path: &Path, resync: bool) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let recovery = if resync {
            Recovery::Resync
        } else {
            Recovery::Prefix
        };
        Ok(FileFeed {
            blocks: FileBlocks::with_recovery(BufReader::new(file), recovery),
        })
    }
}

impl<R: std::io::Read> Feed for FileFeed<R> {
    fn next(&mut self, _wait: Duration) -> Result<Next<'_>> {
        // where the walk was cut (or, with --resync, what was skipped) is said on stderr
        let block = self
            .blocks
            .next_good(|e| eprintln!("{} {e}", style("capture:").yellow()));
        Ok(block.map_or(Next::End, Next::Block))
    }
}

/// Whether a path is a capture: a file starting with a tape block.
pub(crate) fn is_capture(path: &str) -> bool {
    let mut magic = [0u8; MAGIC.len()];
    File::open(path)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut magic))
        .is_ok()
        && magic == MAGIC
}

/// Decoder and printer: blocks in, text out.
pub(crate) struct Monitor {
    decoder: Decoder,
    view: ViewArgs,
    t0: Option<u64>,
    /// Text goes to stderr: colour by what stderr is, not stdout
    stderr: bool,
}

impl Monitor {
    /// `local_cache`: look device APIs up in `~/.wire_weaver` when a capture doesn't carry them.
    pub(crate) fn new(view: ViewArgs, local_cache: bool) -> Self {
        Self::with_frames(view, local_cache, true)
    }

    /// `frames`: the recorder gave frames too (else messages only, and are printed as they come).
    pub(crate) fn with_frames(view: ViewArgs, local_cache: bool, frames: bool) -> Self {
        // explained at every layer: requests and stats go by their names, not ids
        let decoder = Decoder::new(DecoderConfig {
            frames,
            ..DecoderConfig::default()
        });
        Monitor {
            decoder: if local_cache {
                decoder.with_local_cache()
            } else {
                decoder
            },
            view,
            t0: None,
            stderr: false,
        }
    }

    /// Printed to stderr: colour codes only where stderr is a terminal (stdout may be the other way round).
    pub(crate) fn for_stderr(mut self) -> Self {
        self.stderr = true;
        self
    }

    pub(crate) fn block(&mut self, block: &Block<'_>, out: &mut impl Write) -> Result<()> {
        self.decoder.feed(block);
        self.drain(out)
    }

    /// Print what still waits, at the end.
    pub(crate) fn finish(&mut self, out: &mut impl Write) -> Result<()> {
        self.decoder.finish();
        self.drain(out)
    }

    fn drain(&mut self, out: &mut impl Write) -> Result<()> {
        while let Some(d) = self.decoder.next() {
            if let Some(text) = self.render(&d) {
                let text = if self.stderr && !console::colors_enabled_stderr() {
                    console::strip_ansi_codes(&text).into_owned()
                } else {
                    text
                };
                out.write_all(text.as_bytes())?;
            }
        }
        out.flush()?;
        Ok(())
    }

    pub(crate) fn stats_report(&mut self) -> String {
        self.decoder.stats.take().report()
    }

    fn render(&mut self, d: &Decoded) -> Option<String> {
        let t0 = *self.t0.get_or_insert(d.t);
        let t = d.t.saturating_sub(t0);
        let head = format!(
            "{} {}",
            style(format!("{:>11.6}", t as f64 / 1e9)).dim(),
            style(d.link.map_or("  ".into(), |l| format!("L{l}"))).dim()
        );
        let mut s = String::new();
        match &d.what {
            What::Session(session) => {
                _ = writeln!(s, "{head} {}", style(session_line(session)).cyan());
            }
            What::LinkEvent(e) => {
                let text = link_event_line(e);
                let text = match e {
                    LinkEventOwned::Error { .. } | LinkEventOwned::Lost { .. } => {
                        style(text).red().to_string()
                    }
                    _ => style(text).cyan().to_string(),
                };
                _ = writeln!(s, "{head} {text}");
            }
            What::BlocksLost { blocks } => {
                _ = writeln!(
                    s,
                    "{head} {}",
                    style(format!("{blocks} blocks lost (the reader fell behind)")).red()
                );
            }
            What::Frame(f) => {
                if self.view.layer != Layer::Frame {
                    return None;
                }
                _ = writeln!(s, "{head} {}", frame_line(f));
                if self.view.bytes {
                    hex_lines(&mut s, &f.bytes);
                }
            }
            What::Message(m) => {
                let link = d.link.unwrap_or(0);
                if self.view.layer == Layer::Api {
                    let api = m.api.as_ref()?;
                    // the API of the message's own resource (a global trait's), else the device's
                    let bundle = api
                        .envelope
                        .as_ref()
                        .and_then(|e| e.resource.as_ref())
                        .or_else(|| self.decoder.link_api(link))
                        .map(|r| r.bundle().clone());
                    _ = writeln!(s, "{head} {}", api_line(m, api, bundle.as_deref()));
                    if self.view.bits
                        && let (Some(e), Some(bundle)) = (&api.envelope, &bundle)
                    {
                        s += &explain::render(&e.title, &m.bytes, &e.explained, bundle);
                        s += "\n";
                    }
                } else {
                    _ = writeln!(s, "{head} {}", message_line(m));
                }
                if self.view.bytes {
                    hex_lines(&mut s, &m.bytes);
                }
            }
        }
        Some(s)
    }
}

fn arrow(dir: Dir) -> console::StyledObject<&'static str> {
    match dir {
        Dir::Tx => style("→").green(),
        Dir::Rx => style("←").blue(),
    }
}

fn session_line(s: &SessionOwned) -> String {
    let framer = match &s.framer {
        wire_weaver_client::ww_tap::Framer::None => "no framer".to_string(),
        other => format!("{other:?}"),
    };
    let mut text = format!("session {:?} {:?} '{}', {framer}", s.side, s.medium, s.peer);
    if s.frame_size.0 > 0 {
        _ = write!(text, ", frames up to {} B", s.frame_size.0);
    }
    if !s.device_info.is_empty() {
        text += ", link set up";
    }
    text
}

fn link_event_line(e: &LinkEventOwned) -> String {
    match e {
        LinkEventOwned::Connected => "connected".into(),
        LinkEventOwned::LinkReady => "link ready".into(),
        LinkEventOwned::Disconnected {
            reason,
            keep_streams,
        } => format!(
            "disconnected: {reason}{}",
            if *keep_streams { " (streams kept)" } else { "" }
        ),
        LinkEventOwned::Error { what } => format!("error: {what}"),
        LinkEventOwned::Lost { records } => {
            format!("{} records lost (no free block in the recorder)", records.0)
        }
        other => format!("{other:?} (a newer ww_tap's)"),
    }
}

fn frame_line(f: &FrameInfo) -> String {
    let mut s = format!(
        "{} {} {} B",
        arrow(f.dir),
        style(format!("frame #{}", f.no)).bold(),
        f.bytes.len()
    );
    if f.gap > 0 {
        _ = write!(
            s,
            " {}",
            style(format!("({} frames missing before)", f.gap)).red()
        );
    }
    if let Some(report) = f.outcome.prefix_report {
        let prefix: Vec<String> = f.bytes[f.outcome.prefix.clone()]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        _ = write!(s, " [{} {}]", prefix.join(" "), prefix_text(report));
    }
    let pieces = pieces_text(&f.outcome);
    if !pieces.is_empty() {
        _ = write!(s, ": {pieces}");
    }
    s
}

/// What a frame's prefix check found, coloured: red when the frame is dropped or frames were lost.
fn prefix_text(report: FrameReport) -> String {
    let text = match report {
        FrameReport::Unchecked | FrameReport::InOrder => return "in order".into(),
        FrameReport::Synced(at) => return format!("synced at seq {} epoch {}", at.seq, at.epoch),
        FrameReport::NewEpoch(at) => {
            return format!("new session: seq {} epoch {}", at.seq, at.epoch);
        }
        FrameReport::Gap { lost } => format!("{lost} frames lost before"),
        FrameReport::Resynced(at) => {
            format!("dropped, resynced at seq {} epoch {}", at.seq, at.epoch)
        }
        FrameReport::Duplicate => "dropped: duplicate".into(),
        FrameReport::Late => "dropped: late".into(),
        FrameReport::StaleEpoch => "dropped: previous session".into(),
        FrameReport::TooShort => "dropped: shorter than the prefix".into(),
    };
    style(text).red().to_string()
}

fn pieces_text(o: &FrameOutcome) -> String {
    let mut parts: Vec<String> = o
        .pieces
        .iter()
        .map(|p| {
            let here = p.payload.len();
            let size = if here == p.len {
                format!("{here} B")
            } else {
                format!("{here}/{} B", p.len)
            };
            let text = format!("{:?} {} {size}", p.kind, kind_name(p.user_kind));
            match &p.outcome {
                PieceOutcome::Message | PieceOutcome::Part => text,
                PieceOutcome::Dropped(why) => {
                    style(format!("{text} dropped: {why}")).red().to_string()
                }
            }
        })
        .collect();
    if let Some((at, why)) = &o.error {
        parts.push(style(format!("@{at}: {why}")).red().to_string());
    }
    if let Some(n) = o.dropped_partial {
        parts.push(
            style(format!("a split message dropped after {n} B"))
                .red()
                .to_string(),
        );
    }
    parts.join(", ")
}

fn message_line(m: &MessageInfo) -> String {
    let mut s = format!(
        "{} {} {} B",
        arrow(m.dir),
        style(m.link_kind).bold(),
        m.bytes.len()
    );
    if let Some(no) = m.no {
        _ = write!(s, " msg #{no}");
    }
    if !m.frames.is_empty() {
        _ = write!(s, " {}", style(frames_text(m)).dim());
    }
    match &m.check {
        Check::Match | Check::NoFrames => {}
        Check::Mismatch {
            framer_kind,
            framer_bytes,
        } => {
            _ = write!(
                s,
                " {}",
                style(format!(
                    "MISMATCH: the framer gave {} {} B",
                    kind_name(*framer_kind),
                    framer_bytes.len()
                ))
                .red()
            );
        }
        Check::NoRecord => _ = write!(s, " {}", style("(framer only)").yellow()),
    }
    if let Some(api) = &m.api {
        _ = write!(s, " {}", api.title);
        if api.seq != 0 {
            _ = write!(s, " {}", style(format!("seq {}", api.seq)).dim());
        }
        if let Some((no, dt)) = api.reply_to {
            _ = write!(
                s,
                " {}",
                style(format!("reply to #{no} in {}", ns(dt))).dim()
            );
        }
    }
    s
}

/// `in #12@2+10`, `in #12@58+6, #13@2+62, #14@2+40`, or `in #12..#20 (9 frames)`.
fn frames_text(m: &MessageInfo) -> String {
    let f = &m.frames;
    if f.len() > 3 {
        return format!(
            "in #{}..#{} ({} frames)",
            f[0].frame_no,
            f[f.len() - 1].frame_no,
            f.len()
        );
    }
    let parts: Vec<String> = f
        .iter()
        .map(|p| format!("#{}@{}+{}", p.frame_no, p.offset, p.len))
        .collect();
    format!("in {}", parts.join(", "))
}

fn api_line(
    m: &MessageInfo,
    api: &wire_weaver_client::tap::ApiInfo,
    bundle: Option<&ApiBundleOwned>,
) -> String {
    let mut s = format!("{} {}", arrow(m.dir), style(&api.title).bold());
    if let (Some(e), Some(bundle)) = (&api.envelope, bundle) {
        let fields = payload_text(e, bundle);
        if !fields.is_empty() {
            _ = write!(s, "  {fields}");
        }
    }
    if let Some((no, dt)) = api.reply_to {
        _ = write!(
            s,
            " {}",
            style(format!("(reply to #{no} in {})", ns(dt))).dim()
        );
    } else if api.seq != 0
        && m.dir == Dir::Tx
        && let Some(no) = m.no
    {
        _ = write!(s, " {}", style(format!("#{no}")).dim());
    }
    s
}

/// The payload's values: `a: 1, b: 2` for arguments or the properties of an update, the value alone otherwise, a
/// text where there is no value (what an `Observe` asks or was granted).
fn payload_text(e: &wire_weaver_client::envelope::Envelope, bundle: &ApiBundleOwned) -> String {
    let parts = &e.explained.parts;
    let Some(at) = parts.iter().position(|p| {
        p.kind == PartKind::Value
            && match p.label.as_str() {
                "args" | "data" => p.ty == "TailBytes",
                "cfg" | "granted" => true,
                _ => false,
            }
    }) else {
        return String::new();
    };
    let payload = &parts[at];
    // not explained (an unknown resource): the bytes
    if let Some(v) = &payload.value {
        return fmt_value(v, None, bundle);
    }
    if let Some(text) = &payload.note {
        return text.clone();
    }
    let values = match &e.explained.result {
        Ok(v) => v,
        Err(err) => return style(format!("({err:#})")).red().to_string(),
    };
    let names: Vec<&str> = parts[at + 1..]
        .iter()
        .take_while(|p| p.depth > payload.depth)
        .filter(|p| p.depth == payload.depth + 1)
        .map(|p| p.label.as_str())
        .collect();
    values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let value = fmt_value(v, e.types.get(i), bundle);
            match names.get(i) {
                Some(name) if !name.is_empty() => format!("{name}: {value}"),
                _ => value,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn hex_lines(s: &mut String, bytes: &[u8]) {
    for (i, chunk) in bytes.chunks(32).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        _ = writeln!(
            s,
            "{:>15} {}",
            style(format!("{:04x}", i * 32)).dim(),
            style(hex.join(" ")).dim()
        );
    }
}

/// Run `on_block` on every block from `feed` until `stop` is set and the feed is idle (or it ends).
fn consume(
    feed: &mut dyn Feed,
    stop: &AtomicBool,
    mut on_block: impl FnMut(&Block<'_>) -> Result<()>,
) -> Result<()> {
    consume_idle(feed, stop, |b| match b {
        Some(b) => on_block(b),
        None => Ok(()),
    })
}

/// [consume], with `on(None)` when the feed was idle for a while.
fn consume_idle(
    feed: &mut dyn Feed,
    stop: &AtomicBool,
    mut on: impl FnMut(Option<&Block<'_>>) -> Result<()>,
) -> Result<()> {
    loop {
        match feed.next(Duration::from_millis(100))? {
            Next::Block(b) => on(Some(&b))?,
            Next::Idle if stop.load(Ordering::Relaxed) => return Ok(()),
            Next::Idle => on(None)?,
            Next::End => return Ok(()),
        }
    }
}

/// Wait for Ctrl+C or the duration, disconnect so the capture ends with it, then let the consumer drain and stop.
async fn run_live<T: Send + 'static>(
    device: DynClient,
    recorder: Recorder,
    duration: Option<f64>,
    mut stopped: StopSignal,
    consumer: impl FnOnce(Arc<AtomicBool>) -> Result<T> + Send + 'static,
) -> Result<T> {
    let stop = Arc::new(AtomicBool::new(false));
    let thread = tokio::task::spawn_blocking({
        let stop = stop.clone();
        move || consumer(stop)
    });
    let wait = async {
        match duration {
            Some(s) => tokio::time::sleep(Duration::from_secs_f64(s)).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = stopped.wait() => {}
        _ = wait => {}
    }
    _ = device.disconnect().asynch().await;
    recorder.flush();
    stop.store(true, Ordering::Relaxed);
    thread.await?
}

pub(crate) async fn monitor(
    args: MonitorArgs,
    device: DynClient,
    tap: LiveTap,
    stop: StopSignal,
) -> Result<()> {
    tap.add_bundles(&device);
    let (mut feed, recorder) = LiveFeed::new(tap);
    let view = args.view;
    run_live(device, recorder, args.duration, stop, move |stop| {
        let mut monitor = Monitor::with_frames(view, false, view.layer == Layer::Frame);
        let mut out = std::io::stdout().lock();
        consume(&mut feed, &stop, |b| monitor.block(b, &mut out))?;
        monitor.finish(&mut out)
    })
    .await
}

pub(crate) async fn stats(
    args: StatsArgs,
    device: DynClient,
    tap: LiveTap,
    stop: StopSignal,
) -> Result<()> {
    tap.add_bundles(&device);
    let (mut feed, recorder) = LiveFeed::new(tap);
    let every = Duration::from_secs_f64(args.every.max(0.01));
    run_live(device, recorder, args.duration, stop, move |stop| {
        let view = ViewArgs {
            layer: Layer::Frame,
            bytes: false,
            bits: false,
        };
        let mut monitor = Monitor::new(view, false);
        let mut last = Instant::now();
        let print = |monitor: &mut Monitor, last: &mut Instant| {
            if last.elapsed() >= every {
                *last = Instant::now();
                println!("{}", monitor.stats_report());
            }
        };
        let mut sink = std::io::sink();
        consume(&mut feed, &stop, |b| {
            monitor.block(b, &mut sink)?;
            print(&mut monitor, &mut last);
            Ok(())
        })?;
        monitor.finish(&mut sink)?;
        println!("{}", monitor.stats_report());
        Ok(())
    })
    .await
}

/// `ww stats --file FILE`: the statistics of a whole capture.
pub(crate) fn stats_file(args: &StatsArgs) -> Result<()> {
    let path = args.file.as_deref().expect("--file");
    let view = ViewArgs {
        layer: Layer::Frame,
        bytes: false,
        bits: false,
    };
    let mut monitor = Monitor::new(view, true);
    let mut feed = FileFeed::open(path, args.resync)?;
    let mut sink = std::io::sink();
    let stop = AtomicBool::new(true);
    consume(&mut feed, &stop, |b| monitor.block(b, &mut sink))?;
    monitor.finish(&mut sink)?;
    print!("{}", monitor.stats_report());
    Ok(())
}

pub(crate) async fn capture(
    args: CaptureArgs,
    device: DynClient,
    tap: LiveTap,
    stop: StopSignal,
) -> Result<()> {
    tap.add_bundles(&device);
    let policy = args.sync.0;
    let writer = if args.append {
        TapeWriter::append(&args.output, policy)
    } else {
        TapeWriter::create(&args.output, policy)
    }
    .map_err(|e| match e {
        WriteError::Exists(p) => anyhow!(
            "{} exists: --append to add to it, or another name",
            p.display()
        ),
        e => anyhow!("{}: {e}", args.output.display()),
    })?;
    let (mut feed, recorder) = LiveFeed::new(tap);
    let (path, blocks, bytes, lost) =
        run_live(device, recorder, args.duration, stop, move |stop| {
            let mut writer = writer;
            let shown = writer.current_path().display().to_string();
            let bar = ProgressBar::new_spinner().with_style(
                ProgressStyle::with_template("{spinner} {msg} ({elapsed})").expect("template"),
            );
            bar.enable_steady_tick(Duration::from_millis(200));
            let mut lost = 0u64;
            let mut next_seq = None;
            // a failed write or sync ends the capture: the file stays as it is (its valid prefix reads), never retried
            let result = consume_idle(&mut feed, &stop, |b| {
                let Some(b) = b else {
                    return Ok(writer.tick()?);
                };
                writer.write_block(b.bytes())?;
                if let Some(n) = next_seq
                    && b.seq > n
                {
                    lost += b.seq - n;
                }
                next_seq = Some(b.seq + 1);
                bar.set_message(format!(
                    "capturing to {shown}: {} blocks, {}",
                    writer.blocks(),
                    writer.bytes().human_count_bytes()
                ));
                Ok(())
            });
            bar.finish_and_clear();
            let (blocks, bytes) = (writer.blocks(), writer.bytes());
            let current = writer.current_path().to_path_buf();
            result
                .with_context(|| format!("capture stopped, {} kept as it is", current.display()))?;
            let path = writer.finish()?;
            Ok((path, blocks, bytes, lost))
        })
        .await?;
    println!(
        "{} {blocks} blocks, {} to {}{}",
        style("captured").green(),
        bytes.human_count_bytes(),
        path.display(),
        if lost > 0 {
            style(format!(", {lost} blocks lost (the writer fell behind)"))
                .red()
                .to_string()
        } else {
            String::new()
        }
    );
    Ok(())
}

/// `ww explain FILE`: a capture printed as `ww monitor` printed it live.
pub(crate) fn explain_file(path: &Path, view: ViewArgs, resync: bool) -> Result<()> {
    let mut feed = FileFeed::open(path, resync)?;
    let mut out = std::io::stdout().lock();
    replay(&mut feed, view, true, &mut out)
}

/// A feed to its end through a [Monitor].
pub(crate) fn replay(
    feed: &mut dyn Feed,
    view: ViewArgs,
    local_cache: bool,
    out: &mut impl Write,
) -> Result<()> {
    let mut monitor = Monitor::new(view, local_cache);
    let stop = AtomicBool::new(true);
    consume(feed, &stop, |b| monitor.block(b, out))?;
    monitor.finish(out)
}

#[cfg(test)]
mod tests;
