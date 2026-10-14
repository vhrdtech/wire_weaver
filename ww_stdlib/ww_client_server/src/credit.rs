//! Device side of credit-based flow control for `#[credit]` sinks, see `docs/api/streams.md`.
//!
//! [SinkCredit] is sans-IO: generated server code feeds it writes and sideband commands and sends the
//! [StreamSideband] events it returns, user code only sets the capacity and calls [SinkCredit::free].

use crate::StreamSideband;
use wire_weaver::shrink_wrap::UNib32;

/// Flow control state of one credit sink on the device.
///
/// All counters are byte positions from the start of the session (`Open`): `have` bytes were accepted in order,
/// `freed` of them were consumed by user code, so the device can take up to `freed + capacity`.
#[derive(Debug, Clone)]
pub struct SinkCredit {
    capacity: u32,
    threshold: u32,
    have: u32,
    freed: u32,
    /// Limit last told to the host
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
    /// Device can buffer `capacity` bytes of this sink, re-granted every `capacity / 2` bytes freed.
    pub const fn new(capacity: u32) -> Self {
        SinkCredit {
            capacity,
            threshold: Self::clamp_threshold(capacity / 2, capacity),
            have: 0,
            freed: 0,
            granted: capacity,
            owed: true,
            resend_armed: true,
            overruns: 0,
        }
    }

    /// Send a new Credit once the limit moved by at least `threshold` bytes (clamped to `1..=capacity`).
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

    /// Change how much the device can buffer. Credit already granted is not taken back when it shrinks.
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

    /// Bytes accepted in order since Open, also the position of the next expected write.
    pub fn have(&self) -> u32 {
        self.have
    }

    /// Bytes freed by user code since Open.
    pub fn freed(&self) -> u32 {
        self.freed
    }

    /// Bytes accepted but not freed yet.
    pub fn buffered(&self) -> u32 {
        self.have - self.freed
    }

    /// Bytes the host sent beyond the credit it was granted, should stay 0.
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
        self.granted = self.capacity;
        self.owed = true;
        self.resend_armed = true;
    }

    /// A write of `len` bytes at position `pos` arrived (called by generated code).
    pub fn on_write(&mut self, pos: u32, len: u32) -> CreditVerdict {
        if pos == self.have {
            let end = pos.saturating_add(len);
            let allowed = self.granted.max(self.limit());
            if end > allowed {
                self.overruns = self
                    .overruns
                    .saturating_add(end - allowed.max(pos).min(end));
            }
            self.have = end;
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

    /// User code consumed `len` bytes and the space can be given back to the host.
    /// Send the result of [Self::take_credit] afterwards when not in a write handler (generated code does it after
    /// each write).
    pub fn free(&mut self, len: u32) {
        self.freed = self.freed.saturating_add(len).min(self.have);
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
        let mut c = SinkCredit::new(100).with_threshold(30);
        c.open();
        assert_eq!(c.take_credit(), credit(0, 100));
        assert_eq!(c.take_credit(), None);
        assert_eq!(c.on_write(0, 40), CreditVerdict::Accept);
        assert_eq!(c.on_write(40, 40), CreditVerdict::Accept);
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
        assert_eq!(c.on_write(0, 8), CreditVerdict::Accept);
        assert_eq!(c.on_write(8, 8), CreditVerdict::Accept);
        assert_eq!(c.overruns(), 6);
    }

    #[test]
    fn gap_sends_resend_once() {
        let mut c = SinkCredit::new(100);
        c.take_credit();
        assert_eq!(c.on_write(0, 10), CreditVerdict::Accept);
        // 10..20 lost
        assert_eq!(
            c.on_write(20, 10),
            CreditVerdict::Gap(Some(resend(10, 90)))
        );
        assert_eq!(c.on_write(30, 10), CreditVerdict::Gap(None));
        // resend from 10
        assert_eq!(c.on_write(10, 10), CreditVerdict::Accept);
        assert_eq!(c.on_write(20, 10), CreditVerdict::Accept);
        assert_eq!(c.on_write(10, 10), CreditVerdict::Duplicate);
        // next gap re-armed
        assert!(matches!(c.on_write(50, 10), CreditVerdict::Gap(Some(_))));
    }

    #[test]
    fn query() {
        let mut c = SinkCredit::new(100);
        c.take_credit();
        c.on_write(0, 10);
        assert_eq!(c.query(10), credit(10, 90).unwrap());
        // lost at the end, nothing after it revealed the gap
        assert_eq!(c.query(30), resend(10, 90));
        assert_eq!(c.on_write(20, 10), CreditVerdict::Gap(None));
        c.free(10);
        assert_eq!(c.query(10), credit(10, 100).unwrap());
    }

    #[test]
    fn open_resets() {
        let mut c = SinkCredit::new(16);
        c.take_credit();
        c.on_write(0, 16);
        c.free(16);
        c.open();
        assert_eq!(c.have(), 0);
        assert_eq!(c.take_credit(), credit(0, 16));
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
