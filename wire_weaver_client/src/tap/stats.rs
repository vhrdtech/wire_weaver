//! Framer and link statistics, from what the decoder sees (TR-14): everything the bytes show, per link and
//! direction, over any window ([Stats::take] starts a new one).

use std::collections::BTreeMap;
use std::fmt::Write;

use ww_tap::Dir;

use ww_framer::prefix::FrameReport;

use super::reassembly::{FrameOutcome, PieceOutcome};

/// What frame prefix checks found (a `SeqPrefix` medium: UDP, USB with the diagnostic), as `FramedRx` counts it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrefixCounts {
    /// Frames checked, including the dropped ones.
    pub checked: u64,
    /// Frames missing, summed over every gap.
    pub lost: u64,
    /// Dropped: received before, from before the expected one (reordered), from the previous session, too short
    /// for a prefix, or taken as the new reference after a long loss.
    pub duplicate: u64,
    pub late: u64,
    pub stale_epoch: u64,
    pub too_short: u64,
    pub resynced: u64,
    /// The sender started a new session (link setup).
    pub new_epoch: u64,
}

impl PrefixCounts {
    fn count(&mut self, report: FrameReport) {
        self.checked += 1;
        match report {
            FrameReport::Gap { lost } => self.lost += lost as u64,
            FrameReport::Duplicate => self.duplicate += 1,
            FrameReport::Late => self.late += 1,
            FrameReport::StaleEpoch => self.stale_epoch += 1,
            FrameReport::TooShort => self.too_short += 1,
            FrameReport::Resynced(_) => self.resynced += 1,
            FrameReport::NewEpoch(_) => self.new_epoch += 1,
            FrameReport::Unchecked | FrameReport::Synced(_) | FrameReport::InOrder => {}
        }
    }

    /// Frames dropped by their prefix.
    pub fn dropped(&self) -> u64 {
        self.duplicate + self.late + self.stale_epoch + self.too_short + self.resynced
    }
}

/// One direction of a link.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DirStats {
    pub frames: u64,
    pub frame_bytes: u64,
    /// Sum of the session's frame size over the frames: `frame_bytes / frame_capacity` is the fill ratio.
    pub frame_capacity: u64,
    /// Bytes of frame prefixes, of heads, of checksums and tails, of message payload in frames.
    pub prefix_bytes: u64,
    pub head_bytes: u64,
    pub trailer_bytes: u64,
    pub payload_bytes: u64,
    /// Messages the framer gave back.
    pub messages: u64,
    pub message_bytes: u64,
    /// Of those, split over more than one frame, and the frames they took in total.
    pub split_messages: u64,
    pub split_frames: u64,
    /// Pieces dropped (a checksum mismatch, a lost Start or End), split messages dropped half way.
    pub dropped_pieces: u64,
    pub checksum_failures: u64,
    pub partial_dropped: u64,
    /// Frames whose rest could not be walked (a bad head, a piece past the end).
    pub bad_frames: u64,
    /// Frames missing by their numbers (the tap lost them, or they were recorded before a consumer joined).
    pub frame_gaps: u64,
    /// What the frame prefixes say: frames the medium lost, duplicated or reordered (none without a prefix).
    pub prefix: PrefixCounts,
    /// Message records whose bytes differ from what the framer gave back: a framer bug.
    pub mismatches: u64,
    /// Messages recorded by the transport, by `ww_link` kind name.
    pub kinds: BTreeMap<&'static str, u64>,
    /// From a message being written to the frame that carried its end going out (tx), ns.
    pub latency_sum_ns: u64,
    pub latency_max_ns: u64,
    pub latency_count: u64,
}

impl DirStats {
    pub(crate) fn frame(&mut self, len: usize, frame_size: usize, outcome: &FrameOutcome) {
        self.frames += 1;
        self.frame_bytes += len as u64;
        self.frame_capacity += frame_size.max(len) as u64;
        if outcome.error.is_some() {
            self.bad_frames += 1;
        }
        if outcome.dropped_partial.is_some() {
            self.partial_dropped += 1;
        }
        if let Some(report) = outcome.prefix_report {
            self.prefix.count(report);
            self.prefix_bytes += outcome.prefix.len() as u64;
        }
        for p in &outcome.pieces {
            self.head_bytes += p.head.len() as u64;
            self.trailer_bytes += p.trailer.len() as u64;
            self.payload_bytes += p.payload.len() as u64;
            if let PieceOutcome::Dropped(why) = p.outcome {
                self.dropped_pieces += 1;
                if why.starts_with("checksum") {
                    self.checksum_failures += 1;
                }
            }
        }
        for m in &outcome.completed {
            self.messages += 1;
            self.message_bytes += m.bytes.len() as u64;
            if m.parts.len() > 1 {
                self.split_messages += 1;
                self.split_frames += m.parts.len() as u64;
            }
        }
    }

    pub(crate) fn latency(&mut self, ns: u64) {
        self.latency_sum_ns += ns;
        self.latency_max_ns = self.latency_max_ns.max(ns);
        self.latency_count += 1;
    }

    pub fn fill_ratio(&self) -> Option<f64> {
        (self.frame_capacity > 0).then(|| self.frame_bytes as f64 / self.frame_capacity as f64)
    }

    pub fn messages_per_frame(&self) -> Option<f64> {
        (self.frames > 0).then(|| self.messages as f64 / self.frames as f64)
    }
}

/// Per request path (or event title): count and reply time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathStats {
    pub count: u64,
    pub replies: u64,
    pub reply_sum_ns: u64,
    pub reply_max_ns: u64,
}

/// One link.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinkStats {
    pub tx: DirStats,
    pub rx: DirStats,
    /// Requests by what they ask for (`Call motor.set_speed`), with reply times.
    pub requests: BTreeMap<String, PathStats>,
    /// Events without a request (stream data, property updates) by what they are.
    pub events: BTreeMap<String, u64>,
    /// Requests whose reply never came within the window.
    pub unanswered: u64,
    pub link_events: u64,
}

impl LinkStats {
    pub fn dir(&mut self, dir: Dir) -> &mut DirStats {
        match dir {
            Dir::Tx => &mut self.tx,
            Dir::Rx => &mut self.rx,
        }
    }
}

/// Everything since the last [Stats::take].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub links: BTreeMap<u32, LinkStats>,
    pub blocks: u64,
    /// Blocks missing by their seq.
    pub blocks_lost: u64,
    /// Records the producer reported lost ([ww_tap::LinkEvent::Lost]).
    pub records_lost: u64,
    /// Producer time the window covers, ns.
    pub from_ns: Option<u64>,
    pub to_ns: u64,
}

impl Stats {
    pub(crate) fn link(&mut self, link: u32) -> &mut LinkStats {
        self.links.entry(link).or_default()
    }

    pub(crate) fn at(&mut self, t: u64) {
        self.from_ns.get_or_insert(t);
        self.to_ns = self.to_ns.max(t);
    }

    /// This window, and start a new one.
    pub fn take(&mut self) -> Stats {
        std::mem::take(self)
    }

    /// A text report, one block per link and direction.
    pub fn report(&self) -> String {
        let mut s = String::new();
        let secs = match self.from_ns {
            Some(from) if self.to_ns > from => (self.to_ns - from) as f64 / 1e9,
            _ => 0.0,
        };
        let rate = |n: u64| {
            if secs > 0.0 {
                format!(" ({:.1}/s)", n as f64 / secs)
            } else {
                String::new()
            }
        };
        _ = writeln!(
            s,
            "window {secs:.3} s, {} blocks, {} blocks lost, {} records lost",
            self.blocks, self.blocks_lost, self.records_lost
        );
        for (link, l) in &self.links {
            for (name, d) in [("tx", &l.tx), ("rx", &l.rx)] {
                if d.frames == 0 && d.kinds.is_empty() {
                    continue;
                }
                _ = writeln!(s, "link {link} {name}:");
                if d.frames > 0 {
                    _ = writeln!(
                        s,
                        "  frames {}{}, {} bytes, fill {:.1}%, {:.2} messages/frame",
                        d.frames,
                        rate(d.frames),
                        d.frame_bytes,
                        d.fill_ratio().unwrap_or(0.0) * 100.0,
                        d.messages_per_frame().unwrap_or(0.0)
                    );
                    _ = writeln!(
                        s,
                        "  overhead: prefixes {} B, heads {} B, checksums and tails {} B, unused {} B",
                        d.prefix_bytes,
                        d.head_bytes,
                        d.trailer_bytes,
                        d.frame_capacity.saturating_sub(d.frame_bytes)
                    );
                    _ = writeln!(
                        s,
                        "  messages {}, {} split over {} frames",
                        d.messages, d.split_messages, d.split_frames
                    );
                    if d.dropped_pieces + d.bad_frames + d.frame_gaps + d.mismatches > 0 {
                        _ = writeln!(
                            s,
                            "  trouble: {} pieces dropped ({} checksum), {} partial messages dropped, {} bad frames, {} frame gaps, {} framer mismatches",
                            d.dropped_pieces,
                            d.checksum_failures,
                            d.partial_dropped,
                            d.bad_frames,
                            d.frame_gaps,
                            d.mismatches
                        );
                    }
                    let p = &d.prefix;
                    if p.checked > 0 {
                        _ = writeln!(
                            s,
                            "  frame prefix: {} lost, {} dropped ({} duplicate, {} late, {} previous session, {} too short, {} resynced), {} new sessions",
                            p.lost,
                            p.dropped(),
                            p.duplicate,
                            p.late,
                            p.stale_epoch,
                            p.too_short,
                            p.resynced,
                            p.new_epoch
                        );
                    }
                    if let Some(mean) = d.latency_sum_ns.checked_div(d.latency_count) {
                        _ = writeln!(
                            s,
                            "  message to frame: mean {}, max {}",
                            ns(mean),
                            ns(d.latency_max_ns)
                        );
                    }
                }
                if !d.kinds.is_empty() {
                    let kinds: Vec<String> =
                        d.kinds.iter().map(|(k, n)| format!("{k} {n}")).collect();
                    _ = writeln!(s, "  link messages: {}", kinds.join(", "));
                }
            }
            if !l.requests.is_empty() || !l.events.is_empty() {
                _ = writeln!(s, "link {link} api:");
            }
            for (what, p) in &l.requests {
                let reply = match p.reply_sum_ns.checked_div(p.replies) {
                    Some(mean) => format!(", reply mean {} max {}", ns(mean), ns(p.reply_max_ns)),
                    None => String::new(),
                };
                _ = writeln!(s, "  {what}: {}{}{reply}", p.count, rate(p.count));
            }
            for (what, n) in &l.events {
                _ = writeln!(s, "  {what}: {n}{}", rate(*n));
            }
        }
        s
    }
}

/// ns as a short human duration.
pub fn ns(ns: u64) -> String {
    if ns >= 1_000_000_000 {
        format!("{:.2} s", ns as f64 / 1e9)
    } else if ns >= 1_000_000 {
        format!("{:.2} ms", ns as f64 / 1e6)
    } else if ns >= 1_000 {
        format!("{:.1} µs", ns as f64 / 1e3)
    } else {
        format!("{ns} ns")
    }
}
