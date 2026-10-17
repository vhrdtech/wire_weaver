//! Host side of credit-based flow control for `#[credit]` sinks, see `docs/api/credit_sinks.md`.
//!
//! [CreditTx] is sans-IO, [crate::Sink] drives it from async, blocking and non-blocking (`try_send`) code.

use std::collections::VecDeque;
use std::time::{Duration, Instant};
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use wire_weaver::shrink_wrap::{SerializeShrinkWrapOwned, UNib32, UVlq32};
use ww_client_server::credit::{MAX_WINDOW, extend_pos, item_span, wire_pos};
use ww_client_server::{CreditWriteOwned, StreamSideband};

use crate::StreamError;

/// Timing of a credit sink, tune it to the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreditConfig {
    /// Send `CreditQuery` when out of credit (or waiting in `flush()`) and nothing came from the device for this
    /// long. Should be longer than the link's round trip.
    pub query_interval: Duration,
    /// Fail with [crate::Error::Timeout] when the device made no progress for this long (no new credit, nothing accepted),
    /// longer than its worst stall (e.g., a flash sector erase). None waits forever.
    pub stall_timeout: Option<Duration>,
}

impl Default for CreditConfig {
    fn default() -> Self {
        CreditConfig {
            query_interval: Duration::from_secs(1),
            stall_timeout: Some(Duration::from_secs(30)),
        }
    }
}

/// What to do when waiting took too long, see [CreditTx::due].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Due {
    /// No answer to Open yet, send it again
    Open,
    /// Ask the device where it is
    Query(StreamSideband),
    /// No progress for `stall_timeout`
    Stalled,
}

/// Flow control state of one credit sink on the host.
#[derive(Debug)]
pub(crate) struct CreditTx {
    pub(crate) config: CreditConfig,
    /// Open was sent for the current session
    opened: bool,
    /// The device answered the Open (a Credit or Resend with `have` at the start), credit events before that are
    /// stale
    synced: bool,
    /// Byte position of the next new item (bytes of items sent since Open, resends not counted). Positions are full
    /// `u64` here, the wire carries them modulo `POS_MODULUS` (`ww_client_server::credit`).
    sent: u64,
    /// Bytes the device accepted in order
    acked: u64,
    /// Position up to which the host may send
    limit: u64,
    /// Largest grant seen: the device's receive buffer (the first grant after Open is all of it), an item bigger than
    /// this can never be sent. At most [MAX_WINDOW].
    window: u32,
    /// Items from `acked` to `sent`, kept for a resend: (byte position, item bytes)
    unacked: VecDeque<(u64, Vec<u8>)>,
    /// Resend everything in `unacked` from this position on
    resend_from: Option<u64>,
    last_progress: Instant,
    last_query: Instant,
}

impl CreditTx {
    pub(crate) fn new(now: Instant) -> Self {
        CreditTx {
            config: CreditConfig::default(),
            opened: false,
            synced: false,
            sent: 0,
            acked: 0,
            limit: 0,
            window: 0,
            unacked: VecDeque::new(),
            resend_from: None,
            last_progress: now,
            last_query: now,
        }
    }

    pub(crate) fn is_opened(&self) -> bool {
        self.opened
    }

    /// Bytes the device accepted in this session.
    pub(crate) fn acked(&self) -> u64 {
        self.acked
    }

    /// Bytes sent in this session (not counting resends).
    pub(crate) fn sent(&self) -> u64 {
        self.sent
    }

    /// Start a new session, the caller sends Open.
    pub(crate) fn opening(&mut self, now: Instant) {
        self.opening_at(now, 0);
    }

    /// [Self::opening] with the session starting at byte position `start`, for tests of long sessions with the
    /// device's `SinkCredit::open_at`.
    pub(crate) fn opening_at(&mut self, now: Instant, start: u64) {
        *self = CreditTx {
            config: self.config,
            opened: true,
            sent: start,
            acked: start,
            limit: start,
            ..CreditTx::new(now)
        };
    }

    /// Session is over: Close sent or the link went down, the next value opens a new one.
    pub(crate) fn closed(&mut self) {
        self.opened = false;
        self.synced = false;
    }

    /// Sideband event from the device, returns false if it isn't about credit (it is for the user then).
    pub(crate) fn on_sideband(&mut self, sideband: StreamSideband, now: Instant) -> bool {
        let (have, grant, is_resend) = match sideband {
            StreamSideband::Credit { have, grant } => (have.0, grant.0, false),
            StreamSideband::Resend { have, grant } => (have.0, grant.0, true),
            _ => return false,
        };
        if !self.opened {
            return true;
        }
        if !self.synced {
            // left over from a previous session, until the answer to Open
            if have != wire_pos(self.acked) {
                return true;
            }
            self.synced = true;
            self.last_progress = now;
        }
        // nearest to what was acked: what the device has lies between acked and sent, at most a window apart
        let Some(have) = extend_pos(self.acked, have) else {
            return true;
        };
        if have > self.sent {
            // can't be from this session
            return true;
        }
        // never more than MAX_WINDOW in flight, or positions on the wire could be read wrong
        let grant = grant.min(MAX_WINDOW);
        let mut progress = false;
        while let Some((pos, value)) = self.unacked.front() {
            if pos + item_span(value.len() as u32) as u64 <= have {
                self.unacked.pop_front();
            } else {
                break;
            }
        }
        if have > self.acked {
            self.acked = have;
            progress = true;
        }
        let limit = have + grant as u64;
        if limit > self.limit {
            self.limit = limit;
            progress = true;
        }
        self.window = self.window.max(grant);
        if is_resend {
            // answered: no need to ask again before query_interval, but only an advance is progress, so a value
            // the device keeps refusing ends in a stall timeout
            self.last_query = now;
            if have < self.sent {
                self.resend_from = Some(have);
            }
        }
        if progress {
            self.last_progress = now;
            self.last_query = now;
        }
        true
    }

    /// Next value to send again after a `Resend`, if there is credit for it.
    pub(crate) fn next_resend(&mut self) -> Result<Option<Vec<u8>>, StreamError> {
        let Some(from) = self.resend_from else {
            return Ok(None);
        };
        let Some(idx) = self
            .unacked
            .iter()
            .position(|(pos, value)| *pos + item_span(value.len() as u32) as u64 > from)
        else {
            self.resend_from = None;
            return Ok(None);
        };
        let (pos, value) = &self.unacked[idx];
        let end = *pos + item_span(value.len() as u32) as u64;
        if end > self.limit {
            return Ok(None);
        }
        let bytes = encode(*pos, value)?;
        self.resend_from = if idx + 1 < self.unacked.len() {
            Some(end)
        } else {
            None
        };
        Ok(Some(bytes))
    }

    /// Encoded write of a new item if all of it fits into the credit left, None when the caller has to wait.
    /// Resends go first, see [Self::next_resend].
    pub(crate) fn try_new(&mut self, value: &[u8]) -> Result<Option<Vec<u8>>, StreamError> {
        if !self.synced || self.resend_from.is_some() {
            return Ok(None);
        }
        let len = u32::try_from(value.len()).map_err(|_| too_big(value.len(), self.window))?;
        if len > self.window {
            return Err(too_big(value.len(), self.window));
        }
        let end = self.sent + item_span(len) as u64;
        if end > self.limit {
            return Ok(None);
        }
        let bytes = encode(self.sent, value)?;
        self.unacked.push_back((self.sent, value.to_vec()));
        self.sent = end;
        Ok(Some(bytes))
    }

    /// Everything sent was accepted by the device.
    pub(crate) fn all_acked(&self) -> bool {
        self.synced && self.acked == self.sent && self.resend_from.is_none()
    }

    /// Ask the device where it is right away (e.g., in `flush()`).
    pub(crate) fn query_now(&mut self, now: Instant) -> StreamSideband {
        self.last_query = now;
        StreamSideband::CreditQuery {
            sent: UNib32(wire_pos(self.sent)),
        }
    }

    /// Called when nothing came for [Self::wait_time]: what to send, or that the device is stuck.
    pub(crate) fn due(&mut self, now: Instant) -> Option<Due> {
        if let Some(stall) = self.config.stall_timeout
            && now.duration_since(self.last_progress) >= stall
        {
            return Some(Due::Stalled);
        }
        if now.duration_since(self.last_query) < self.config.query_interval {
            return None;
        }
        if self.synced {
            Some(Due::Query(self.query_now(now)))
        } else {
            self.last_query = now;
            Some(Due::Open)
        }
    }

    /// How long to wait for an event before calling [Self::due].
    pub(crate) fn wait_time(&self, now: Instant) -> Duration {
        let query = (self.last_query + self.config.query_interval).saturating_duration_since(now);
        match self.config.stall_timeout {
            Some(stall) => query.min((self.last_progress + stall).saturating_duration_since(now)),
            None => query,
        }
    }
}

fn encode(pos: u64, value: &[u8]) -> Result<Vec<u8>, StreamError> {
    let write = CreditWriteOwned {
        pos: UVlq32(wire_pos(pos)),
        data: TailBytesOwned(value.to_vec()),
    };
    Ok(write.to_ww_bytes_owned()?)
}

fn too_big(len: usize, buffer: u32) -> StreamError {
    StreamError::ItemTooBig { len, buffer }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire_weaver::shrink_wrap::{BufReader, DeserializeShrinkWrap};
    use ww_client_server::CreditWrite;

    fn credit(have: u64, grant: u32) -> StreamSideband {
        StreamSideband::Credit {
            have: UNib32(wire_pos(have)),
            grant: UNib32(grant),
        }
    }

    fn resend(have: u64, grant: u32) -> StreamSideband {
        StreamSideband::Resend {
            have: UNib32(wire_pos(have)),
            grant: UNib32(grant),
        }
    }

    fn pos(bytes: &[u8]) -> u32 {
        let mut rd = BufReader::new(bytes);
        CreditWrite::des_shrink_wrap(&mut rd).unwrap().pos.0
    }

    fn synced(window: u32) -> (CreditTx, Instant) {
        let now = Instant::now();
        let mut c = CreditTx::new(now);
        c.opening(now);
        assert!(c.on_sideband(credit(0, window), now));
        (c, now)
    }

    #[test]
    fn waits_for_initial_credit_and_ignores_stale() {
        let now = Instant::now();
        let mut c = CreditTx::new(now);
        c.opening(now);
        assert_eq!(c.try_new(&[1]).unwrap(), None);
        // from a previous session
        assert!(c.on_sideband(credit(100, 10), now));
        assert_eq!(c.try_new(&[1]).unwrap(), None);
        assert!(!c.on_sideband(StreamSideband::Open, now));
        assert!(c.on_sideband(credit(0, 10), now));
        assert_eq!(pos(&c.try_new(&[1]).unwrap().unwrap()), 0);
    }

    #[test]
    fn never_exceeds_credit() {
        let (mut c, now) = synced(10);
        assert!(c.try_new(&[0; 4]).unwrap().is_some());
        assert!(c.try_new(&[0; 4]).unwrap().is_some());
        assert_eq!(c.try_new(&[0; 4]).unwrap(), None);
        assert!(matches!(
            c.try_new(&[0; 11]),
            Err(StreamError::ItemTooBig {
                len: 11,
                buffer: 10
            })
        ));
        c.on_sideband(credit(4, 10), now);
        assert_eq!(pos(&c.try_new(&[0; 4]).unwrap().unwrap()), 8);
        assert!(!c.all_acked());
        c.on_sideband(credit(12, 10), now);
        assert!(c.all_acked());
        assert!(c.unacked.is_empty());
    }

    #[test]
    fn go_back_n() {
        let (mut c, now) = synced(100);
        for _ in 0..4 {
            c.try_new(&[0; 10]).unwrap().unwrap();
        }
        // 10..20 lost, the device saw 20..30
        c.on_sideband(resend(10, 90), now);
        assert_eq!(c.try_new(&[0; 10]).unwrap(), None, "resends go first");
        let resent: Vec<u32> = std::iter::from_fn(|| c.next_resend().unwrap())
            .map(|b| pos(&b))
            .collect();
        assert_eq!(resent, [10, 20, 30]);
        assert_eq!(pos(&c.try_new(&[0; 10]).unwrap().unwrap()), 40);
    }

    #[test]
    fn go_back_n_mixed_sizes() {
        let (mut c, now) = synced(100);
        let sizes = [3usize, 40, 1, 20];
        let sent: Vec<u32> = sizes
            .iter()
            .map(|len| pos(&c.try_new(&vec![0; *len]).unwrap().unwrap()))
            .collect();
        assert_eq!(sent, [0, 3, 43, 44]);
        // 36 left, a 37 byte item waits
        assert_eq!(c.try_new(&[0; 37]).unwrap(), None);
        // the 40 byte item was lost: resend from its first byte
        c.on_sideband(resend(3, 97), now);
        let resent: Vec<u32> = std::iter::from_fn(|| c.next_resend().unwrap())
            .map(|b| pos(&b))
            .collect();
        assert_eq!(resent, [3, 43, 44]);
        c.on_sideband(credit(64, 100), now);
        assert!(c.all_acked());
        assert_eq!(pos(&c.try_new(&[0; 37]).unwrap().unwrap()), 64);
    }

    #[test]
    fn empty_item_takes_a_position() {
        let (mut c, now) = synced(10);
        assert_eq!(pos(&c.try_new(&[]).unwrap().unwrap()), 0);
        assert_eq!(pos(&c.try_new(&[0; 3]).unwrap().unwrap()), 1);
        c.on_sideband(resend(0, 10), now);
        let resent: Vec<u32> = std::iter::from_fn(|| c.next_resend().unwrap())
            .map(|b| pos(&b))
            .collect();
        assert_eq!(resent, [0, 1]);
        c.on_sideband(credit(4, 10), now);
        assert!(c.all_acked());
    }

    #[test]
    fn credit_does_not_rewind() {
        let (mut c, now) = synced(100);
        c.try_new(&[0; 10]).unwrap().unwrap();
        c.try_new(&[0; 10]).unwrap().unwrap();
        c.on_sideband(credit(10, 90), now);
        assert_eq!(c.next_resend().unwrap(), None);
        assert_eq!(c.acked(), 10);
    }

    #[test]
    fn query_and_stall() {
        let now = Instant::now();
        let mut c = CreditTx::new(now);
        c.config.query_interval = Duration::from_millis(10);
        c.config.stall_timeout = Some(Duration::from_millis(35));
        c.opening(now);
        assert_eq!(c.due(now), None);
        assert_eq!(c.wait_time(now), Duration::from_millis(10));
        assert_eq!(c.due(now + Duration::from_millis(10)), Some(Due::Open));
        c.on_sideband(credit(0, 10), now + Duration::from_millis(15));
        c.try_new(&[0; 10]).unwrap().unwrap();
        assert_eq!(
            c.due(now + Duration::from_millis(25)),
            Some(Due::Query(StreamSideband::CreditQuery { sent: UNib32(10) }))
        );
        assert_eq!(c.due(now + Duration::from_millis(50)), Some(Due::Stalled));
    }

    #[test]
    fn positions_wrap_on_the_wire() {
        const M: u64 = ww_client_server::credit::POS_MODULUS as u64;
        for start in [M - 25, 3 * M - 1, (1 << 40) - 7] {
            let now = Instant::now();
            let mut c = CreditTx::new(now);
            c.opening_at(now, start);
            // stale: not the start of this session
            assert!(c.on_sideband(credit(0, 100), now));
            assert_eq!(c.try_new(&[0; 10]).unwrap(), None);
            assert!(c.on_sideband(credit(start, 100), now));
            let sent: Vec<u32> = (0..5)
                .map(|_| pos(&c.try_new(&[0; 10]).unwrap().unwrap()))
                .collect();
            let wire = wire_pos;
            assert_eq!(
                sent,
                [0, 10, 20, 30, 40].map(|d| wire(start + d)),
                "wraps after the modulus"
            );
            // the write past the wrap was lost
            c.on_sideband(resend(start + 30, 70), now);
            let resent: Vec<u32> = std::iter::from_fn(|| c.next_resend().unwrap())
                .map(|b| pos(&b))
                .collect();
            assert_eq!(resent, [wire(start + 30), wire(start + 40)]);
            c.on_sideband(credit(start + 50, 100), now);
            assert!(c.all_acked());
            assert_eq!((c.acked(), c.sent()), (start + 50, start + 50));
        }
    }

    #[test]
    fn never_more_than_max_window_in_flight() {
        let (mut c, now) = synced(10);
        c.on_sideband(credit(0, u32::MAX), now);
        assert_eq!(c.limit, MAX_WINDOW as u64);
        assert_eq!(c.window, MAX_WINDOW);
    }
}
