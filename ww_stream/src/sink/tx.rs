//! Host side of a credit sink.

extern crate alloc;

use super::{MAX_WINDOW, extend_pos, wire_pos};
use crate::{Opened, StreamCursor};
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use wire_weaver::shrink_wrap::{UVlq32, UVlq64};
use ww_client_server::StreamSideband;

/// Timing of a credit sink on the host, tune it to the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkConfig {
    /// Send a `Query` when out of credit (or waiting in `flush()`) and nothing came from the device for this long,
    /// and the open again when it wasn't answered. Should be longer than the link's round trip.
    pub query_interval_ms: u64,
    /// The device made no progress for this long (no new credit, nothing accepted): [SinkDue::Stalled]. Longer than
    /// its worst stall (e.g., a flash sector erase). None waits forever.
    pub stall_timeout_ms: Option<u64>,
}

impl Default for SinkConfig {
    fn default() -> Self {
        SinkConfig {
            query_interval_ms: 1000,
            stall_timeout_ms: Some(30_000),
        }
    }
}

/// What to do when waiting took too long, see [SinkTx::due].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkDue {
    /// No answer to the open yet: send it again, with this cursor (repeating an open changes nothing on the device).
    Open(StreamCursor),
    /// Ask the device where it is.
    Query(StreamSideband),
    /// No progress for `stall_timeout_ms`.
    Stalled,
}

/// A write to send, see [SinkTx::try_new] and [SinkTx::next_resend]: the fields of
/// a write to the sink, [RequestKind::sink_write](ww_client_server::RequestKind::sink_write).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkWriteOut<'a> {
    /// Units of the item, None for one.
    pub units: Option<UVlq64>,
    /// Position of its first unit, modulo the wire's [POS_MODULUS](super::POS_MODULUS).
    pub pos: UVlq32,
    /// The item, the serialized value.
    pub data: &'a [u8],
}

/// An item can't be sent at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkTxError {
    /// Bigger than the device's buffer (the largest grant it gave): it could never take it.
    ItemTooBig { len: usize, buffer: u32 },
    /// Every item has at least one unit, and at most [MAX_WINDOW].
    BadUnits { units: u64 },
}

/// The device's answer to an open doesn't fit the host's session, see [SinkTx::opened].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkOpenError {
    /// Not opening, or the answer is for another session (a late one): ignore it.
    Stale,
    /// The device is behind what it acknowledged before: it lost the session (a reboot) and started it again at
    /// `device_at` (0). Everything sent since `device_at` is gone with it; user code decides what to do (a DFU plan
    /// starts over). The sink is closed.
    SessionLost { device_at: u64, acked: u64 },
    /// The device is ahead of what was sent in this session: no device answers that. The sink is closed.
    Ahead { device_at: u64, sent: u64 },
}

/// What [SinkTx::on_sideband] made of a sideband event from the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkSideband {
    /// About credit, taken.
    Credit,
    /// The device has no session for the sink (it rebooted, or never saw the open): open again with
    /// [SinkTx::resume].
    DeviceClosed,
    /// Not about credit: it is for user code.
    Other,
}

#[derive(Debug)]
struct Unacked {
    pos: u64,
    units: u64,
    data: Vec<u8>,
}

impl Unacked {
    fn end(&self) -> u64 {
        self.pos + self.units
    }
}

/// Flow control state of one credit sink on the host, the sender.
///
/// Sans-IO: the host runtime opens with the cursor it returns, sends the writes it hands out, feeds it the device's
/// answers and sideband, and calls [Self::due] when nothing came for [Self::wait_ms]. Time is in milliseconds of
/// any monotonic clock.
///
/// It keeps a copy of every item the device hasn't acknowledged, for a resend (go-back-N) and for a resume after a
/// reconnect: at most one grant of bytes, at most [MAX_WINDOW] units.
#[derive(Debug)]
pub struct SinkTx {
    pub config: SinkConfig,
    /// Session of the host, Some from the open to a close
    session: Option<u32>,
    /// The device answered the open; credit events before that are stale
    synced: bool,
    /// Unit position of the next new item (resends not counted)
    sent: u64,
    /// Units the device accepted in order
    acked: u64,
    /// Bytes of items sent and acknowledged, in the host's own count: the byte reference of the device's grant
    acked_bytes: u64,
    /// Byte limit up to which items may be in flight, in the same count
    limit_bytes: u64,
    /// Largest grant seen: the device's receive buffer (the first grant after an open is all of it), an item bigger
    /// than this can never be sent. At most [MAX_WINDOW].
    window: u32,
    /// Items from `acked` to `sent`
    unacked: VecDeque<Unacked>,
    /// Bytes of `unacked`
    unacked_bytes: u64,
    /// Send everything in `unacked` from this position on again
    resend_from: Option<u64>,
    last_progress: u64,
    last_query: u64,
}

impl SinkTx {
    pub fn new(config: SinkConfig) -> Self {
        SinkTx {
            config,
            session: None,
            synced: false,
            sent: 0,
            acked: 0,
            acked_bytes: 0,
            limit_bytes: 0,
            window: 0,
            unacked: VecDeque::new(),
            unacked_bytes: 0,
            resend_from: None,
            last_progress: 0,
            last_query: 0,
        }
    }

    /// Session of the host, Some from the open to a close (also while the link is down).
    pub fn session(&self) -> Option<u32> {
        self.session
    }

    /// The device answered the open and grants credit.
    pub fn is_synced(&self) -> bool {
        self.synced
    }

    /// Units the device accepted in this session.
    pub fn acked(&self) -> u64 {
        self.acked
    }

    /// Units sent in this session (not counting resends).
    pub fn sent(&self) -> u64 {
        self.sent
    }

    /// Largest grant seen, the device's buffer.
    pub fn window(&self) -> u32 {
        self.window
    }

    /// Where the device is, to save with what user code sent: every unit before `pos` was accepted. A restarted
    /// host passes it to [Self::resume_from].
    pub fn cursor(&self) -> Option<StreamCursor> {
        self.session.map(|session| StreamCursor {
            session: UVlq32(session),
            pos: UVlq64(self.acked),
        })
    }

    /// Open a new session `session` (the host names it: random, or a counter that never repeats with the same
    /// device), positions from 0. Send `StreamOpen` with the returned cursor.
    pub fn open(&mut self, session: u32, now_ms: u64) -> StreamCursor {
        self.open_at(session, 0, now_ms)
    }

    /// [Self::open] with the session starting at position `start`, for tests of long sessions with the device's
    /// [SinkCredit::open_at](super::SinkCredit::open_at).
    pub fn open_at(&mut self, session: u32, start: u64, now_ms: u64) -> StreamCursor {
        *self = SinkTx {
            session: Some(session),
            sent: start,
            acked: start,
            last_progress: now_ms,
            last_query: now_ms,
            ..SinkTx::new(self.config)
        };
        StreamCursor {
            session: UVlq32(session),
            pos: UVlq64(start),
        }
    }

    /// Open the same session again, after a reconnect or a `Close` from the device: what wasn't acknowledged is
    /// kept and sent again from where the device is. Send `StreamOpen` with the returned cursor; None without a
    /// session.
    pub fn resume(&mut self, now_ms: u64) -> Option<StreamCursor> {
        let cursor = self.cursor()?;
        self.synced = false;
        self.resend_from = None;
        self.last_progress = now_ms;
        self.last_query = now_ms;
        Some(cursor)
    }

    /// Open the session of `cursor` again in a new host (a restart), with nothing kept from before: the device
    /// answers where it is, at or after `cursor.pos`, and user code continues from there in its own data. Send
    /// `StreamOpen` with the returned cursor.
    pub fn resume_from(&mut self, cursor: StreamCursor, now_ms: u64) -> StreamCursor {
        self.open_at(cursor.session.0, cursor.pos.0, now_ms)
    }

    /// The device answered the open (its [Opened] and `capacity`, from `StreamOpened`): returns where it is, the
    /// position from which items continue. Its `Ack` with the grant comes next.
    ///
    /// `capacity` is the device's buffer, the most it can ever grant: an item bigger than that fails with
    /// [SinkTxError::ItemTooBig]. Without it (a device that doesn't say), the largest grant seen counts.
    pub fn opened(
        &mut self,
        opened: &Opened,
        capacity: Option<u32>,
        now_ms: u64,
    ) -> Result<u64, SinkOpenError> {
        if self.session != Some(opened.session) {
            return Err(SinkOpenError::Stale);
        }
        if let Some(capacity) = capacity {
            self.window = self.window.max(capacity.min(MAX_WINDOW));
        }
        let device_at = opened.live_from;
        if device_at < self.acked {
            let acked = self.acked;
            self.closed();
            return Err(SinkOpenError::SessionLost { device_at, acked });
        }
        if device_at > self.sent {
            if !self.unacked.is_empty() {
                let sent = self.sent;
                self.closed();
                return Err(SinkOpenError::Ahead { device_at, sent });
            }
            // a restarted host: the device got more than the cursor it saved
            self.sent = device_at;
            self.acked = device_at;
        }
        self.ack_to(device_at);
        self.resend_from = (device_at < self.sent).then_some(device_at);
        // the grant comes with the Ack after the answer
        self.limit_bytes = self.acked_bytes;
        self.synced = true;
        self.last_progress = now_ms;
        self.last_query = now_ms;
        Ok(device_at)
    }

    /// The session is over: `Close` sent. The next item needs a new [Self::open].
    pub fn closed(&mut self) {
        self.session = None;
        self.synced = false;
        self.resend_from = None;
    }

    /// The link went down: credit events are stale until the device answers [Self::resume]'s open.
    pub fn link_lost(&mut self) {
        self.synced = false;
        self.resend_from = None;
    }

    /// Drop acknowledged items up to `have` (an item boundary, or nothing changes) and move `acked` there.
    fn ack_to(&mut self, have: u64) -> bool {
        if have <= self.acked {
            return false;
        }
        if have != self.sent && !self.unacked.iter().any(|u| u.end() == have) {
            return false;
        }
        while let Some(front) = self.unacked.front() {
            if front.end() > have {
                break;
            }
            let len = front.data.len() as u64;
            self.unacked.pop_front();
            self.unacked_bytes -= len;
            self.acked_bytes += len;
        }
        self.acked = have;
        true
    }

    /// Sideband event from the device.
    pub fn on_sideband(&mut self, sideband: &StreamSideband, now_ms: u64) -> SinkSideband {
        let (have, grant, is_resend) = match *sideband {
            StreamSideband::Ack { have, grant, .. } => (have.0, grant.0, false),
            StreamSideband::Resend { from, grant } => (from.0, grant.0, true),
            StreamSideband::Close => {
                return if self.session.is_some() {
                    self.link_lost();
                    SinkSideband::DeviceClosed
                } else {
                    SinkSideband::Credit
                };
            }
            _ => return SinkSideband::Other,
        };
        if !self.synced {
            // left over from before the answer to the open
            return SinkSideband::Credit;
        }
        // nearest to what was acked: what the device has lies between acked and sent, at most a window apart
        let Some(have) = extend_pos(self.acked, have) else {
            return SinkSideband::Credit;
        };
        if have < self.acked || have > self.sent {
            // can't be from this session, or older than what was acknowledged: a late one
            return SinkSideband::Credit;
        }
        let mut progress = have > self.acked;
        if progress && !self.ack_to(have) {
            // not an item boundary: no device sends that
            return SinkSideband::Credit;
        }
        // never more than MAX_WINDOW in flight, or positions on the wire could be read wrong
        let grant = grant.min(MAX_WINDOW);
        let limit = self.acked_bytes + grant as u64;
        if limit > self.limit_bytes {
            self.limit_bytes = limit;
            progress = true;
        }
        self.window = self.window.max(grant);
        if is_resend {
            // answered: no need to ask again before query_interval, but only an advance is progress, so an item the
            // device keeps refusing ends in a stall
            self.last_query = now_ms;
            if have < self.sent {
                self.resend_from = Some(have);
            }
        }
        if progress {
            self.last_progress = now_ms;
            self.last_query = now_ms;
        }
        SinkSideband::Credit
    }

    /// Bytes in flight in front of the item at `idx` of `unacked`.
    fn bytes_before(&self, idx: usize) -> u64 {
        self.unacked
            .iter()
            .take(idx)
            .map(|u| u.data.len() as u64)
            .sum()
    }

    /// Next item to send again after a `Resend` or a resume, if there is credit for it.
    pub fn next_resend(&mut self) -> Option<SinkWriteOut<'_>> {
        let from = self.resend_from?;
        if !self.synced {
            return None;
        }
        let Some(idx) = self.unacked.iter().position(|u| u.end() > from) else {
            self.resend_from = None;
            return None;
        };
        let end_bytes =
            self.acked_bytes + self.bytes_before(idx) + self.unacked[idx].data.len() as u64;
        if end_bytes > self.limit_bytes {
            return None;
        }
        self.resend_from = if idx + 1 < self.unacked.len() {
            Some(self.unacked[idx].end())
        } else {
            None
        };
        let u = &self.unacked[idx];
        Some(write_out(u.pos, u.units, &u.data))
    }

    /// A new item of `units` units, if all of it fits into the credit left; None when the caller has to wait
    /// (resends go first, see [Self::next_resend]).
    pub fn try_new(
        &mut self,
        data: &[u8],
        units: u64,
    ) -> Result<Option<SinkWriteOut<'_>>, SinkTxError> {
        if units == 0 || units > MAX_WINDOW as u64 {
            return Err(SinkTxError::BadUnits { units });
        }
        // no grant seen yet: the device's Ack follows the answer to the open
        if !self.synced || self.window == 0 || self.resend_from.is_some() {
            return Ok(None);
        }
        let len = data.len();
        if len > self.window as usize {
            return Err(SinkTxError::ItemTooBig {
                len,
                buffer: self.window,
            });
        }
        if self.acked_bytes + self.unacked_bytes + len as u64 > self.limit_bytes {
            return Ok(None);
        }
        if self.sent + units - self.acked > MAX_WINDOW as u64 {
            return Ok(None);
        }
        let pos = self.sent;
        self.unacked.push_back(Unacked {
            pos,
            units,
            data: data.to_vec(),
        });
        self.unacked_bytes += len as u64;
        self.sent += units;
        let u = self.unacked.back().expect("just pushed");
        Ok(Some(write_out(u.pos, u.units, &u.data)))
    }

    /// Items are waiting to be sent again (after a resume or a `Resend`), see [Self::next_resend].
    pub fn resending(&self) -> bool {
        self.resend_from.is_some()
    }

    /// Everything sent was accepted by the device.
    pub fn all_acked(&self) -> bool {
        self.synced && self.acked == self.sent && self.resend_from.is_none()
    }

    /// Ask the device where it is right away (e.g., in `flush()`).
    pub fn query_now(&mut self, now_ms: u64) -> StreamSideband {
        self.last_query = now_ms;
        StreamSideband::Query {
            sent: UVlq64(wire_pos(self.sent) as u64),
            backfill: None,
        }
    }

    /// Called when nothing came for [Self::wait_ms]: what to send, or that the device is stuck. None also when
    /// there is no session.
    pub fn due(&mut self, now_ms: u64) -> Option<SinkDue> {
        let cursor = self.cursor()?;
        if let Some(stall) = self.config.stall_timeout_ms
            && now_ms.saturating_sub(self.last_progress) >= stall
        {
            return Some(SinkDue::Stalled);
        }
        if now_ms.saturating_sub(self.last_query) < self.config.query_interval_ms {
            return None;
        }
        if self.synced {
            Some(SinkDue::Query(self.query_now(now_ms)))
        } else {
            self.last_query = now_ms;
            Some(SinkDue::Open(cursor))
        }
    }

    /// How long to wait for an event before calling [Self::due], in ms.
    pub fn wait_ms(&self, now_ms: u64) -> u64 {
        let query = (self.last_query + self.config.query_interval_ms).saturating_sub(now_ms);
        match self.config.stall_timeout_ms {
            Some(stall) => query.min((self.last_progress + stall).saturating_sub(now_ms)),
            None => query,
        }
    }
}

fn write_out(pos: u64, units: u64, data: &[u8]) -> SinkWriteOut<'_> {
    SinkWriteOut {
        units: (units != 1).then_some(UVlq64(units)),
        pos: UVlq32(wire_pos(pos)),
        data,
    }
}
