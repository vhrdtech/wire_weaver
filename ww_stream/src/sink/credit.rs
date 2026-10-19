//! Device side of a credit sink.

use super::{MAX_WINDOW, extend_pos, wire_pos};
use crate::{Opened, StreamCursor};
use wire_weaver::shrink_wrap::{UVlq32, UVlq64};
use ww_client_server::StreamSideband;
use ww_client_server::stream::{StoreStatus, StreamStatus};

/// Flow control state of one credit sink on the device, the receiver.
///
/// Sans-IO: generated server code feeds it the host's open, writes and queries, and sends the [StreamSideband]
/// events it returns; user code only sets the capacity and calls [SinkCredit::free] once it consumed an item.
///
/// It counts units (positions, `have`: every unit before it was accepted in order) and bytes (of items as written:
/// accepted and freed) separately. The host may send while the bytes it has in flight beyond `have` stay within the
/// last grant, `freed + capacity - accepted` bytes.
#[derive(Debug, Clone)]
pub struct SinkCredit {
    capacity: u32,
    threshold: u32,
    /// Session the host opened last, None before the first open
    session: Option<u32>,
    /// The host has it open: from an open to a close. A closed session is kept, so that a late duplicate close
    /// (UDP) costs the host a reopen of the same session, not the session
    open: bool,
    /// Units accepted in order: the position of the next expected write
    have: u64,
    /// Bytes of items accepted in this session
    accepted: u64,
    /// Bytes of them freed by user code
    freed: u64,
    /// Byte limit (`freed + capacity`) last told to the host
    granted: u64,
    /// An Ack is sent on the next [SinkCredit::take_ack] regardless of the threshold (after an open)
    owed: bool,
    /// A write after a gap asks for a resend, once per gap
    resend_armed: bool,
    /// A write or query without a session answers with a Close, once until the next open
    close_armed: bool,
    overruns: u32,
    /// Largest item in bytes if known, 0 otherwise; caps the threshold so a re-grant always fits one more item
    max_item: u32,
    /// Bytes and units of the last accepted item
    last_item_len: u32,
    last_item_units: u64,
}

/// A credit sink's buffer size doesn't work, see [SinkCredit::try_sized_for].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkCreditError {
    /// The buffer can't hold the largest item, the host could never send it.
    BufferTooSmall {
        buffer_bytes: u32,
        max_item_bytes: u32,
    },
    /// The buffer is larger than [MAX_WINDOW], positions on the wire could be read wrong.
    BufferTooLarge { buffer_bytes: u32 },
}

impl core::fmt::Display for SinkCreditError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SinkCreditError::BufferTooSmall {
                buffer_bytes,
                max_item_bytes,
            } => write!(
                f,
                "credit sink buffer of {buffer_bytes} bytes is smaller than its largest item of {max_item_bytes} bytes"
            ),
            SinkCreditError::BufferTooLarge { buffer_bytes } => write!(
                f,
                "credit sink buffer of {buffer_bytes} bytes is larger than the largest window of {MAX_WINDOW} bytes"
            ),
        }
    }
}

/// What generated code does with a write, see [SinkCredit::on_write].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkVerdict {
    /// In order, pass it to the write handler, then send [SinkCredit::take_ack] if there is one.
    Accept,
    /// Already accepted before (a resend overlapping what arrived), drop it.
    Duplicate,
    /// Something before it was lost, drop it and send the event if there is one (a `ResendFrom`, once per gap).
    Gap(Option<StreamSideband>),
    /// Not open: the host didn't open the sink, closed it, or the device lost its session (a reboot). Drop it and
    /// send the event if there is one (a `Close`, once until the next open), so a host that thinks it is open opens
    /// again.
    NoSession(Option<StreamSideband>),
    /// No host sends this (a position not below the modulus, 0 units): drop it.
    Invalid,
}

impl SinkCredit {
    /// Device can buffer `capacity` bytes of this sink, re-granted every `capacity / 2` bytes freed.
    ///
    /// Nothing checks that an item fits into `capacity`, prefer [Self::sized_for] when the largest item is known.
    /// Panics when `capacity` is larger than [MAX_WINDOW] (a compile error in a `const` or `static`).
    pub const fn new(capacity: u32) -> Self {
        if capacity > MAX_WINDOW {
            panic!("credit sink buffer is larger than MAX_WINDOW");
        }
        SinkCredit {
            capacity,
            threshold: Self::clamp_threshold(capacity / 2, capacity, 0),
            session: None,
            open: false,
            have: 0,
            accepted: 0,
            freed: 0,
            granted: 0,
            owed: false,
            resend_armed: true,
            close_armed: true,
            overruns: 0,
            max_item: 0,
            last_item_len: 0,
            last_item_units: 0,
        }
    }

    /// Device can buffer `buffer_bytes` of this sink, whose items take at most `max_item_bytes` (the generated
    /// `<SINK>_MAX_ITEM_BYTES`, None for unbounded types).
    ///
    /// Panics when the buffer can't hold the largest item, the host could never send it, or when it is larger than
    /// [MAX_WINDOW]. In a `const` or `static` that is a compile error:
    ///
    /// ```
    /// # use ww_stream::sink::SinkCredit;
    /// const PLAN_CREDIT: SinkCredit = SinkCredit::sized_for(4096, Some(1027));
    /// ```
    ///
    /// ```compile_fail
    /// # use ww_stream::sink::SinkCredit;
    /// const PLAN_CREDIT: SinkCredit = SinkCredit::sized_for(512, Some(1027));
    /// ```
    ///
    /// ```compile_fail
    /// # use ww_stream::sink::{SinkCredit, MAX_WINDOW};
    /// const PLAN_CREDIT: SinkCredit = SinkCredit::sized_for(MAX_WINDOW + 1, Some(1027));
    /// ```
    ///
    /// The threshold is capped at `buffer_bytes - max_item_bytes + 1`, so a re-grant always lets one more item in.
    pub const fn sized_for(buffer_bytes: u32, max_item_bytes: Option<u32>) -> Self {
        match Self::try_sized_for(buffer_bytes, max_item_bytes) {
            Ok(credit) => credit,
            Err(SinkCreditError::BufferTooSmall { .. }) => {
                panic!("credit sink buffer is smaller than its largest item")
            }
            Err(SinkCreditError::BufferTooLarge { .. }) => {
                panic!("credit sink buffer is larger than MAX_WINDOW")
            }
        }
    }

    /// [Self::sized_for] with an error instead of a panic, for a buffer size known only at runtime.
    pub const fn try_sized_for(
        buffer_bytes: u32,
        max_item_bytes: Option<u32>,
    ) -> Result<Self, SinkCreditError> {
        let max_item = match max_item_bytes {
            Some(max_item) => max_item,
            None => 0,
        };
        if buffer_bytes > MAX_WINDOW {
            return Err(SinkCreditError::BufferTooLarge { buffer_bytes });
        }
        if max_item > buffer_bytes {
            return Err(SinkCreditError::BufferTooSmall {
                buffer_bytes,
                max_item_bytes: max_item,
            });
        }
        let mut credit = Self::new(buffer_bytes);
        credit.max_item = max_item;
        credit.threshold = Self::clamp_threshold(buffer_bytes / 2, buffer_bytes, max_item);
        Ok(credit)
    }

    /// Send a new Ack once the byte limit moved by at least `threshold` bytes (clamped to `1..=capacity`, and to
    /// `capacity - max item + 1` when the largest item is known). A big threshold means few acks over a slow link,
    /// a small one keeps the link busy. With items of different sizes and no known largest one, keep it at most
    /// `capacity - largest item + 1`, or a host waiting for room for a big item only gets it from its next `Query`.
    pub const fn with_threshold(mut self, threshold: u32) -> Self {
        self.threshold = Self::clamp_threshold(threshold, self.capacity, self.max_item);
        self
    }

    const fn clamp_threshold(threshold: u32, capacity: u32, max_item: u32) -> u32 {
        let upper = if max_item > 0 && max_item <= capacity {
            capacity - max_item + 1
        } else {
            capacity
        };
        if threshold == 0 {
            1
        } else if threshold > upper && upper > 0 {
            upper
        } else {
            threshold
        }
    }

    /// Change how much the device can buffer. Credit already granted is not taken back when it shrinks.
    /// Never below the largest item: the host fails a send of an item bigger than the largest grant it saw.
    /// A capacity larger than [MAX_WINDOW] is refused, the old one stays.
    pub fn set_capacity(&mut self, capacity: u32) -> Result<(), SinkCreditError> {
        if capacity > MAX_WINDOW {
            return Err(SinkCreditError::BufferTooLarge {
                buffer_bytes: capacity,
            });
        }
        self.capacity = capacity;
        self.threshold = Self::clamp_threshold(self.threshold, capacity, self.max_item);
        Ok(())
    }

    pub fn set_threshold(&mut self, threshold: u32) {
        self.threshold = Self::clamp_threshold(threshold, self.capacity, self.max_item);
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    /// Session the host opened last, None before the first open (kept after a close).
    pub fn session(&self) -> Option<u32> {
        self.session
    }

    /// The host has the sink open: from an open to a close.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Units accepted in order in this session, also the position of the next expected write.
    pub fn have(&self) -> u64 {
        self.have
    }

    /// Bytes of items accepted in this session.
    pub fn accepted(&self) -> u64 {
        self.accepted
    }

    /// Bytes freed by user code in this session.
    pub fn freed(&self) -> u64 {
        self.freed
    }

    /// Bytes accepted but not freed yet.
    pub fn buffered(&self) -> u32 {
        u32::try_from(self.accepted - self.freed).unwrap_or(u32::MAX)
    }

    /// Bytes of the item the write handler is called with (the serialized value as sent), what to [Self::free] once
    /// it is consumed. Typed values get their size here, a handler of a byte sink can use its data's length as well.
    pub fn last_item_len(&self) -> u32 {
        self.last_item_len
    }

    /// Units of the item the write handler is called with, as the host named them.
    pub fn last_item_units(&self) -> u64 {
        self.last_item_units
    }

    /// Bytes the host sent beyond the credit it was granted, should stay 0.
    pub fn overruns(&self) -> u32 {
        self.overruns
    }

    /// Byte limit up to which the host may send (in the device's count of accepted bytes).
    pub fn limit(&self) -> u64 {
        self.freed + self.capacity as u64
    }

    fn grant(&self) -> u32 {
        u32::try_from(self.limit().saturating_sub(self.accepted)).unwrap_or(u32::MAX)
    }

    /// The host opens the sink with `cursor` (called by generated code on `StreamOpen`). The host names the session:
    /// the one the device is in is kept as it is (a reconnect, a restarted host, a repeated or late duplicate open),
    /// any other starts at position 0. An Ack with the grant is owed either way, send [Self::take_ack] after the
    /// answer.
    ///
    /// The answer has `live_from` (and `oldest`) at `have`: where the host continues. A host whose cursor is past it
    /// had progress in a session the device lost.
    pub fn open(&mut self, cursor: StreamCursor) -> Opened {
        let session = cursor.session.0;
        if self.session != Some(session) {
            self.start(session, 0);
        }
        self.open = true;
        self.owed = true;
        self.resend_armed = true;
        self.opened()
    }

    /// Start `session` at position `pos`, as if `pos` units had passed already. The protocol always starts at 0:
    /// this is for tests of long sessions (many times the modulus) without sending that much, with the host's
    /// [SinkTx::open_at](super::SinkTx::open_at) at the same position.
    pub fn open_at(&mut self, session: u32, pos: u64) -> Opened {
        self.start(session, pos);
        self.open = true;
        self.owed = true;
        self.resend_armed = true;
        self.opened()
    }

    fn start(&mut self, session: u32, pos: u64) {
        self.session = Some(session);
        self.have = pos;
        self.accepted = 0;
        self.freed = 0;
        self.granted = 0;
        self.close_armed = true;
    }

    fn opened(&self) -> Opened {
        Opened {
            session: self.session.unwrap_or(0),
            oldest: self.have,
            live_from: self.have,
            resume_from: None,
        }
    }

    /// The host closed the sink (sideband `Close`). Bytes still buffered are user code's to [Self::free]; the next
    /// session starts with the whole capacity. The session is kept: writes are refused until an open, and an open
    /// of the same session (the host's answer to a late duplicate `Close`) goes on where it was.
    pub fn close(&mut self) {
        self.open = false;
        self.owed = false;
        self.close_armed = true;
    }

    /// A write of an item of `len` bytes and `units` units (1 when the write carries none) at position `pos` (as on
    /// the wire, see [wire_pos]) arrived (called by generated code).
    pub fn on_write(&mut self, pos: u32, units: u64, len: u32) -> SinkVerdict {
        if units == 0 {
            return SinkVerdict::Invalid;
        }
        if !self.open {
            return SinkVerdict::NoSession(self.close_once());
        }
        let Some(pos) = extend_pos(self.have, pos as u64) else {
            if pos as u64 >= super::POS_MODULUS as u64 {
                return SinkVerdict::Invalid;
            }
            // before the start of the session
            return SinkVerdict::Duplicate;
        };
        if pos == self.have {
            let end = self.accepted + len as u64;
            let allowed = self.granted.max(self.limit());
            if end > allowed {
                let over = end - allowed.max(self.accepted).min(end);
                self.overruns = self
                    .overruns
                    .saturating_add(u32::try_from(over).unwrap_or(u32::MAX));
            }
            self.have = pos.saturating_add(units);
            self.accepted = end;
            self.last_item_len = len;
            self.last_item_units = units;
            self.resend_armed = true;
            SinkVerdict::Accept
        } else if pos < self.have {
            SinkVerdict::Duplicate
        } else if self.resend_armed {
            self.resend_armed = false;
            SinkVerdict::Gap(Some(self.resend()))
        } else {
            SinkVerdict::Gap(None)
        }
    }

    /// User code consumed `len` bytes (whole items or parts of them) and the space can be given back to the host.
    /// Send the result of [Self::take_ack] afterwards when not in a write handler (generated code does it after
    /// each write).
    pub fn free(&mut self, len: u32) {
        self.freed = (self.freed + len as u64).min(self.accepted);
    }

    /// Ack to send to the host, if one is owed (after an open) or the limit moved by at least the threshold since
    /// the last one.
    pub fn take_ack(&mut self) -> Option<StreamSideband> {
        if !self.open {
            return None;
        }
        let limit = self.limit();
        if self.owed || limit >= self.granted + self.threshold as u64 {
            Some(self.ack())
        } else {
            None
        }
    }

    /// Answer to `Query { sent }` from the host (called by generated code, `sent` as on the wire): `ResendFrom` if
    /// writes before `sent` are missing, `Ack` otherwise; `Close` (once) without a session. None for a position
    /// no host sends.
    pub fn on_query(&mut self, sent: u64) -> Option<StreamSideband> {
        if !self.open {
            return self.close_once();
        }
        let sent = extend_pos(self.have, sent)?;
        if sent > self.have {
            self.resend_armed = false;
            Some(self.resend())
        } else {
            Some(self.ack())
        }
    }

    /// State for a read on the sink: open, session, position, bytes buffered and capacity.
    pub fn status(&self) -> StreamStatus {
        StreamStatus {
            open: self.open,
            session: self.session.map(UVlq32),
            next: UVlq64(self.have),
            oldest: None,
            newest: None,
            in_flight: UVlq64(0),
            store: Some(StoreStatus {
                used_bytes: UVlq64(self.buffered() as u64),
                capacity_bytes: UVlq64(self.capacity as u64),
                persistent: false,
                dropped_units: UVlq64(0),
            }),
        }
    }

    fn close_once(&mut self) -> Option<StreamSideband> {
        if self.close_armed {
            self.close_armed = false;
            Some(StreamSideband::Close)
        } else {
            None
        }
    }

    fn ack(&mut self) -> StreamSideband {
        self.granted = self.granted.max(self.limit());
        self.owed = false;
        StreamSideband::Ack {
            have: UVlq64(wire_pos(self.have) as u64),
            backfill: None,
            grant: UVlq32(self.grant()),
        }
    }

    fn resend(&mut self) -> StreamSideband {
        self.granted = self.granted.max(self.limit());
        self.owed = false;
        StreamSideband::ResendFrom {
            from: UVlq64(wire_pos(self.have) as u64),
            grant: UVlq32(self.grant()),
        }
    }
}
