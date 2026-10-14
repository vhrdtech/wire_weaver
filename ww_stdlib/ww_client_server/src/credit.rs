//! Device side of credit-based flow control for `#[credit]` sinks, see `docs/api/streams.md`.
//!
//! [SinkCredit] is sans-IO: generated server code feeds it writes and sideband commands and sends the
//! [StreamSideband] events it returns, user code only sets the capacity and calls [SinkCredit::free].
//!
//! Credit counts elements (sink values, one per write), whatever their type. The device sizes its receive queue in
//! element slots: `slots = buffer bytes / worst-case element size`, see [SinkCredit::slots_for]. The generated
//! `stream_data_ser().<sink>_max_element_bytes()` gives the worst-case size where it is known.

use crate::StreamSideband;
use wire_weaver::shrink_wrap::UNib32;

/// Flow control state of one credit sink on the device.
///
/// All counters are element positions from the start of the session (`Open`): `have` elements were accepted in
/// order, `freed` of them were consumed by user code, so the device can take up to `freed + capacity`.
#[derive(Debug, Clone)]
pub struct SinkCredit {
    capacity: u32,
    threshold: u32,
    have: u32,
    freed: u32,
    /// Limit last told to the host, 0 until the first Credit after Open
    granted: u32,
    /// A Credit is sent on the next [SinkCredit::take_credit] regardless of the threshold (after Open)
    owed: bool,
    /// A write after a gap sends a Resend, once per gap
    resend_armed: bool,
    overruns: u32,
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
}

impl SinkCredit {
    /// Device can buffer `capacity` elements of this sink, re-granted every `capacity / 2` elements freed.
    pub const fn new(capacity: u32) -> Self {
        SinkCredit {
            capacity,
            threshold: Self::clamp_threshold(capacity / 2, capacity),
            have: 0,
            freed: 0,
            granted: 0,
            owed: true,
            resend_armed: true,
            overruns: 0,
        }
    }

    /// Send a new Credit once the limit moved by at least `threshold` elements (clamped to `1..=capacity`).
    /// A big threshold means few acks over a slow link, a small one keeps the link busy.
    pub const fn with_threshold(mut self, threshold: u32) -> Self {
        self.threshold = Self::clamp_threshold(threshold, self.capacity);
        self
    }

    const fn clamp_threshold(threshold: u32, capacity: u32) -> u32 {
        if threshold == 0 {
            1
        } else if threshold > capacity && capacity > 0 {
            capacity
        } else {
            threshold
        }
    }

    /// Change how many elements the device can buffer. Credit already granted is not taken back when it shrinks.
    pub fn set_capacity(&mut self, capacity: u32) {
        self.capacity = capacity;
        self.threshold = Self::clamp_threshold(self.threshold, capacity);
    }

    pub fn set_threshold(&mut self, threshold: u32) {
        self.threshold = Self::clamp_threshold(threshold, self.capacity);
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    /// Elements accepted in order since Open, also the position of the next expected write.
    pub fn have(&self) -> u32 {
        self.have
    }

    /// Elements freed by user code since Open.
    pub fn freed(&self) -> u32 {
        self.freed
    }

    /// Elements accepted but not freed yet.
    pub fn buffered(&self) -> u32 {
        self.have - self.freed
    }

    /// Elements the host sent beyond the credit it was granted, should stay 0.
    pub fn overruns(&self) -> u32 {
        self.overruns
    }

    /// Position up to which the host may send.
    pub fn limit(&self) -> u32 {
        self.freed.saturating_add(self.capacity)
    }

    fn grant(&self) -> u32 {
        self.limit().saturating_sub(self.have)
    }

    /// Start a new session (called by generated code on Open): counters reset, a Credit with the whole capacity is
    /// owed to the host.
    pub fn open(&mut self) {
        self.have = 0;
        self.freed = 0;
        self.granted = 0;
        self.owed = true;
        self.resend_armed = true;
    }

    /// The element at position `pos` arrived (called by generated code).
    pub fn on_write(&mut self, pos: u32) -> CreditVerdict {
        if pos == self.have {
            if pos >= self.granted.max(self.limit()) {
                self.overruns = self.overruns.saturating_add(1);
            }
            self.have = pos.saturating_add(1);
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

    /// User code consumed `count` elements and their slots can be given back to the host.
    /// Send the result of [Self::take_credit] afterwards when not in a write handler (generated code does it after
    /// each write).
    pub fn free(&mut self, count: u32) {
        self.freed = self.freed.saturating_add(count).min(self.have);
    }

    /// Number of element slots a receive buffer of `buffer_bytes` holds when an element takes at most
    /// `max_element_bytes`, e.g., `SinkCredit::new(SinkCredit::slots_for(4096, max))` with `max` from the generated
    /// `stream_data_ser().<sink>_max_element_bytes()`. Never 0.
    pub const fn slots_for(buffer_bytes: u32, max_element_bytes: u32) -> u32 {
        if max_element_bytes == 0 {
            return if buffer_bytes == 0 { 1 } else { buffer_bytes };
        }
        let slots = buffer_bytes / max_element_bytes;
        if slots == 0 { 1 } else { slots }
    }

    /// Credit event to send to the host, if one is owed or the limit moved by at least the threshold since the last
    /// one.
    pub fn take_credit(&mut self) -> Option<StreamSideband> {
        let limit = self.limit();
        if self.owed || limit >= self.granted.saturating_add(self.threshold) {
            Some(self.credit())
        } else {
            None
        }
    }

    /// Answer to `CreditQuery { sent }` from the host (called by generated code): `Resend` if writes before `sent`
    /// are missing, `Credit` otherwise.
    pub fn query(&mut self, sent: u32) -> StreamSideband {
        if sent > self.have {
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
            have: UNib32(self.have),
            grant: UNib32(self.grant()),
        }
    }

    fn resend(&mut self) -> StreamSideband {
        self.granted = self.granted.max(self.limit());
        self.owed = false;
        StreamSideband::Resend {
            have: UNib32(self.have),
            grant: UNib32(self.grant()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credit(have: u32, grant: u32) -> Option<StreamSideband> {
        Some(StreamSideband::Credit {
            have: UNib32(have),
            grant: UNib32(grant),
        })
    }

    fn resend(have: u32, grant: u32) -> StreamSideband {
        StreamSideband::Resend {
            have: UNib32(have),
            grant: UNib32(grant),
        }
    }

    #[test]
    fn initial_credit_and_threshold() {
        let mut c = SinkCredit::new(10).with_threshold(3);
        c.open();
        assert_eq!(c.take_credit(), credit(0, 10));
        assert_eq!(c.take_credit(), None);
        for pos in 0..8 {
            assert_eq!(c.on_write(pos), CreditVerdict::Accept);
        }
        assert_eq!(c.buffered(), 8);
        c.free(2);
        assert_eq!(c.take_credit(), None, "limit moved by 2 < 3");
        c.free(2);
        assert_eq!(c.take_credit(), credit(8, 6));
        assert_eq!(c.limit(), 14);
        assert_eq!(c.overruns(), 0);
    }

    #[test]
    fn overrun_counted() {
        let mut c = SinkCredit::new(2);
        assert_eq!(c.take_credit(), credit(0, 2));
        for pos in 0..5 {
            assert_eq!(c.on_write(pos), CreditVerdict::Accept);
        }
        assert_eq!(c.overruns(), 3);
    }

    #[test]
    fn gap_sends_resend_once() {
        let mut c = SinkCredit::new(10);
        c.take_credit();
        assert_eq!(c.on_write(0), CreditVerdict::Accept);
        // 1 lost
        assert_eq!(c.on_write(2), CreditVerdict::Gap(Some(resend(1, 9))));
        assert_eq!(c.on_write(3), CreditVerdict::Gap(None));
        // resend from 1
        assert_eq!(c.on_write(1), CreditVerdict::Accept);
        assert_eq!(c.on_write(2), CreditVerdict::Accept);
        assert_eq!(c.on_write(1), CreditVerdict::Duplicate);
        // next gap re-armed
        assert!(matches!(c.on_write(5), CreditVerdict::Gap(Some(_))));
    }

    #[test]
    fn query() {
        let mut c = SinkCredit::new(10);
        c.take_credit();
        c.on_write(0);
        assert_eq!(c.query(1), credit(1, 9).unwrap());
        // lost at the end, nothing after it revealed the gap
        assert_eq!(c.query(3), resend(1, 9));
        assert_eq!(c.on_write(2), CreditVerdict::Gap(None));
        c.free(1);
        assert_eq!(c.query(1), credit(1, 10).unwrap());
    }

    #[test]
    fn open_resets() {
        let mut c = SinkCredit::new(4);
        c.take_credit();
        for pos in 0..4 {
            c.on_write(pos);
        }
        c.free(4);
        c.open();
        assert_eq!(c.have(), 0);
        assert_eq!(c.take_credit(), credit(0, 4));
    }

    #[test]
    fn slots() {
        assert_eq!(SinkCredit::slots_for(4096, 100), 40);
        assert_eq!(SinkCredit::slots_for(50, 100), 1);
        assert_eq!(SinkCredit::slots_for(64, 0), 64);
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
    }
}
