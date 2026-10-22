//! Per-frame prefix: a few bytes [Tx](crate::Tx) writes once at the start of every frame and
//! [FramedRx](crate::FramedRx) reads once before the first head. Heads, checksums and tails never see it, except
//! that the checksum of a message covers the prefix of the frame the checksum is in (see
//! [Checksum](crate::traits::Checksum)), so a corrupted prefix fails the CRC of the messages after it.
//!
//! The framer keeps a `u32` frame counter and a session epoch ([FrameCtx]) and hands them to the prefix, which
//! decides what to write:
//!
//! - [NopPrefix]: nothing, the default. RTT, byte streams, USB.
//! - [SeqPrefix]: one byte, `00ee_ssss`: a 4-bit frame counter (mod 16) and a 2-bit session epoch, 2 spare bits
//!   (sent as 0, ignored on read). UDP: lost, duplicated, reordered and stale frames are told apart, see
//!   [FrameReport].
//!
//! ```
//! use ww_framer::{FramedRx, Tx};
//! use ww_framer::framed::U2Head;
//! use ww_framer::prefix::{FrameReport, SeqPrefix};
//! use ww_framer::traits::{NopChecksum, NopTail};
//!
//! let mut tx_buf = [0u8; 16];
//! let mut tx = Tx::<U2Head, NopChecksum, NopTail, SeqPrefix>::new(&mut tx_buf);
//! let mut rx_buf = [0u8; 64];
//! let mut rx = FramedRx::<U2Head, NopChecksum, NopTail, SeqPrefix>::new(&mut rx_buf);
//!
//! tx.write(0, &[1, 2, 3]).unwrap();
//! let len = tx.flush();
//! assert_eq!(&tx.buf()[..len], &[0x00, 0x03, 1, 2, 3]); // prefix: epoch 0, frame 0
//! let frame = tx.buf()[..len].to_vec();
//! assert!(matches!(rx.stage(&frame), Ok(FrameReport::Synced(_))));
//! rx.reassemble();
//! assert_eq!(rx.message(), Some((0, &[1, 2, 3][..])));
//!
//! // the same frame again is a duplicate and dropped
//! assert_eq!(rx.stage(&frame), Ok(FrameReport::Duplicate));
//! rx.reassemble();
//! assert_eq!(rx.message(), None);
//! ```

use shrink_wrap::BufWriter;

use crate::traits::WrError;

/// Longest prefix the framer supports. [Tx](crate::Tx) and [FramedRx](crate::FramedRx) keep a copy of the current
/// frame's prefix of this size, for the checksum.
pub const MAX_PREFIX_LEN: usize = 4;

/// Where a frame is in its sender's sequence: [Tx](crate::Tx) counts frames from 0 in every session (`seq`) and
/// moves `epoch` on by one with [Tx::new_session](crate::Tx::new_session). A prefix writes as many low bits of
/// both as it has room for.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct FrameCtx {
    pub seq: u32,
    pub epoch: u8,
}

/// What [FramedRx::stage](crate::FramedRx::stage) found in a frame's prefix.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FrameReport {
    /// No prefix ([NopPrefix]), nothing to check.
    Unchecked,
    /// First frame after the receiver was created, reset or resynced: taken as the reference, decoded.
    Synced(FrameCtx),
    /// The frame that was expected.
    InOrder,
    /// `lost` frames are missing before this one: a message being assembled is dropped, this frame is decoded.
    Gap { lost: u32 },
    /// The sender started a new session (its epoch moved on): taken as the new reference, a message being assembled
    /// is dropped, this frame is decoded.
    NewEpoch(FrameCtx),
    /// The second of two frames in a row that look received before and follow each other: more frames were lost
    /// than the counter can tell (8 or more with [SeqPrefix]). Taken as the new reference, but dropped, as it could
    /// still be a replayed one.
    Resynced(FrameCtx),
    /// A frame received before: dropped.
    Duplicate,
    /// A frame from before the expected one, not received before (reordered): dropped. It was already counted as
    /// lost by the [FrameReport::Gap] that skipped it.
    Late,
    /// A frame from the sender's previous session: dropped.
    StaleEpoch,
    /// The frame is shorter than its prefix: dropped.
    TooShort,
}

impl FrameReport {
    /// The frame is not decoded.
    pub fn is_dropped(self) -> bool {
        matches!(
            self,
            FrameReport::Duplicate
                | FrameReport::Late
                | FrameReport::StaleEpoch
                | FrameReport::TooShort
                | FrameReport::Resynced(_)
        )
    }

    /// A split message being assembled can't continue in this frame.
    pub fn breaks_message(self) -> bool {
        matches!(
            self,
            FrameReport::Synced(_) | FrameReport::Gap { .. } | FrameReport::NewEpoch(_)
        )
    }
}

/// Counters of what [FramedRx](crate::FramedRx) found in frame prefixes since it was created, all zero with
/// [NopPrefix]. Wrap around at `u32::MAX`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct PrefixStats {
    /// Frames decoded (everything but the dropped ones).
    pub frames: u32,
    /// Frames missing, summed over every [FrameReport::Gap]; late frames counted here as well.
    pub lost: u32,
    /// [FrameReport::Duplicate] frames.
    pub duplicate: u32,
    /// [FrameReport::Late] frames.
    pub late: u32,
    /// [FrameReport::StaleEpoch] frames.
    pub stale_epoch: u32,
    /// [FrameReport::NewEpoch] frames.
    pub new_epoch: u32,
    /// [FrameReport::Synced] and [FrameReport::Resynced] frames.
    pub resynced: u32,
    /// [FrameReport::TooShort] frames.
    pub too_short: u32,
}

/// A per-frame prefix, see the [module docs](self).
pub trait FramePrefix {
    /// Bytes written at the start of every frame, at most [MAX_PREFIX_LEN].
    const LEN: usize;
    /// What the receiver keeps between frames: the expected position and the counters. `()` when there is nothing
    /// to check, so a framer without a prefix carries nothing for it.
    type Rx: Copy + Default + Send;

    /// Write exactly [Self::LEN] bytes for a frame at `frame`.
    fn write(frame: FrameCtx, wr: &mut BufWriter<'_>) -> Result<(), WrError>;
    /// Check a received prefix (`bytes` is exactly [Self::LEN] long) against what `rx` expects, and move `rx` on.
    fn read(rx: &mut Self::Rx, bytes: &[u8]) -> FrameReport;
    /// Take the next frame as the reference, whatever its position (counters are kept).
    fn resync(rx: &mut Self::Rx);
    /// Counters so far.
    fn stats(rx: &Self::Rx) -> PrefixStats;
    /// The frame expected next, once a frame was received; None without a counter. With [Self::continue_at], a
    /// receiver in another process goes on where this one stopped (a host taking over a medium mid-stream, CLT-44).
    fn expected(_rx: &Self::Rx) -> Option<FrameCtx> {
        None
    }
    /// Take `next` as the frame expected next, as if every frame before it was received. No-op without a counter.
    fn continue_at(_rx: &mut Self::Rx, _next: FrameCtx) {}
}

/// No prefix: frames start with the first head.
pub struct NopPrefix;

impl FramePrefix for NopPrefix {
    const LEN: usize = 0;
    type Rx = ();

    #[inline(always)]
    fn write(_frame: FrameCtx, _wr: &mut BufWriter<'_>) -> Result<(), WrError> {
        Ok(())
    }

    #[inline(always)]
    fn read(_rx: &mut (), _bytes: &[u8]) -> FrameReport {
        FrameReport::Unchecked
    }

    #[inline(always)]
    fn resync(_rx: &mut ()) {}

    fn stats(_rx: &()) -> PrefixStats {
        PrefixStats::default()
    }
}

/// One byte `00ee_ssss`: frame counter mod 16 (`s`), session epoch mod 4 (`e`), 2 spare bits (sent as 0, ignored
/// on read). See [SeqRx] for how the receiver tells frames apart.
pub struct SeqPrefix;

impl SeqPrefix {
    const SEQ_BITS: u32 = 4;
    const EPOCH_BITS: u32 = 2;

    /// The prefix byte of a frame at `frame`.
    pub const fn encode(frame: FrameCtx) -> u8 {
        ((frame.epoch & 0b11) << 4) | (frame.seq as u8 & 0x0F)
    }

    /// Low bits of `(seq, epoch)` as they are in a prefix byte, e.g. for a tap or a monitor.
    pub const fn decode(byte: u8) -> (u8, u8) {
        (byte & 0x0F, (byte >> 4) & 0b11)
    }
}

impl FramePrefix for SeqPrefix {
    const LEN: usize = 1;
    type Rx = SeqRx;

    #[inline]
    fn write(frame: FrameCtx, wr: &mut BufWriter<'_>) -> Result<(), WrError> {
        wr.write_u8(Self::encode(frame))?;
        Ok(())
    }

    #[inline]
    fn read(rx: &mut SeqRx, bytes: &[u8]) -> FrameReport {
        let (seq, epoch) = Self::decode(bytes[0]);
        rx.check::<{ Self::SEQ_BITS }, { Self::EPOCH_BITS }>(seq as u32, epoch)
    }

    fn resync(rx: &mut SeqRx) {
        rx.synced = false;
    }

    fn stats(rx: &SeqRx) -> PrefixStats {
        rx.stats
    }

    fn expected(rx: &SeqRx) -> Option<FrameCtx> {
        rx.synced.then(|| rx.expected())
    }

    fn continue_at(rx: &mut SeqRx, next: FrameCtx) {
        rx.synced = true;
        rx.next = next.seq;
        rx.epoch = next.epoch;
        rx.seen = u16::MAX;
        rx.dup_back = 0;
    }
}

/// Receiver side of a counter prefix such as [SeqPrefix]: rebuilds the sender's full `u32` frame counter and epoch
/// from their low bits and classifies each frame ([FrameReport]).
///
/// - **Counter.** The low bits are read as the nearest value to the expected one: up to half the counter range
///   ahead is a [FrameReport::Gap] (7 frames with 4 bits), behind it a [FrameReport::Duplicate] (received before,
///   from a bitmap of the last 16 frames) or [FrameReport::Late] one; both are dropped, so messages never come out
///   twice or out of order. Losing 8 or more frames in a row makes the next ones look behind: two of them in a row
///   that look received before and follow each other set the new reference ([FrameReport::Resynced]), so such a
///   burst loss costs two frames more than it took; when they look not received (more was lost before), up to 8
///   more frames are dropped until the counter comes round.
/// - **Epoch.** The sender moves its epoch on by one per session. One or two ahead is a new session
///   ([FrameReport::NewEpoch]): two, because a sender may start more than one session before the receiver gets a
///   frame of it (link setup retried). One behind is the previous session: [FrameReport::StaleEpoch]. So a frame
///   delayed across two session changes is taken as new: sessions (a link setup each) last much longer than
///   frames are reordered.
/// - **Sync.** After [FramePrefix::resync] (and at first) the next frame is taken as it is.
#[derive(Copy, Clone, Debug, Default)]
pub struct SeqRx {
    synced: bool,
    /// Counter of the frame expected next.
    next: u32,
    epoch: u8,
    /// Bit `i` set: frame `next - 1 - i` was received.
    seen: u16,
    /// The frame right before was a duplicate this far behind, 0 otherwise.
    dup_back: u32,
    stats: PrefixStats,
}

impl SeqRx {
    /// The frame expected next, valid once a frame was received.
    pub fn expected(&self) -> FrameCtx {
        FrameCtx {
            seq: self.next,
            epoch: self.epoch,
        }
    }

    /// Check a frame whose counter's low `SEQ_BITS` are `seq` and epoch's low `EPOCH_BITS` are `epoch`.
    pub fn check<const SEQ_BITS: u32, const EPOCH_BITS: u32>(
        &mut self,
        seq: u32,
        epoch: u8,
    ) -> FrameReport {
        let seq_mask = (1u32 << SEQ_BITS) - 1;
        let epoch_mask = (1u8 << EPOCH_BITS) - 1;
        if !self.synced {
            let frame = FrameCtx { seq, epoch };
            self.adopt(frame);
            self.stats.frames = self.stats.frames.wrapping_add(1);
            self.stats.resynced = self.stats.resynced.wrapping_add(1);
            return FrameReport::Synced(frame);
        }
        let epoch_ahead = epoch.wrapping_sub(self.epoch) & epoch_mask;
        if epoch_ahead != 0 {
            // one behind (all ones) is the previous session; anything else ahead
            if epoch_ahead == epoch_mask && epoch_mask > 1 {
                self.stats.stale_epoch = self.stats.stale_epoch.wrapping_add(1);
                return FrameReport::StaleEpoch;
            }
            let frame = FrameCtx {
                seq,
                epoch: self.epoch.wrapping_add(epoch_ahead),
            };
            self.adopt(frame);
            self.stats.frames = self.stats.frames.wrapping_add(1);
            self.stats.new_epoch = self.stats.new_epoch.wrapping_add(1);
            return FrameReport::NewEpoch(frame);
        }
        let ahead = seq.wrapping_sub(self.next) & seq_mask;
        let half = 1u32 << (SEQ_BITS - 1);
        if ahead < half {
            self.dup_back = 0;
            self.next = self.next.wrapping_add(ahead + 1);
            // ahead + 1 bits shift in, the newest set (frames in a gap were not seen)
            self.seen = self.seen.checked_shl(ahead + 1).unwrap_or(0) | 1;
            self.stats.frames = self.stats.frames.wrapping_add(1);
            if ahead == 0 {
                return FrameReport::InOrder;
            }
            self.stats.lost = self.stats.lost.wrapping_add(ahead);
            return FrameReport::Gap { lost: ahead };
        }
        // behind by 1..=half
        let back = (1u32 << SEQ_BITS) - ahead;
        let bit = 1u16 << (back - 1);
        if self.seen & bit == 0 {
            self.dup_back = 0;
            self.seen |= bit;
            self.stats.late = self.stats.late.wrapping_add(1);
            return FrameReport::Late;
        }
        if self.dup_back == back + 1 {
            // two "duplicates" that follow each other: frames after a burst loss
            let frame = FrameCtx {
                seq: self.next.wrapping_sub(back),
                epoch: self.epoch,
            };
            self.adopt(frame);
            self.stats.resynced = self.stats.resynced.wrapping_add(1);
            return FrameReport::Resynced(frame);
        }
        self.dup_back = back;
        self.stats.duplicate = self.stats.duplicate.wrapping_add(1);
        FrameReport::Duplicate
    }

    fn adopt(&mut self, frame: FrameCtx) {
        self.synced = true;
        self.next = frame.seq.wrapping_add(1);
        self.epoch = frame.epoch;
        self.seen = 1;
        self.dup_back = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rx_at(seq: u32, epoch: u8) -> SeqRx {
        let mut rx = SeqRx::default();
        assert!(matches!(
            rx.check::<4, 2>(seq & 15, epoch & 3),
            FrameReport::Synced(_)
        ));
        rx
    }

    fn check(rx: &mut SeqRx, seq: u32, epoch: u8) -> FrameReport {
        rx.check::<4, 2>(seq & 15, epoch & 3)
    }

    #[test]
    fn encode_decode() {
        assert_eq!(SeqPrefix::encode(FrameCtx { seq: 0, epoch: 0 }), 0x00);
        assert_eq!(
            SeqPrefix::encode(FrameCtx { seq: 17, epoch: 5 }),
            0b0001_0001
        );
        assert_eq!(
            SeqPrefix::encode(FrameCtx { seq: 15, epoch: 3 }),
            0b0011_1111
        );
        assert_eq!(SeqPrefix::decode(0b0011_1111), (15, 3));
        // spare bits ignored
        assert_eq!(SeqPrefix::decode(0b1110_0101), (5, 2));
    }

    #[test]
    fn in_order_and_wrap() {
        let mut rx = rx_at(0, 0);
        for seq in 1..100 {
            assert_eq!(check(&mut rx, seq, 0), FrameReport::InOrder, "seq {seq}");
        }
        assert_eq!(rx.expected(), FrameCtx { seq: 100, epoch: 0 });
        assert_eq!(rx.stats.frames, 100);
        // the reference can be anywhere
        let mut rx = rx_at(13, 2);
        assert_eq!(rx.expected(), FrameCtx { seq: 14, epoch: 2 });
        assert_eq!(check(&mut rx, 14, 2), FrameReport::InOrder);
        assert_eq!(check(&mut rx, 15, 2), FrameReport::InOrder);
        assert_eq!(check(&mut rx, 16, 2), FrameReport::InOrder);
        assert_eq!(rx.expected().seq, 17);
    }

    #[test]
    fn gaps() {
        let mut rx = rx_at(0, 0);
        assert_eq!(check(&mut rx, 2, 0), FrameReport::Gap { lost: 1 });
        assert_eq!(check(&mut rx, 10, 0), FrameReport::Gap { lost: 7 });
        assert_eq!(rx.expected().seq, 11);
        // across the wrap
        assert_eq!(check(&mut rx, 17, 0), FrameReport::Gap { lost: 6 });
        assert_eq!(rx.expected().seq, 18);
        assert_eq!(rx.stats.lost, 14);
    }

    #[test]
    fn duplicates_and_late_frames() {
        let mut rx = rx_at(0, 0);
        assert_eq!(check(&mut rx, 1, 0), FrameReport::InOrder);
        assert_eq!(check(&mut rx, 1, 0), FrameReport::Duplicate);
        assert_eq!(check(&mut rx, 2, 0), FrameReport::InOrder);
        // 3 and 4 swapped
        assert_eq!(check(&mut rx, 4, 0), FrameReport::Gap { lost: 1 });
        assert_eq!(check(&mut rx, 3, 0), FrameReport::Late);
        assert_eq!(check(&mut rx, 5, 0), FrameReport::InOrder);
        // the late one again is a duplicate now
        assert_eq!(check(&mut rx, 3, 0), FrameReport::Duplicate);
        assert_eq!(check(&mut rx, 6, 0), FrameReport::InOrder);
        assert_eq!(rx.expected().seq, 7);
        assert_eq!(
            (rx.stats.duplicate, rx.stats.late, rx.stats.lost),
            (2, 1, 1)
        );
    }

    #[test]
    fn burst_loss_resyncs_on_the_second_frame_from_behind() {
        let mut rx = rx_at(0, 0);
        for seq in 1..=8 {
            assert_eq!(check(&mut rx, seq, 0), FrameReport::InOrder);
        }
        // 9..=18 lost (10 frames): 19 reads as 6 behind (19 ≡ 3, expected 9), frame 3 was received
        assert_eq!(check(&mut rx, 19, 0), FrameReport::Duplicate);
        assert_eq!(
            check(&mut rx, 20, 0),
            FrameReport::Resynced(FrameCtx { seq: 4, epoch: 0 })
        );
        assert_eq!(check(&mut rx, 21, 0), FrameReport::InOrder);
        assert_eq!(rx.expected().seq & 15, 22 & 15);
        assert_eq!(rx.stats.resynced, 2);
    }

    #[test]
    fn repeated_duplicates_and_reordering_never_resync() {
        let mut rx = rx_at(0, 0);
        for seq in 1..=5 {
            check(&mut rx, seq, 0);
        }
        // the same frame over and over
        for _ in 0..4 {
            assert_eq!(check(&mut rx, 3, 0), FrameReport::Duplicate);
        }
        // two old frames out of order
        assert_eq!(check(&mut rx, 2, 0), FrameReport::Duplicate);
        assert_eq!(check(&mut rx, 4, 0), FrameReport::Duplicate);
        // reordered by several frames: late, never a resync
        assert_eq!(check(&mut rx, 9, 0), FrameReport::Gap { lost: 3 });
        assert_eq!(check(&mut rx, 6, 0), FrameReport::Late);
        assert_eq!(check(&mut rx, 7, 0), FrameReport::Late);
        assert_eq!(check(&mut rx, 8, 0), FrameReport::Late);
        assert_eq!(check(&mut rx, 10, 0), FrameReport::InOrder);
        assert_eq!(rx.stats.resynced, 1);
    }

    #[test]
    fn epochs() {
        let mut rx = rx_at(5, 0);
        assert_eq!(check(&mut rx, 6, 0), FrameReport::InOrder);
        // new session: counter starts over
        assert_eq!(
            check(&mut rx, 0, 1),
            FrameReport::NewEpoch(FrameCtx { seq: 0, epoch: 1 })
        );
        assert_eq!(check(&mut rx, 1, 1), FrameReport::InOrder);
        // a frame of the previous session arriving late
        assert_eq!(check(&mut rx, 7, 0), FrameReport::StaleEpoch);
        assert_eq!(check(&mut rx, 2, 1), FrameReport::InOrder);
        // two sessions on (link setup retried, the first reply lost)
        assert_eq!(
            check(&mut rx, 0, 3),
            FrameReport::NewEpoch(FrameCtx { seq: 0, epoch: 3 })
        );
        // and across the wrap of the 2 bits
        assert!(matches!(check(&mut rx, 0, 0), FrameReport::NewEpoch(_)));
        assert_eq!(check(&mut rx, 1, 0), FrameReport::InOrder);
        assert_eq!(check(&mut rx, 3, 3), FrameReport::StaleEpoch);
        assert_eq!((rx.stats.new_epoch, rx.stats.stale_epoch), (3, 2));
    }

    #[test]
    fn resync_takes_anything() {
        let mut rx = rx_at(5, 0);
        SeqPrefix::resync(&mut rx);
        assert_eq!(
            check(&mut rx, 9, 3),
            FrameReport::Synced(FrameCtx { seq: 9, epoch: 3 })
        );
        assert_eq!(check(&mut rx, 10, 3), FrameReport::InOrder);
    }

    #[test]
    fn report_flags() {
        assert!(FrameReport::Duplicate.is_dropped());
        assert!(FrameReport::Late.is_dropped());
        assert!(FrameReport::StaleEpoch.is_dropped());
        assert!(FrameReport::TooShort.is_dropped());
        assert!(!FrameReport::Gap { lost: 1 }.is_dropped());
        assert!(FrameReport::Gap { lost: 1 }.breaks_message());
        assert!(!FrameReport::InOrder.breaks_message());
        assert!(!FrameReport::Unchecked.breaks_message());
    }
}
