//! [RamRing]: a byte ring in RAM, the default store of reliable streams.

use super::{FullPolicy, PushError, StoreState, StoredEvent, StreamStore};
use crate::StreamHead;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, SerializeShrinkWrap};

/// Bytes of a record's own header: total length (u32), `pos` and `end` (u64 each), head length (u8).
pub const RECORD_OVERHEAD: usize = 4 + 8 + 8 + 1;
/// Longest encoded head a record takes.
const MAX_HEAD: usize = 64;

/// Events of one reliable stream in a byte ring of `N` bytes in RAM. Survives host reconnects, not reboots.
///
/// Each event takes its item, its encoded head and [RECORD_OVERHEAD] bytes. Records wrap around the end of the
/// buffer, so all `N` bytes are usable. `RamRing::new()` is `const`, so a big ring can live in a `static` (or a
/// `StaticCell`) instead of on the stack.
pub struct RamRing<const N: usize> {
    buf: [u8; N],
    policy: FullPolicy,
    session: u32,
    /// Offset of the oldest record
    start: usize,
    used: usize,
    oldest: Option<u64>,
    newest: Option<u64>,
    /// Offset of the newest record
    newest_off: usize,
    next: u64,
    dropped_units: u64,
    /// Offset and position of a record at or after the last one read, so sequential reads don't scan from the start
    hint: Option<(usize, u64)>,
}

/// Errors of a [RamRing].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RamRingError {
    /// [StreamStore::read] got a buffer smaller than the item.
    BufferTooSmall { item_len: usize },
    /// The head couldn't be encoded (a position of 2⁵⁷ or more) or is longer than a record allows.
    Head(wire_weaver::shrink_wrap::Error),
}

struct Record {
    len: usize,
    pos: u64,
    end: u64,
    head_len: usize,
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
            policy: FullPolicy::DropNewest,
            session: 0,
            start: 0,
            used: 0,
            oldest: None,
            newest: None,
            newest_off: 0,
            next: 0,
            dropped_units: 0,
            hint: None,
        }
    }

    /// What to do when an event doesn't fit, see [FullPolicy].
    pub const fn with_policy(mut self, policy: FullPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Events kept.
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    fn wrap(off: usize) -> usize {
        if off >= N { off - N } else { off }
    }

    fn copy_in(&mut self, off: usize, bytes: &[u8]) {
        let first = bytes.len().min(N - off);
        self.buf[off..off + first].copy_from_slice(&bytes[..first]);
        self.buf[..bytes.len() - first].copy_from_slice(&bytes[first..]);
    }

    fn copy_out(&self, off: usize, out: &mut [u8]) {
        let first = out.len().min(N - off);
        out[..first].copy_from_slice(&self.buf[off..off + first]);
        let rest = out.len() - first;
        out[first..].copy_from_slice(&self.buf[..rest]);
    }

    fn record(&self, off: usize) -> Record {
        let mut header = [0u8; RECORD_OVERHEAD];
        self.copy_out(off, &mut header);
        let u64_at = |i: usize| u64::from_le_bytes(header[i..i + 8].try_into().unwrap());
        Record {
            len: u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize,
            pos: u64_at(4),
            end: u64_at(12),
            head_len: header[20] as usize,
        }
    }

    fn pop_oldest(&mut self, dropped: bool) {
        let rec = self.record(self.start);
        if dropped {
            self.dropped_units += rec.end - rec.pos;
        }
        self.start = Self::wrap(self.start + rec.len);
        self.used -= rec.len;
        if self.used == 0 {
            self.oldest = None;
            self.newest = None;
        } else {
            self.oldest = Some(self.record(self.start).pos);
        }
        if let Some((_, pos)) = self.hint
            && pos <= rec.pos
        {
            self.hint = None;
        }
    }
}

impl<const N: usize> StreamStore for RamRing<N> {
    type Error = RamRingError;

    async fn push(
        &mut self,
        head: &StreamHead,
        item: &[u8],
    ) -> Result<(), PushError<RamRingError>> {
        let mut head_buf = [0u8; MAX_HEAD];
        let head_bytes = head
            .to_ww_bytes(&mut head_buf)
            .map_err(|e| PushError::Store(RamRingError::Head(e)))?;
        let len = RECORD_OVERHEAD + head_bytes.len() + item.len();
        if len > N || len > u32::MAX as usize {
            return Err(PushError::TooBig);
        }
        while N - self.used < len {
            match self.policy {
                FullPolicy::DropNewest => return Err(PushError::Full),
                FullPolicy::DropOldest => self.pop_oldest(true),
            }
        }
        let mut header = [0u8; RECORD_OVERHEAD];
        header[0..4].copy_from_slice(&(len as u32).to_le_bytes());
        header[4..12].copy_from_slice(&head.pos.to_le_bytes());
        header[12..20].copy_from_slice(&head.end().to_le_bytes());
        header[20] = head_bytes.len() as u8;
        let off = Self::wrap(self.start + self.used);
        let head_len = head_bytes.len();
        self.copy_in(off, &header);
        self.copy_in(Self::wrap(off + RECORD_OVERHEAD), &head_buf[..head_len]);
        self.copy_in(Self::wrap(off + RECORD_OVERHEAD + head_len), item);
        self.used += len;
        if self.oldest.is_none() {
            self.oldest = Some(head.pos);
        }
        self.newest = Some(head.pos);
        self.newest_off = off;
        self.next = head.end();
        Ok(())
    }

    async fn read<'b>(
        &mut self,
        pos: u64,
        buf: &'b mut [u8],
    ) -> Result<Option<StoredEvent<'b>>, RamRingError> {
        if self.used == 0 || self.newest.is_some_and(|n| n < pos) {
            return Ok(None);
        }
        let (mut off, mut walked) = match self.hint {
            Some((off, hint_pos)) if hint_pos <= pos => {
                let walked = if off >= self.start {
                    off - self.start
                } else {
                    off + N - self.start
                };
                (off, walked)
            }
            _ => (self.start, 0),
        };
        if self.newest.is_some_and(|n| n == pos) {
            off = self.newest_off;
            walked = 0;
        }
        loop {
            let rec = self.record(off);
            if rec.pos >= pos {
                self.hint = Some((off, rec.pos));
                let item_len = rec.len - RECORD_OVERHEAD - rec.head_len;
                if buf.len() < item_len {
                    return Err(RamRingError::BufferTooSmall { item_len });
                }
                let head_off = Self::wrap(off + RECORD_OVERHEAD);
                let mut head_buf = [0u8; MAX_HEAD];
                self.copy_out(head_off, &mut head_buf[..rec.head_len]);
                let head = StreamHead::from_ww_bytes(&head_buf[..rec.head_len])
                    .map_err(RamRingError::Head)?;
                debug_assert_eq!(head.end(), rec.end);
                self.copy_out(Self::wrap(head_off + rec.head_len), &mut buf[..item_len]);
                return Ok(Some(StoredEvent {
                    head,
                    item: &buf[..item_len],
                }));
            }
            walked += rec.len;
            if walked >= self.used {
                return Ok(None);
            }
            off = Self::wrap(off + rec.len);
        }
    }

    async fn release(&mut self, pos: u64) -> Result<(), RamRingError> {
        while self.used > 0 && self.record(self.start).end <= pos {
            self.pop_oldest(false);
        }
        Ok(())
    }

    fn state(&self) -> StoreState {
        StoreState {
            session: self.session,
            oldest: self.oldest,
            newest: self.newest,
            next: self.next,
            used_bytes: self.used as u64,
            capacity_bytes: N as u64,
            persistent: false,
            dropped_units: self.dropped_units,
        }
    }

    async fn reset(&mut self, session: u32) -> Result<(), RamRingError> {
        // field by field: a whole new ring would be built on the stack first
        self.session = session;
        self.start = 0;
        self.used = 0;
        self.oldest = None;
        self.newest = None;
        self.newest_off = 0;
        self.next = 0;
        self.dropped_units = 0;
        self.hint = None;
        Ok(())
    }
}
