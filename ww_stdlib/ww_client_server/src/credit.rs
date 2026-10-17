//! Device side of credit-based flow control for `#[credit]` sinks, see `docs/api/credit_sinks.md`.
//!
//! [SinkCredit] is sans-IO: generated server code feeds it writes and sideband commands and sends the
//! [StreamSideband] events it returns, user code only sets the capacity and calls [SinkCredit::free].
//!
//! Credit counts bytes of sink items: the serialized value of each write, as sent. Items are never split, the host
//! sends one only when it fits wholly into the credit left, so the device must always be able to grant at least its
//! largest item. [SinkCredit::sized_for] checks that against the generated
//! `<Level>StreamSerializer::<SINK>_MAX_ITEM_BYTES` (also `stream_data_ser().<sink>_max_item_bytes()`).
//!
//! Positions are `u64` on both sides, a session has no size limit. On the wire they are taken modulo
//! [POS_MODULUS] (2^28, at most 4 bytes as the `UVlq32` of a write, see [wire_pos]), and the receiver gets the full position back
//! from the one it expects ([extend_pos], serial number arithmetic as in TCP). That works as long as every position
//! on the wire is less than 2^27 away from what the receiver expects: credit keeps what is in flight within the
//! device's buffer, and [MAX_WINDOW] keeps the buffer at a quarter of the modulus.

use crate::StreamSideband;
use wire_weaver::shrink_wrap::UNib32;

/// Positions of credit sinks go on the wire modulo 2^28: the most a `UVlq32` holds in 4 bytes (`CreditWrite::pos`),
/// a `UNib32` in 10 nibbles (the positions in `StreamSideband::Credit`, `CreditQuery` and `Resend`).
pub const POS_MODULUS: u32 = 1 << 28;

/// Largest credit window, the most bytes a sink lets the host have in flight (its capacity): 64 MiB, a quarter of
/// [POS_MODULUS].
///
/// A position on the wire is read right when it is less than `POS_MODULUS / 2` bytes away from the one the receiver
/// expects. Everything in flight lies within one window around it, so two windows take half of the modulus, the
/// other half is the margin for a late packet (a duplicate delayed on UDP): one that arrives more than 64 MiB of
/// traffic late looks like a write from ahead (a gap, answered with a Resend), only 256 MiB late it would look like
/// the expected one. [SinkCredit] refuses a larger capacity, the host never puts more than this in flight, whatever
/// the device grants.
pub const MAX_WINDOW: u32 = POS_MODULUS / 4;

/// A position as sent on the wire: modulo [POS_MODULUS].
pub const fn wire_pos(pos: u64) -> u32 {
    (pos % POS_MODULUS as u64) as u32
}

/// Full position of the wire position `wire`, the one nearest to `near` (the position the receiver expects): from
/// `near - 2^27` to `near + 2^27 - 1`. None when `wire` isn't below [POS_MODULUS] (an invalid one), or when the
/// position falls before the start of the session (a late packet from a previous one).
pub const fn extend_pos(near: u64, wire: u32) -> Option<u64> {
    if wire >= POS_MODULUS {
        return None;
    }
    let delta = wire.wrapping_sub(near as u32) & (POS_MODULUS - 1);
    let delta = if delta >= POS_MODULUS / 2 {
        delta as i64 - POS_MODULUS as i64
    } else {
        delta as i64
    };
    near.checked_add_signed(delta)
}

/// Flow control state of one credit sink on the device.
///
/// All counters are byte positions from the start of the session (`Open`): `have` bytes were accepted in order,
/// `freed` of them were consumed by user code, so the device can take up to `freed + capacity`.
#[derive(Debug, Clone)]
pub struct SinkCredit {
    capacity: u32,
    threshold: u32,
    have: u64,
    freed: u64,
    /// Limit last told to the host, 0 until the first Credit after Open
    granted: u64,
    /// A Credit is sent on the next [SinkCredit::take_credit] regardless of the threshold (after Open)
    owed: bool,
    /// A write after a gap sends a Resend, once per gap
    resend_armed: bool,
    overruns: u32,
    /// Largest item in bytes if known, 0 otherwise; caps the threshold so a re-grant always fits one more item
    max_item: u32,
    /// Bytes of the last accepted item
    last_item_len: u32,
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
pub enum CreditVerdict {
    /// In order, pass it to the write handler.
    Accept,
    /// Already accepted before (a resend overlapping what arrived), drop it.
    Duplicate,
    /// Something before it was lost, drop it and send the event if there is one (a Resend, once per gap).
    Gap(Option<StreamSideband>),
    /// The position isn't below [POS_MODULUS], no host sends that: drop it.
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
            have: 0,
            freed: 0,
            granted: 0,
            owed: true,
            resend_armed: true,
            overruns: 0,
            max_item: 0,
            last_item_len: 0,
        }
    }

    /// Device can buffer `buffer_bytes` of this sink, whose items take at most `max_item_bytes` (the generated
    /// `<SINK>_MAX_ITEM_BYTES`, None for unbounded types).
    ///
    /// Panics when the buffer can't hold the largest item, the host could never send it, or when it is larger than
    /// [MAX_WINDOW]. In a `const` or `static` that is a compile error:
    ///
    /// ```
    /// # use ww_client_server::credit::SinkCredit;
    /// const PLAN_CREDIT: SinkCredit = SinkCredit::sized_for(4096, Some(1027));
    /// ```
    ///
    /// ```compile_fail
    /// # use ww_client_server::credit::SinkCredit;
    /// const PLAN_CREDIT: SinkCredit = SinkCredit::sized_for(512, Some(1027));
    /// ```
    ///
    /// ```compile_fail
    /// # use ww_client_server::credit::{SinkCredit, MAX_WINDOW};
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

    /// Send a new Credit once the limit moved by at least `threshold` bytes (clamped to `1..=capacity`, and to
    /// `capacity - max item + 1` when the largest item is known). A big threshold means few acks over a slow link,
    /// a small one keeps the link busy. With items of different sizes and no known largest one, keep it at most
    /// `capacity - largest item + 1`, or a host waiting for room for a big item only gets it from its next
    /// `CreditQuery`.
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
    /// Never below the largest item: the host fails a send of an item bigger than the largest credit it saw.
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

    /// Bytes accepted in order since Open, also the position of the next expected write.
    pub fn have(&self) -> u64 {
        self.have
    }

    /// Bytes freed by user code since Open.
    pub fn freed(&self) -> u64 {
        self.freed
    }

    /// Bytes accepted but not freed yet.
    pub fn buffered(&self) -> u32 {
        u32::try_from(self.have - self.freed).unwrap_or(u32::MAX)
    }

    /// Bytes of the item the write handler is called with (the serialized value as sent), what to [Self::free] once
    /// it is consumed. Typed values get their size here, a handler of a byte sink can use its data's length as well.
    pub fn last_item_len(&self) -> u32 {
        self.last_item_len
    }

    /// Bytes the host sent beyond the credit it was granted, should stay 0.
    pub fn overruns(&self) -> u32 {
        self.overruns
    }

    /// Position up to which the host may send.
    pub fn limit(&self) -> u64 {
        self.freed + self.capacity as u64
    }

    fn grant(&self) -> u32 {
        u32::try_from(self.limit().saturating_sub(self.have)).unwrap_or(u32::MAX)
    }

    /// Start a new session (called by generated code on Open): counters reset, a Credit with the whole capacity is
    /// owed to the host.
    pub fn open(&mut self) {
        self.open_at(0);
    }

    /// [Self::open], with the session starting at byte position `pos` instead of 0, as if `pos` bytes had passed
    /// already. The protocol always starts at 0: this is for tests of long sessions (many times the modulus) without
    /// sending that much, with the host starting at the same position.
    pub fn open_at(&mut self, pos: u64) {
        self.have = pos;
        self.freed = pos;
        self.granted = 0;
        self.owed = true;
        self.resend_armed = true;
    }

    /// A write of an item of `len` bytes at byte position `pos` (as on the wire, see [wire_pos]) arrived (called by
    /// generated code).
    ///
    /// An empty item takes one position, so that each item has its own, and is freed right away: it needs no room.
    pub fn on_write(&mut self, pos: u32, len: u32) -> CreditVerdict {
        if pos >= POS_MODULUS {
            return CreditVerdict::Invalid;
        }
        let Some(pos) = extend_pos(self.have, pos) else {
            // before the start of the session
            return CreditVerdict::Duplicate;
        };
        if pos == self.have {
            let end = pos + item_span(len) as u64;
            let allowed = self.granted.max(self.limit());
            if end > allowed {
                let over = end - allowed.max(pos).min(end);
                self.overruns = self
                    .overruns
                    .saturating_add(u32::try_from(over).unwrap_or(u32::MAX));
            }
            self.have = end;
            if len == 0 {
                self.freed += 1;
            }
            self.last_item_len = len;
            self.resend_armed = true;
            CreditVerdict::Accept
        } else if pos < self.have {
            CreditVerdict::Duplicate
        } else if self.resend_armed {
            self.resend_armed = false;
            CreditVerdict::Gap(Some(self.resend()))
        } else {
            CreditVerdict::Gap(None)
        }
    }

    /// User code consumed `len` bytes (whole items or parts of them) and the space can be given back to the host.
    /// Send the result of [Self::take_credit] afterwards when not in a write handler (generated code does it after
    /// each write).
    pub fn free(&mut self, len: u32) {
        self.freed = (self.freed + len as u64).min(self.have);
    }

    /// Credit event to send to the host, if one is owed or the limit moved by at least the threshold since the last
    /// one.
    pub fn take_credit(&mut self) -> Option<StreamSideband> {
        let limit = self.limit();
        if self.owed || limit >= self.granted + self.threshold as u64 {
            Some(self.credit())
        } else {
            None
        }
    }

    /// Answer to `CreditQuery { sent }` from the host (called by generated code, `sent` as on the wire): `Resend` if
    /// writes before `sent` are missing, `Credit` otherwise.
    pub fn query(&mut self, sent: u32) -> StreamSideband {
        if extend_pos(self.have, sent).is_some_and(|sent| sent > self.have) {
            self.resend_armed = false;
            self.resend()
        } else {
            self.credit()
        }
    }

    fn credit(&mut self) -> StreamSideband {
        self.granted = self.granted.max(self.limit());
        self.owed = false;
        StreamSideband::Credit {
            have: UNib32(wire_pos(self.have)),
            grant: UNib32(self.grant()),
        }
    }

    fn resend(&mut self) -> StreamSideband {
        self.granted = self.granted.max(self.limit());
        self.owed = false;
        StreamSideband::Resend {
            have: UNib32(wire_pos(self.have)),
            grant: UNib32(self.grant()),
        }
    }
}

/// Positions an item of `len` bytes takes: its bytes, one for an empty item.
pub const fn item_span(len: u32) -> u32 {
    if len == 0 { 1 } else { len }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credit(have: u64, grant: u32) -> Option<StreamSideband> {
        Some(StreamSideband::Credit {
            have: UNib32(wire_pos(have)),
            grant: UNib32(grant),
        })
    }

    fn resend(have: u64, grant: u32) -> StreamSideband {
        StreamSideband::Resend {
            have: UNib32(wire_pos(have)),
            grant: UNib32(grant),
        }
    }

    #[test]
    fn initial_credit_and_threshold() {
        let mut c = SinkCredit::new(100).with_threshold(30);
        c.open();
        assert_eq!(c.take_credit(), credit(0, 100));
        assert_eq!(c.take_credit(), None);
        assert_eq!(c.on_write(wire_pos(0), 40), CreditVerdict::Accept);
        assert_eq!(c.on_write(wire_pos(40), 40), CreditVerdict::Accept);
        assert_eq!(c.buffered(), 80);
        c.free(20);
        assert_eq!(c.take_credit(), None, "limit moved by 20 < 30");
        c.free(20);
        assert_eq!(c.take_credit(), credit(80, 60));
        assert_eq!(c.limit(), 140);
        assert_eq!(c.overruns(), 0);
    }

    #[test]
    fn overrun_counted() {
        let mut c = SinkCredit::new(10);
        assert_eq!(c.take_credit(), credit(0, 10));
        assert_eq!(c.on_write(wire_pos(0), 8), CreditVerdict::Accept);
        assert_eq!(c.on_write(wire_pos(8), 8), CreditVerdict::Accept);
        assert_eq!(c.overruns(), 6);
    }

    #[test]
    fn gap_sends_resend_once() {
        let mut c = SinkCredit::new(100);
        c.take_credit();
        assert_eq!(c.on_write(wire_pos(0), 10), CreditVerdict::Accept);
        // 10..20 lost
        assert_eq!(
            c.on_write(wire_pos(20), 10),
            CreditVerdict::Gap(Some(resend(10, 90)))
        );
        assert_eq!(c.on_write(wire_pos(30), 10), CreditVerdict::Gap(None));
        // resend from 10
        assert_eq!(c.on_write(wire_pos(10), 10), CreditVerdict::Accept);
        assert_eq!(c.on_write(wire_pos(20), 10), CreditVerdict::Accept);
        assert_eq!(c.on_write(wire_pos(10), 10), CreditVerdict::Duplicate);
        // next gap re-armed
        assert!(matches!(
            c.on_write(wire_pos(50), 10),
            CreditVerdict::Gap(Some(_))
        ));
    }

    #[test]
    fn query() {
        let mut c = SinkCredit::new(100);
        c.take_credit();
        c.on_write(wire_pos(0), 10);
        assert_eq!(c.query(wire_pos(10)), credit(10, 90).unwrap());
        // lost at the end, nothing after it revealed the gap
        assert_eq!(c.query(wire_pos(30)), resend(10, 90));
        assert_eq!(c.on_write(wire_pos(20), 10), CreditVerdict::Gap(None));
        c.free(10);
        assert_eq!(c.query(wire_pos(10)), credit(10, 100).unwrap());
    }

    #[test]
    fn open_resets() {
        let mut c = SinkCredit::new(16);
        c.take_credit();
        c.on_write(wire_pos(0), 16);
        c.free(16);
        c.open();
        assert_eq!(c.have(), 0);
        assert_eq!(c.take_credit(), credit(0, 16));
    }

    #[test]
    fn empty_item_takes_a_position() {
        let mut c = SinkCredit::new(4);
        c.take_credit();
        assert_eq!(c.on_write(wire_pos(0), 4), CreditVerdict::Accept);
        // the buffer is full, but an empty item doesn't need room once it is in
        assert_eq!(c.on_write(wire_pos(4), 0), CreditVerdict::Accept);
        assert_eq!((c.have(), c.buffered(), c.limit()), (5, 4, 5));
        // a lost empty item is a gap like any other
        assert!(matches!(
            c.on_write(wire_pos(6), 1),
            CreditVerdict::Gap(Some(_))
        ));
        c.free(4);
        assert_eq!(c.buffered(), 0);
        assert_eq!(c.take_credit(), credit(5, 4));
    }

    #[test]
    fn sized_for_checks_largest_item() {
        assert_eq!(
            SinkCredit::try_sized_for(100, Some(101)).unwrap_err(),
            SinkCreditError::BufferTooSmall {
                buffer_bytes: 100,
                max_item_bytes: 101
            }
        );
        // unbounded items: nothing to check here, the host errors on an item bigger than the buffer
        assert_eq!(
            SinkCredit::try_sized_for(100, None).unwrap().threshold(),
            50
        );
        // a re-grant must make room for one more largest item: 100 - 80 + 1
        let c = SinkCredit::sized_for(100, Some(80));
        assert_eq!(c.threshold(), 21);
        assert_eq!(c.with_threshold(90).threshold(), 21);
        assert_eq!(SinkCredit::sized_for(100, Some(100)).threshold(), 1);
    }

    #[test]
    fn regrant_fits_largest_item() {
        // items of 1..=80 bytes into 100: the host stops when the next one doesn't fit, the device frees all it got
        let mut c = SinkCredit::sized_for(100, Some(80));
        c.take_credit();
        assert_eq!(c.on_write(wire_pos(0), 30), CreditVerdict::Accept);
        assert_eq!(c.on_write(wire_pos(30), 50), CreditVerdict::Accept);
        // next one has 80 bytes, 20 left
        c.free(80);
        // limit moved by 80 >= 21: the host learns it can send 100
        assert_eq!(c.take_credit(), credit(80, 100));
    }

    const M: u64 = POS_MODULUS as u64;

    #[test]
    fn extend_pos_takes_the_nearest() {
        let half = M / 2;
        for near in [
            0,
            1,
            1000,
            M - 1,
            M,
            M + 1,
            5 * M - 3,
            1 << 40,
            u64::MAX / 2,
        ] {
            for delta in [0i64, 1, -1, 100, -100, half as i64 - 1, -(half as i64)] {
                let Some(pos) = near.checked_add_signed(delta) else {
                    continue;
                };
                assert_eq!(extend_pos(near, wire_pos(pos)), Some(pos), "{near} {delta}");
            }
        }
        // half the modulus ahead is read as half behind
        assert_eq!(extend_pos(M, wire_pos(M + M / 2)), Some(M / 2));
        // before the start of the session
        assert_eq!(extend_pos(10, wire_pos(M - 5)), None);
        // no host sends these
        assert_eq!(extend_pos(10, POS_MODULUS), None);
        assert_eq!(extend_pos(10, u32::MAX), None);
        assert!(wire_pos(u64::MAX) < POS_MODULUS);
    }

    #[test]
    fn positions_wrap_on_the_wire() {
        // sessions far past the modulus (and the old 4 GiB limit), started there instead of sending that much
        for start in [M - 50, 2 * M - 1, 16 * M - 7, (1 << 40) - 25] {
            let mut c = SinkCredit::new(100).with_threshold(30);
            c.open_at(start);
            assert_eq!(c.take_credit(), credit(start, 100));
            let mut pos = start;
            for _ in 0..10 {
                assert_eq!(c.on_write(wire_pos(pos), 40), CreditVerdict::Accept);
                assert_eq!(c.on_write(wire_pos(pos), 40), CreditVerdict::Duplicate);
                // lost write, the next one is a gap
                assert_eq!(
                    c.on_write(wire_pos(pos + 80), 10),
                    CreditVerdict::Gap(Some(resend(pos + 40, 60)))
                );
                assert_eq!(c.query(wire_pos(pos + 80)), resend(pos + 40, 60));
                assert_eq!(c.on_write(wire_pos(pos + 40), 40), CreditVerdict::Accept);
                assert_eq!(c.query(wire_pos(pos + 80)), credit(pos + 80, 20).unwrap());
                c.free(80);
                assert_eq!(c.take_credit(), credit(pos + 80, 100));
                pos += 80;
            }
            assert_eq!(c.have(), start + 800);
            assert_eq!(c.overruns(), 0);
            assert_eq!(c.on_write(POS_MODULUS, 1), CreditVerdict::Invalid);
        }
    }

    #[test]
    fn late_write_from_a_previous_session() {
        let mut c = SinkCredit::new(100);
        c.take_credit();
        // the previous session ended just below the modulus
        assert_eq!(c.on_write(wire_pos(M - 20), 10), CreditVerdict::Duplicate);
        assert_eq!(c.on_write(wire_pos(0), 10), CreditVerdict::Accept);
    }

    #[test]
    fn window_is_bounded() {
        assert_eq!(MAX_WINDOW as u64 * 4, M);
        assert_eq!(SinkCredit::new(MAX_WINDOW).capacity(), MAX_WINDOW);
        assert_eq!(
            SinkCredit::try_sized_for(MAX_WINDOW + 1, Some(10)).unwrap_err(),
            SinkCreditError::BufferTooLarge {
                buffer_bytes: MAX_WINDOW + 1
            }
        );
        let mut c = SinkCredit::new(100);
        assert!(c.set_capacity(MAX_WINDOW + 1).is_err());
        assert_eq!(c.capacity(), 100);
        assert_eq!(c.set_capacity(MAX_WINDOW), Ok(()));
        // a full window takes a quarter of the modulus, also at the wrap
        c.open_at(M - 3);
        assert_eq!(c.take_credit(), credit(M - 3, MAX_WINDOW));
        let end = M - 3 + MAX_WINDOW as u64 - 1;
        assert!(matches!(
            c.on_write(wire_pos(end), 1),
            CreditVerdict::Gap(Some(_))
        ));
        assert_eq!(c.query(wire_pos(end)), resend(M - 3, MAX_WINDOW));
    }

    #[test]
    fn one_session_through_many_wraps() {
        // 2.5 GiB in 64 KiB items, every 7th write lost once and every 5th sent twice: positions only, no data
        const ITEM: u32 = 1 << 16;
        let mut c = SinkCredit::sized_for(1 << 20, Some(ITEM));
        c.take_credit();
        let mut host = 0u64;
        let mut n = 0u64;
        while host < 10 * M {
            n += 1;
            if n.is_multiple_of(7) {
                // lost, the next one reveals the gap and the host goes back
                let verdict = c.on_write(wire_pos(host + ITEM as u64), ITEM);
                assert_eq!(verdict, CreditVerdict::Gap(Some(resend(host, 1 << 20))));
            }
            assert_eq!(c.on_write(wire_pos(host), ITEM), CreditVerdict::Accept);
            if n.is_multiple_of(5) {
                assert_eq!(c.on_write(wire_pos(host), ITEM), CreditVerdict::Duplicate);
            }
            c.free(ITEM);
            host += ITEM as u64;
        }
        assert_eq!(c.have(), host);
        assert_eq!(c.overruns(), 0);
    }

    #[test]
    #[should_panic(expected = "MAX_WINDOW")]
    fn new_refuses_a_too_large_window() {
        SinkCredit::new(MAX_WINDOW + 1);
    }
}

#[cfg(test)]
mod wire_tests {
    use crate::{CreditWrite, StreamSideband};
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::tail_bytes::TailBytes;

    fn bytes<T: SerializeShrinkWrap>(value: &T) -> Vec<u8> {
        let mut buf = [0u8; 32];
        value.to_ww_bytes(&mut buf).unwrap().to_vec()
    }

    #[test]
    fn sideband_encoding() {
        // existing variants keep their discriminants
        assert_eq!(bytes(&StreamSideband::Open), [0x00]);
        assert_eq!(bytes(&StreamSideband::User(UNib32(1))), [0x41]);
        let credit = StreamSideband::Credit {
            have: UNib32(0),
            grant: UNib32(4096),
        };
        assert_eq!(bytes(&credit), [0x50, 0x88, 0x88, 0x10]);
        let credit = StreamSideband::Credit {
            have: UNib32(crate::credit::POS_MODULUS - 1),
            grant: UNib32(4096),
        };
        assert_eq!(
            bytes(&credit),
            [0x5f, 0xff, 0xff, 0xff, 0xff, 0x18, 0x88, 0x81]
        );
        assert_eq!(
            bytes(&StreamSideband::CreditQuery { sent: UNib32(300) }),
            [0x6c, 0xd4]
        );
        assert_eq!(
            bytes(&StreamSideband::Resend {
                have: UNib32(5),
                grant: UNib32(7)
            }),
            [0x75, 0x70]
        );
    }

    #[test]
    fn credit_write_encoding() {
        let w = CreditWrite {
            pos: UVlq32(5),
            data: TailBytes(&[1, 2, 3]),
        };
        assert_eq!(bytes(&w), [5, 1, 2, 3]);
        let w = CreditWrite {
            pos: UVlq32(200),
            data: TailBytes(&[9]),
        };
        assert_eq!(bytes(&w), [0x81, 0x48, 9]);
        let mut rd = BufReader::new(&[0x81, 0x48, 9]);
        let w = CreditWrite::des_shrink_wrap(&mut rd).unwrap();
        assert_eq!((w.pos.0, w.data.0), (200, &[9u8][..]));
        // the largest position on the wire takes 4 bytes
        let w = CreditWrite {
            pos: UVlq32(crate::credit::wire_pos(u64::MAX)),
            data: TailBytes(&[9]),
        };
        assert_eq!(bytes(&w), [0xff, 0xff, 0xff, 0x7f, 9]);
    }
}
