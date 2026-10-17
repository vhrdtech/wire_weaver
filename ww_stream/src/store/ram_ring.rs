//! [RamRing]: a byte ring in RAM, the default store of reliable streams.
//!
//! The ring itself ([Ring]) is shared with [PersistentRamRing](super::PersistentRamRing), which keeps its bytes in RAM
//! that survives a soft reset and adds a CRC to every record.

use super::{FullPolicy, PushError, StoreState, StoredEvent, StreamStore};
use crate::StreamHead;
use core::marker::PhantomData;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, SerializeShrinkWrap};

/// Bytes of a record's own header: total length (u32), `pos` and `end` (u64 each), head length (u8).
pub const RECORD_OVERHEAD: usize = 4 + 8 + 8 + 1;
/// Longest encoded head a record takes.
pub(super) const MAX_HEAD: usize = 64;
/// Longest check value a record header carries after [RECORD_OVERHEAD].
const MAX_CHECK: usize = 4;

/// Events of one reliable stream in a byte ring of `N` bytes in RAM. Survives host reconnects, not reboots.
///
/// Each event takes its item, its encoded head and [RECORD_OVERHEAD] bytes. Records wrap around the end of the
/// buffer, so all `N` bytes are usable. `RamRing::new()` is `const`, so a big ring can live in a `static` (or a
/// `StaticCell`) instead of on the stack.
pub struct RamRing<const N: usize> {
    buf: [u8; N],
    ring: Ring<NoCheck>,
}

/// Errors of a [RamRing] (and a [PersistentRamRing](super::PersistentRamRing)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RamRingError {
    /// [StreamStore::read] got a buffer smaller than the item.
    BufferTooSmall { item_len: usize },
    /// The head couldn't be encoded (a position of 2⁵⁷ or more) or is longer than a record allows.
    Head(wire_weaver::shrink_wrap::Error),
}

/// A check value kept in each record's header, after the first [RECORD_OVERHEAD] bytes.
pub(super) trait Check {
    /// Bytes of the check value, at most [MAX_CHECK].
    const LEN: usize;
    /// Check value over these parts, in order.
    fn sum(parts: &[&[u8]]) -> u32;
}

/// No check value: a [RamRing] lives only as long as the firmware runs.
pub(super) struct NoCheck;

impl Check for NoCheck {
    const LEN: usize = 0;
    fn sum(_parts: &[&[u8]]) -> u32 {
        0
    }
}

pub(super) struct Record {
    pub(super) len: usize,
    pub(super) pos: u64,
    pub(super) end: u64,
    pub(super) head_len: usize,
    #[cfg_attr(not(feature = "persistent-ram"), allow(dead_code))]
    pub(super) check: u32,
}

/// Ring state over a byte buffer the owner keeps (`buf.len()` bytes), records with a `C` check value.
pub(super) struct Ring<C: Check> {
    pub(super) policy: FullPolicy,
    pub(super) session: u32,
    /// Offset of the oldest record
    pub(super) start: usize,
    pub(super) used: usize,
    pub(super) oldest: Option<u64>,
    pub(super) newest: Option<u64>,
    /// Offset of the newest record
    pub(super) newest_off: usize,
    /// Where the newest record ends
    pub(super) newest_end: u64,
    pub(super) next: u64,
    pub(super) dropped_units: u64,
    /// Offset and position of a record at or after the last one read, so sequential reads don't scan from the start
    hint: Option<(usize, u64)>,
    check: PhantomData<C>,
}

impl<const N: usize> Default for RamRing<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> RamRing<N> {
    /// Empty ring, session 0, drops the newest event when full.
    pub const fn new() -> Self {
        RamRing {
            buf: [0; N],
            ring: Ring::new(),
        }
    }

    /// What to do when an event doesn't fit, see [FullPolicy].
    pub const fn with_policy(mut self, policy: FullPolicy) -> Self {
        self.ring.policy = policy;
        self
    }

    /// Events kept.
    pub fn is_empty(&self) -> bool {
        self.ring.used == 0
    }
}

/// Copy `src` into `dst`. In tests, a write budget makes a fault stop the copy at any byte.
pub(super) fn write(dst: &mut [u8], src: &[u8]) {
    #[cfg(test)]
    let src = &src[..fault::take(src.len())];
    dst[..src.len()].copy_from_slice(src);
}

impl<C: Check> Ring<C> {
    /// Empty, session 0, drops the newest event when full.
    pub(super) const fn new() -> Self {
        Ring {
            policy: FullPolicy::DropNewest,
            session: 0,
            start: 0,
            used: 0,
            oldest: None,
            newest: None,
            newest_off: 0,
            newest_end: 0,
            next: 0,
            dropped_units: 0,
            hint: None,
            check: PhantomData,
        }
    }

    /// Bytes of a record's header, check value included.
    pub(super) const fn overhead() -> usize {
        RECORD_OVERHEAD + C::LEN
    }

    /// The longest item a ring of `n` bytes takes, with the longest head.
    pub(super) const fn max_item_len(n: usize) -> usize {
        n.saturating_sub(Self::overhead() + MAX_HEAD)
    }

    fn wrap(n: usize, off: usize) -> usize {
        if off >= n { off - n } else { off }
    }

    fn copy_in(buf: &mut [u8], off: usize, bytes: &[u8]) {
        let first = bytes.len().min(buf.len() - off);
        write(&mut buf[off..], &bytes[..first]);
        write(buf, &bytes[first..]);
    }

    pub(super) fn copy_out(buf: &[u8], off: usize, out: &mut [u8]) {
        let first = out.len().min(buf.len() - off);
        out[..first].copy_from_slice(&buf[off..off + first]);
        let rest = out.len() - first;
        out[first..].copy_from_slice(&buf[..rest]);
    }

    /// The `len` bytes at `off`, in one or two pieces (when they wrap around the end).
    #[cfg_attr(not(feature = "persistent-ram"), allow(dead_code))]
    pub(super) fn region(buf: &[u8], off: usize, len: usize) -> (&[u8], &[u8]) {
        let first = len.min(buf.len() - off);
        (&buf[off..off + first], &buf[..len - first])
    }

    /// Record header at `off`, its first [RECORD_OVERHEAD] bytes in `raw`.
    pub(super) fn record_raw(buf: &[u8], off: usize) -> (Record, [u8; RECORD_OVERHEAD]) {
        let mut header = [0u8; RECORD_OVERHEAD + MAX_CHECK];
        Self::copy_out(buf, off, &mut header[..Self::overhead()]);
        let u64_at = |i: usize| u64::from_le_bytes(header[i..i + 8].try_into().unwrap());
        let mut check = [0u8; 4];
        check[..C::LEN].copy_from_slice(&header[RECORD_OVERHEAD..Self::overhead()]);
        let rec = Record {
            len: u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize,
            pos: u64_at(4),
            end: u64_at(12),
            head_len: header[20] as usize,
            check: u32::from_le_bytes(check),
        };
        (rec, header[..RECORD_OVERHEAD].try_into().unwrap())
    }

    fn record(buf: &[u8], off: usize) -> Record {
        Self::record_raw(buf, off).0
    }

    /// Forget the oldest record; its units count as dropped when `dropped`.
    fn pop_oldest(&mut self, buf: &[u8], dropped: bool) {
        let rec = Self::record(buf, self.start);
        if dropped {
            self.dropped_units += rec.end - rec.pos;
        }
        self.start = Self::wrap(buf.len(), self.start + rec.len);
        self.used -= rec.len;
        if self.used == 0 {
            self.oldest = None;
            self.newest = None;
        } else {
            self.oldest = Some(Self::record(buf, self.start).pos);
        }
        if let Some((_, pos)) = self.hint
            && pos <= rec.pos
        {
            self.hint = None;
        }
    }

    /// Append an event. `on_room` is called after old events were dropped to make room and before the new one is
    /// written over them; the new one is in `self` when this returns `Ok`.
    pub(super) fn push(
        &mut self,
        buf: &mut [u8],
        head: &StreamHead,
        item: &[u8],
        mut on_room: impl FnMut(&Self, &[u8]),
    ) -> Result<(), PushError<RamRingError>> {
        let n = buf.len();
        let mut head_buf = [0u8; MAX_HEAD];
        let head_bytes = head
            .to_ww_bytes(&mut head_buf)
            .map_err(|e| PushError::Store(RamRingError::Head(e)))?;
        let head_len = head_bytes.len();
        let len = Self::overhead() + head_len + item.len();
        if len > n || len > u32::MAX as usize {
            return Err(PushError::TooBig);
        }
        let mut dropped = false;
        while n - self.used < len {
            match self.policy {
                FullPolicy::DropNewest => return Err(PushError::Full),
                FullPolicy::DropOldest => {
                    self.pop_oldest(buf, true);
                    dropped = true;
                }
            }
        }
        if dropped {
            on_room(self, buf);
        }
        let mut header = [0u8; RECORD_OVERHEAD + MAX_CHECK];
        header[0..4].copy_from_slice(&(len as u32).to_le_bytes());
        header[4..12].copy_from_slice(&head.pos.to_le_bytes());
        header[12..20].copy_from_slice(&head.end().to_le_bytes());
        header[20] = head_len as u8;
        let check = C::sum(&[&header[..RECORD_OVERHEAD], &head_buf[..head_len], item]);
        header[RECORD_OVERHEAD..RECORD_OVERHEAD + MAX_CHECK].copy_from_slice(&check.to_le_bytes());
        let off = Self::wrap(n, self.start + self.used);
        Self::copy_in(buf, off, &header[..Self::overhead()]);
        Self::copy_in(
            buf,
            Self::wrap(n, off + Self::overhead()),
            &head_buf[..head_len],
        );
        Self::copy_in(buf, Self::wrap(n, off + Self::overhead() + head_len), item);
        self.used += len;
        if self.oldest.is_none() {
            self.oldest = Some(head.pos);
        }
        self.newest = Some(head.pos);
        self.newest_off = off;
        self.newest_end = head.end();
        self.next = head.end();
        Ok(())
    }

    pub(super) fn read<'b>(
        &mut self,
        buf: &[u8],
        pos: u64,
        out: &'b mut [u8],
    ) -> Result<Option<StoredEvent<'b>>, RamRingError> {
        let n = buf.len();
        if self.used == 0 {
            return Ok(None);
        }
        if self.newest_end <= pos && self.newest.is_some_and(|n| n < pos) {
            return Ok(None);
        }
        let (mut off, mut walked) = match self.hint {
            Some((off, hint_pos)) if hint_pos <= pos => {
                let walked = if off >= self.start {
                    off - self.start
                } else {
                    off + n - self.start
                };
                (off, walked)
            }
            _ => (self.start, 0),
        };
        // the newest record holds `pos`
        if self.newest.is_some_and(|n| n <= pos) {
            off = self.newest_off;
            walked = 0;
        }
        loop {
            let rec = Self::record(buf, off);
            if rec.pos >= pos || rec.end > pos {
                self.hint = Some((off, rec.pos));
                let item_len = rec.len - Self::overhead() - rec.head_len;
                if out.len() < item_len {
                    return Err(RamRingError::BufferTooSmall { item_len });
                }
                let head_off = Self::wrap(n, off + Self::overhead());
                let mut head_buf = [0u8; MAX_HEAD];
                Self::copy_out(buf, head_off, &mut head_buf[..rec.head_len]);
                let head = StreamHead::from_ww_bytes(&head_buf[..rec.head_len])
                    .map_err(RamRingError::Head)?;
                debug_assert_eq!(head.end(), rec.end);
                Self::copy_out(
                    buf,
                    Self::wrap(n, head_off + rec.head_len),
                    &mut out[..item_len],
                );
                return Ok(Some(StoredEvent {
                    head,
                    item: &out[..item_len],
                }));
            }
            walked += rec.len;
            if walked >= self.used {
                return Ok(None);
            }
            off = Self::wrap(n, off + rec.len);
        }
    }

    /// Forget events before `pos`. True when any went.
    pub(super) fn release(&mut self, buf: &[u8], pos: u64) -> bool {
        let mut any = false;
        while self.used > 0 && Self::record(buf, self.start).end <= pos {
            self.pop_oldest(buf, false);
            any = true;
        }
        any
    }

    pub(super) fn state(&self, capacity: usize, persistent: bool) -> StoreState {
        StoreState {
            session: self.session,
            oldest: self.oldest,
            newest: self.newest,
            newest_end: self.newest.map(|_| self.newest_end),
            next: self.next,
            used_bytes: self.used as u64,
            capacity_bytes: capacity as u64,
            persistent,
            dropped_units: self.dropped_units,
        }
    }

    pub(super) fn reset(&mut self, session: u32) {
        // field by field: a whole new ring would be built on the stack first
        self.session = session;
        self.start = 0;
        self.used = 0;
        self.oldest = None;
        self.newest = None;
        self.newest_off = 0;
        self.newest_end = 0;
        self.next = 0;
        self.dropped_units = 0;
        self.hint = None;
    }
}

impl<const N: usize> StreamStore for RamRing<N> {
    type Error = RamRingError;

    async fn push(
        &mut self,
        head: &StreamHead,
        item: &[u8],
    ) -> Result<(), PushError<RamRingError>> {
        self.ring.push(&mut self.buf, head, item, |_, _| {})
    }

    fn max_item_len(&self) -> usize {
        Ring::<NoCheck>::max_item_len(N)
    }

    async fn read<'b>(
        &mut self,
        pos: u64,
        buf: &'b mut [u8],
    ) -> Result<Option<StoredEvent<'b>>, RamRingError> {
        self.ring.read(&self.buf, pos, buf)
    }

    async fn release(&mut self, pos: u64) -> Result<(), RamRingError> {
        self.ring.release(&self.buf, pos);
        Ok(())
    }

    fn state(&self) -> StoreState {
        self.ring.state(N, false)
    }

    async fn reset(&mut self, session: u32) -> Result<(), RamRingError> {
        self.ring.reset(session);
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod fault {
    use std::cell::Cell;

    std::thread_local! {
        static BUDGET: Cell<usize> = const { Cell::new(usize::MAX) };
    }

    /// From now on, only `bytes` more bytes are written to a ring (`None`: no limit): the firmware faulted there.
    pub(crate) fn after_bytes(bytes: Option<usize>) {
        BUDGET.set(bytes.unwrap_or(usize::MAX));
    }

    /// Bytes still written of the next `len`.
    pub(super) fn take(len: usize) -> usize {
        let budget = BUDGET.get();
        let n = len.min(budget);
        if budget != usize::MAX {
            BUDGET.set(budget - n);
        }
        n
    }
}
