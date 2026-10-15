//! Link loopback and speed test ([Command::LoopbackTest](crate::event_loop::command::Command::LoopbackTest)),
//! run by [TxCore](super::TxCore) while the link is up. Sans-IO like the cores: driven by `now`, outputs
//! [TestOut] for tx to send, and is told about every `Loopback` message rx got through [LoopbackTest::on_echo].
//!
//! Three stages, each runs for the test duration:
//! 1. `loopback`: one message with `repeat = 1` at a time, the device echoes it back; counts round trips,
//!    lost (no echo within [LoopbackParams::echo_timeout]) and corrupted echoes.
//! 2. `tx_speed`: host → device with `repeat = 0` (device discards). Every [LoopbackParams::marker_every] messages,
//!    an empty `repeat = 1` marker is echoed back, at most [LoopbackParams::markers_in_flight] are in flight: a
//!    device that falls behind slows the host down instead of overflowing.
//! 3. `rx_speed`: device → host, requests of `repeat = `[LoopbackParams::rx_batch] each, at most
//!    [LoopbackParams::batches_in_flight] in flight.
//!
//! Gives up after [LoopbackParams::timeouts_in_a_row] echo timeouts in a row.
//!
//! Each message carries `data` that checks itself ([fill] / [is_intact]): the first 4 bytes are a key, the rest
//! a pseudo-random sequence derived from it. Rx checks it without knowing what tx sent, and the replies of one
//! `repeat = N` request (all with the same data) are checked the same way.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::event_loop::command::{LoopbackParams, TestProgress};

/// `repeat` and `seq` of a `Loopback` link message, before `data`.
pub(crate) const LOOPBACK_HEADER_LEN: usize = 8;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);
/// Messages emitted per call at most, so that the wrapper gets to handle other inputs in between.
const BURST: u32 = 16;
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Stage {
    RoundTrip,
    TxSpeed,
    RxSpeed,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Stage::RoundTrip => "loopback",
            Stage::TxSpeed => "tx_speed",
            Stage::RxSpeed => "rx_speed",
        }
    }
}

/// What [LoopbackTest] wants tx to do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TestOut {
    /// Send a `Loopback` link message.
    Send {
        repeat: u32,
        seq: u32,
        data: Vec<u8>,
    },
    /// Send the current frame now.
    Flush,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Running,
    /// All stages done or failed, the test is over and progress receiver is told so.
    Finished,
}

#[derive(Default, Debug)]
struct Counts {
    sent: u64,
    received: u64,
    lost: u64,
    corrupted: u64,
}

pub(crate) struct LoopbackTest {
    progress_tx: mpsc::UnboundedSender<TestProgress>,
    duration: Duration,
    p: LoopbackParams,
    /// `data` length of every message, but `tx_speed` markers that are empty
    data_len: usize,

    stage: Stage,
    started_at: Instant,
    ends_at: Instant,
    /// Stage time is over, waiting for what is still in flight before the next one starts
    draining: bool,
    next_progress_at: Instant,
    counts: Counts,

    next_seq: u32,
    /// `loopback`: seq of the echo waited for. `tx_speed`: seqs of markers in flight.
    awaiting: VecDeque<u32>,
    /// `tx_speed`: messages sent since the last marker
    since_marker: u32,
    /// `rx_speed`: replies still expected
    rx_outstanding: u64,
    /// `rx_speed`: seq of the next reply, replies of a batch are numbered from 0
    rx_expected_seq: u32,
    last_answer_at: Instant,
    timeouts_in_a_row: u32,
    /// Latest `now` seen
    last_now: Instant,

    out: VecDeque<TestOut>,
}

impl LoopbackTest {
    /// `message_len` is the size of each link message (header and data), at least [LOOPBACK_HEADER_LEN].
    pub fn new(
        now: Instant,
        duration: Duration,
        message_len: usize,
        params: LoopbackParams,
        progress_tx: mpsc::UnboundedSender<TestProgress>,
    ) -> Self {
        let mut test = LoopbackTest {
            progress_tx,
            duration,
            p: params.normalized(),
            data_len: message_len.saturating_sub(LOOPBACK_HEADER_LEN),
            stage: Stage::RoundTrip,
            started_at: now,
            ends_at: now,
            draining: false,
            next_progress_at: now,
            counts: Counts::default(),
            next_seq: 0,
            awaiting: VecDeque::new(),
            since_marker: 0,
            rx_outstanding: 0,
            rx_expected_seq: 0,
            last_answer_at: now,
            timeouts_in_a_row: 0,
            last_now: now,
            out: VecDeque::new(),
        };
        test.start_stage(now, Stage::RoundTrip);
        test
    }

    pub fn poll_output(&mut self) -> Option<TestOut> {
        self.out.pop_front()
    }

    /// Earliest instant at which [Self::on_timer] should be called, one already passed if there is something to
    /// do right away.
    pub fn poll_timeout(&self) -> Instant {
        let now = self.last_now;
        if self.can_send(now) || (self.draining && !self.is_waiting()) {
            return now;
        }
        let mut at = self.next_progress_at;
        if !self.draining {
            at = at.min(self.ends_at);
        }
        if self.is_waiting() {
            at = at.min(self.last_answer_at + self.p.echo_timeout);
        }
        at
    }

    pub fn on_timer(&mut self, now: Instant) -> Status {
        self.last_now = now;
        if self.is_waiting() && now >= self.last_answer_at + self.p.echo_timeout {
            self.on_reply_timeout(now);
            if self.timeouts_in_a_row >= self.p.timeouts_in_a_row {
                return self.fail(format!(
                    "{}: no loopback answer from the device for {} x {:?}, \
                    does it support loopback, does its scratch buffer fit {} bytes?",
                    self.stage.name(),
                    self.p.timeouts_in_a_row,
                    self.p.echo_timeout,
                    LOOPBACK_HEADER_LEN + self.data_len
                ));
            }
        }
        if !self.draining && now >= self.ends_at {
            self.finish_stage(now);
        }
        if self.draining && !self.is_waiting() {
            match self.stage {
                Stage::RoundTrip => self.start_stage(now, Stage::TxSpeed),
                Stage::TxSpeed => self.start_stage(now, Stage::RxSpeed),
                Stage::RxSpeed => return Status::Finished,
            }
        }
        if now >= self.next_progress_at {
            self.next_progress_at = now + PROGRESS_INTERVAL;
            if !self.draining {
                let done = now.saturating_duration_since(self.started_at).as_secs_f32()
                    / self.duration.as_secs_f32().max(f32::EPSILON);
                self.progress(TestProgress::Completion(self.stage.name(), done.min(1.0)));
            }
        }
        self.pump(now);
        Status::Running
    }

    /// A `Loopback` message from the device, `intact` is [is_intact] of its data.
    pub fn on_echo(&mut self, now: Instant, seq: u32, len: usize, intact: bool) {
        self.last_now = now;
        let counting = !self.draining;
        match self.stage {
            Stage::RoundTrip => {
                if self.awaiting.front() != Some(&seq) {
                    debug!("stale loopback echo {seq}, ignoring");
                    return;
                }
                self.awaiting.clear();
                self.answered(now);
                if counting {
                    self.counts.received += 1;
                    if !intact || len != self.data_len {
                        self.counts.corrupted += 1;
                    }
                }
            }
            Stage::TxSpeed => {
                let Some(pos) = self.awaiting.iter().position(|s| *s == seq) else {
                    debug!("stale tx_speed marker {seq}, ignoring");
                    return;
                };
                // markers come back in order, the ones before were lost
                self.awaiting.drain(..=pos);
                self.answered(now);
            }
            Stage::RxSpeed => {
                if self.rx_outstanding == 0 {
                    debug!("unexpected rx_speed reply {seq}, ignoring");
                    return;
                }
                let batch = self.p.rx_batch;
                let gap =
                    (seq.wrapping_add(batch).wrapping_sub(self.rx_expected_seq) % batch) as u64;
                let gap = gap.min(self.rx_outstanding - 1);
                self.rx_outstanding -= gap + 1;
                self.rx_expected_seq = seq.wrapping_add(1) % batch;
                self.answered(now);
                if counting {
                    self.counts.received += 1;
                    self.counts.lost += gap;
                    if !intact || len != self.data_len {
                        self.counts.corrupted += 1;
                    }
                }
            }
        }
        self.pump(now);
    }

    /// Link went down or tx exits.
    pub fn abort(self, reason: &str) {
        _ = self.progress_tx.send(TestProgress::FatalError(format!(
            "{}: {reason}",
            self.stage.name()
        )));
    }

    // Helpers

    fn progress(&self, p: TestProgress) {
        _ = self.progress_tx.send(p);
    }

    fn fail(&mut self, error: String) -> Status {
        warn!("loopback test failed: {error}");
        self.progress(TestProgress::FatalError(error));
        Status::Finished
    }

    fn start_stage(&mut self, now: Instant, stage: Stage) {
        self.stage = stage;
        self.started_at = now;
        self.ends_at = now + self.duration;
        self.draining = false;
        self.next_progress_at = now + PROGRESS_INTERVAL;
        self.counts = Counts::default();
        self.awaiting.clear();
        self.since_marker = 0;
        self.rx_outstanding = 0;
        self.rx_expected_seq = 0;
        self.last_answer_at = now;
        self.timeouts_in_a_row = 0;
        self.progress(TestProgress::TestStarted(stage.name()));
        self.pump(now);
    }

    /// Stage time is over: report, then drain what is still in flight.
    fn finish_stage(&mut self, now: Instant) {
        self.draining = true;
        let elapsed = now
            .saturating_duration_since(self.started_at)
            .max(self.duration)
            .as_secs_f32();
        let c = &self.counts;
        let report = match self.stage {
            Stage::RoundTrip => TestProgress::LoopbackReport {
                tx_count: c.sent,
                per_s: c.sent as f32 / elapsed,
                lost_count: c.lost,
                data_corrupted_count: c.corrupted,
            },
            Stage::TxSpeed | Stage::RxSpeed => {
                let count = match self.stage {
                    Stage::TxSpeed => c.sent,
                    _ => c.received,
                };
                let per_s = count as f32 / elapsed;
                TestProgress::SpeedReport {
                    name: self.stage.name(),
                    count,
                    per_s,
                    bytes_per_s: per_s * (LOOPBACK_HEADER_LEN + self.data_len) as f32,
                    lost_count: c.lost,
                    data_corrupted_count: c.corrupted,
                }
            }
        };
        self.progress(report);
        self.progress(TestProgress::TestCompleted(self.stage.name()));
    }

    fn answered(&mut self, now: Instant) {
        self.last_answer_at = now;
        self.timeouts_in_a_row = 0;
    }

    fn is_waiting(&self) -> bool {
        !self.awaiting.is_empty() || self.rx_outstanding > 0
    }

    fn on_reply_timeout(&mut self, now: Instant) {
        self.timeouts_in_a_row += 1;
        self.last_answer_at = now;
        let lost = match self.stage {
            Stage::RoundTrip => 1,
            // replies of the discarded messages are not expected, what got lost is unknown
            Stage::TxSpeed => 0,
            Stage::RxSpeed => self.rx_outstanding,
        };
        warn!(
            "{}: no answer from device for {:?}",
            self.stage.name(),
            self.p.echo_timeout
        );
        if !self.draining {
            self.counts.lost += lost;
        }
        self.awaiting.clear();
        self.rx_outstanding = 0;
        self.rx_expected_seq = 0;
    }

    fn can_send(&self, now: Instant) -> bool {
        if self.draining || now >= self.ends_at {
            return false;
        }
        match self.stage {
            Stage::RoundTrip => self.awaiting.is_empty(),
            Stage::TxSpeed => self.awaiting.len() < self.p.markers_in_flight as usize,
            Stage::RxSpeed => {
                self.rx_outstanding < self.p.batches_in_flight as u64 * self.p.rx_batch as u64
            }
        }
    }

    /// Emit what the window allows, up to [BURST] messages.
    fn pump(&mut self, now: Instant) {
        let was_waiting = self.is_waiting();
        let mut flush = false;
        for _ in 0..BURST {
            if !self.can_send(now) {
                break;
            }
            let seq = self.next_seq;
            self.next_seq = self.next_seq.wrapping_add(1);
            match self.stage {
                Stage::RoundTrip => {
                    self.send(1, seq, self.data(seq));
                    self.awaiting.push_back(seq);
                    self.counts.sent += 1;
                    flush = true;
                }
                Stage::TxSpeed => {
                    if self.since_marker >= self.p.marker_every {
                        self.since_marker = 0;
                        self.send(1, seq, vec![]);
                        self.awaiting.push_back(seq);
                        flush = true;
                    } else {
                        self.since_marker += 1;
                        self.send(0, seq, self.data(seq));
                        self.counts.sent += 1;
                    }
                }
                Stage::RxSpeed => {
                    self.send(self.p.rx_batch, seq, self.data(seq));
                    self.rx_outstanding += self.p.rx_batch as u64;
                    self.counts.sent += 1;
                    flush = true;
                }
            }
        }
        if !was_waiting && self.is_waiting() {
            // the first one in flight, the reply timeout starts now
            self.last_answer_at = now;
        }
        if flush {
            self.out.push_back(TestOut::Flush);
        }
    }

    fn send(&mut self, repeat: u32, seq: u32, data: Vec<u8>) {
        self.out.push_back(TestOut::Send { repeat, seq, data });
    }

    fn data(&self, key: u32) -> Vec<u8> {
        let mut data = vec![0; self.data_len];
        fill(key, &mut data);
        data
    }
}

/// Fill `data` with a self-checking pattern: `key` (u32 LE), then a xorshift32 sequence seeded by it.
/// Data shorter than 4 bytes is all zeros.
pub(crate) fn fill(key: u32, data: &mut [u8]) {
    if data.len() < 4 {
        data.fill(0);
        return;
    }
    data[..4].copy_from_slice(&key.to_le_bytes());
    let mut state = seed(key);
    for b in &mut data[4..] {
        *b = next(&mut state);
    }
}

/// Whether `data` is as [fill] produced it.
pub(crate) fn is_intact(data: &[u8]) -> bool {
    if data.len() < 4 {
        return data.iter().all(|b| *b == 0);
    }
    let key = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let mut state = seed(key);
    data[4..].iter().all(|b| *b == next(&mut state))
}

fn seed(key: u32) -> u32 {
    (key ^ 0x9E37_79B9) | 1
}

fn next(state: &mut u32) -> u8 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    // defaults of LoopbackParams
    const REPLY_TIMEOUT: Duration = Duration::from_millis(500);
    const TIMEOUTS_IN_A_ROW: u32 = 3;
    const MARKER_EVERY: u32 = 32;
    const BATCH: u32 = 64;
    const WINDOW: usize = 2;

    const MSG: usize = 64;
    const SEC: Duration = Duration::from_secs(1);

    struct Harness {
        test: LoopbackTest,
        progress: mpsc::UnboundedReceiver<TestProgress>,
        now: Instant,
    }

    impl Harness {
        fn new() -> Self {
            Self::with_duration(SEC)
        }

        fn with_duration(duration: Duration) -> Self {
            let (tx, progress) = mpsc::unbounded_channel();
            let now = Instant::now();
            Harness {
                test: LoopbackTest::new(now, duration, MSG, LoopbackParams::default(), tx),
                progress,
                now,
            }
        }

        fn sends(&mut self) -> Vec<(u32, u32, Vec<u8>)> {
            let mut v = vec![];
            while let Some(o) = self.test.poll_output() {
                if let TestOut::Send { repeat, seq, data } = o {
                    v.push((repeat, seq, data));
                }
            }
            v
        }

        fn progress(&mut self) -> Vec<TestProgress> {
            let mut v = vec![];
            while let Ok(p) = self.progress.try_recv() {
                if !matches!(p, TestProgress::Completion(..)) {
                    v.push(p);
                }
            }
            v
        }

        fn advance_to(&mut self, at: Instant) -> Status {
            self.now = at;
            self.test.on_timer(at)
        }

        /// Act as a device that answers everything right away, until the current stage ends.
        fn run_stage(&mut self, step: Duration, mut corrupt: impl FnMut(u32) -> bool) -> Status {
            loop {
                for (repeat, seq, data) in self.sends() {
                    let seqs: Vec<u32> = match repeat {
                        0 => vec![],
                        1 => vec![seq],
                        n => (0..n).collect(),
                    };
                    for s in seqs {
                        let intact = is_intact(&data) && !corrupt(s);
                        self.test.on_echo(self.now, s, data.len(), intact);
                    }
                }
                let at = self.now + step;
                let stage = self.test.stage;
                let status = self.advance_to(at);
                if status == Status::Finished || self.test.stage != stage {
                    return status;
                }
            }
        }
    }

    #[test]
    fn pattern_checks_itself() {
        let mut data = vec![0; 100];
        fill(7, &mut data);
        assert!(is_intact(&data));
        assert_eq!(&data[..4], &7u32.to_le_bytes());
        let mut other = vec![0; 100];
        fill(8, &mut other);
        assert_ne!(data[4..], other[4..]);
        data[50] ^= 1;
        assert!(!is_intact(&data));
        assert!(is_intact(&[0, 0]));
    }

    #[test]
    fn three_stages_against_a_good_device() {
        let mut h = Harness::new();
        assert!(matches!(
            h.progress()[..],
            [TestProgress::TestStarted("loopback")]
        ));
        let sends = h.sends();
        assert_eq!(sends.len(), 1, "one round trip at a time");
        assert_eq!(sends[0].0, 1);
        assert_eq!(sends[0].2.len(), MSG - LOOPBACK_HEADER_LEN);
        assert_eq!(h.test.poll_timeout(), h.now + PROGRESS_INTERVAL);
        // answer it, the next goes out
        h.test.on_echo(h.now, sends[0].1, sends[0].2.len(), true);
        let sends = h.sends();
        assert_eq!(sends.len(), 1);
        h.test.on_echo(h.now, 0, MSG - LOOPBACK_HEADER_LEN, true); // stale, ignored
        assert!(h.sends().is_empty());
        h.test.on_echo(h.now, sends[0].1, sends[0].2.len(), true);

        let step = Duration::from_millis(1);
        assert_eq!(h.run_stage(step, |_| false), Status::Running);
        let p = h.progress();
        let TestProgress::LoopbackReport {
            tx_count,
            lost_count: 0,
            data_corrupted_count: 0,
            ..
        } = p[0]
        else {
            panic!("{p:?}")
        };
        assert!(tx_count > 900, "{tx_count}");
        assert!(matches!(
            p[1..],
            [
                TestProgress::TestCompleted("loopback"),
                TestProgress::TestStarted("tx_speed")
            ]
        ));

        // tx_speed: the window holds the host back until markers come back
        let sends = h.sends();
        assert!(sends.len() >= BURST as usize && sends.len() <= MARKER_EVERY as usize);
        assert!(sends.iter().all(|s| s.0 == 0));
        assert_eq!(h.run_stage(step, |_| false), Status::Running);
        let p = h.progress();
        let TestProgress::SpeedReport {
            name: "tx_speed",
            count,
            lost_count: 0,
            ..
        } = p[0]
        else {
            panic!("{p:?}")
        };
        assert!(count > 1000);
        assert!(matches!(p[2], TestProgress::TestStarted("rx_speed")));

        // after the last tx_speed messages sent while the stage was ending
        let batches: Vec<_> = h.sends().into_iter().filter(|s| s.0 == BATCH).collect();
        assert_eq!(batches.len(), WINDOW, "two batches in flight");
        for (_, _, data) in batches {
            for s in 0..BATCH {
                h.test.on_echo(h.now, s, data.len(), true);
            }
        }
        assert_eq!(h.run_stage(step, |_| false), Status::Finished);
        let p = h.progress();
        let TestProgress::SpeedReport {
            name: "rx_speed",
            count,
            lost_count: 0,
            data_corrupted_count: 0,
            bytes_per_s,
            per_s,
        } = p[0]
        else {
            panic!("{p:?}")
        };
        assert!(count > 1000);
        assert_eq!(bytes_per_s, per_s * MSG as f32);
        assert!(matches!(p[1], TestProgress::TestCompleted("rx_speed")));
    }

    #[test]
    fn lost_and_corrupted_are_counted() {
        let mut h = Harness::new();
        h.progress();
        // first echo lost: times out, counted, the next one goes out
        let sends = h.sends();
        assert_eq!(h.test.poll_timeout(), h.now + PROGRESS_INTERVAL);
        let at = h.now + REPLY_TIMEOUT;
        h.advance_to(at);
        let next = h.sends();
        assert_eq!(next.len(), 1);
        assert_ne!(next[0].1, sends[0].1);
        // corrupted echo
        h.test.on_echo(h.now, next[0].1, next[0].2.len(), false);
        // truncated echo
        let next = h.sends();
        h.test.on_echo(h.now, next[0].1, 3, true);
        h.run_stage(Duration::from_millis(10), |_| false);
        let p = h.progress();
        assert!(
            matches!(
                p[0],
                TestProgress::LoopbackReport {
                    lost_count: 1,
                    data_corrupted_count: 2,
                    ..
                }
            ),
            "{p:?}"
        );

        // rx_speed: one corrupted reply
        h.run_stage(Duration::from_millis(10), |_| false);
        h.progress();
        let mut corrupted = false;
        h.run_stage(Duration::from_millis(10), |s| {
            let first = s == 5 && !corrupted;
            corrupted |= first;
            first
        });
        let p = h.progress();
        assert!(
            matches!(
                p[0],
                TestProgress::SpeedReport {
                    name: "rx_speed",
                    lost_count: 0,
                    data_corrupted_count: 1,
                    ..
                }
            ),
            "{p:?}"
        );
    }

    #[test]
    fn rx_speed_gap_is_lost() {
        let mut h = Harness::new();
        h.sends();
        h.test.start_stage(h.now, Stage::RxSpeed);
        let sends = h.sends();
        assert_eq!(sends.len(), 2);
        let len = MSG - LOOPBACK_HEADER_LEN;
        for s in (0..BATCH).filter(|s| *s != 10 && *s != 11) {
            h.test.on_echo(h.now, s, len, true);
        }
        assert_eq!(h.test.counts.lost, 2);
        assert_eq!(h.test.counts.received, BATCH as u64 - 2);
        // the first batch is done, so another one goes out
        assert_eq!(h.sends().len(), 1);
        // the last of a batch lost: seen when the next batch starts
        for s in 0..BATCH - 1 {
            h.test.on_echo(h.now, s, len, true);
        }
        h.test.on_echo(h.now, 0, len, true);
        assert_eq!(h.test.counts.lost, 3);
    }

    #[test]
    fn params_are_used() {
        let (tx, mut progress) = mpsc::unbounded_channel();
        let now = Instant::now();
        let params = LoopbackParams {
            echo_timeout: Duration::from_millis(100),
            timeouts_in_a_row: 1,
            marker_every: 4,
            markers_in_flight: 1,
            rx_batch: 8,
            batches_in_flight: 3,
        };
        let mut test = LoopbackTest::new(now, 10 * SEC, MSG, params, tx);
        fn sends(test: &mut LoopbackTest) -> Vec<(u32, u32)> {
            let mut v = vec![];
            while let Some(o) = test.poll_output() {
                if let TestOut::Send { repeat, seq, .. } = o {
                    v.push((repeat, seq));
                }
            }
            v
        }
        sends(&mut test);
        test.start_stage(now, Stage::TxSpeed);
        let s: Vec<u32> = sends(&mut test).iter().map(|s| s.0).collect();
        assert_eq!(
            s,
            [0, 0, 0, 0, 1],
            "a marker after 4, then waits for its echo"
        );
        test.start_stage(now, Stage::RxSpeed);
        let s: Vec<u32> = sends(&mut test).iter().map(|s| s.0).collect();
        assert_eq!(s, [8, 8, 8]);
        // one timeout of 100 ms is enough to give up
        assert_eq!(
            test.on_timer(now + Duration::from_millis(100)),
            Status::Finished
        );
        let last = std::iter::from_fn(|| progress.try_recv().ok()).last();
        assert!(
            matches!(last, Some(TestProgress::FatalError(_))),
            "{last:?}"
        );
    }

    #[test]
    fn params_are_normalized() {
        let p = LoopbackParams {
            echo_timeout: SEC,
            timeouts_in_a_row: 0,
            marker_every: 0,
            markers_in_flight: 0,
            rx_batch: 1,
            batches_in_flight: 0,
        }
        .normalized();
        assert_eq!(
            (
                p.timeouts_in_a_row,
                p.marker_every,
                p.markers_in_flight,
                p.rx_batch,
                p.batches_in_flight
            ),
            (1, 1, 1, 2, 1)
        );
        let d = LoopbackParams::default();
        assert_eq!(d.echo_timeout, REPLY_TIMEOUT);
        assert_eq!(
            (
                d.timeouts_in_a_row,
                d.marker_every,
                d.markers_in_flight as usize,
                d.rx_batch
            ),
            (TIMEOUTS_IN_A_ROW, MARKER_EVERY, WINDOW, BATCH)
        );
    }

    #[test]
    fn silent_device_fails_the_test() {
        let mut h = Harness::with_duration(10 * SEC);
        h.progress();
        let mut status = Status::Running;
        for _ in 0..TIMEOUTS_IN_A_ROW {
            h.sends();
            let at = h.now + REPLY_TIMEOUT;
            status = h.advance_to(at);
        }
        assert_eq!(status, Status::Finished);
        let p = h.progress();
        assert!(matches!(&p[..], [TestProgress::FatalError(_)]), "{p:?}");
    }
}
