//! [LogStore]: a [StreamStore] on a block device (SD card, NOR flash) that survives reboots and power cuts.
//!
//! The log is a ring of segments, each with a header (session, clock epoch, the position the stream was at), and
//! records with a CRC each; a superblock, written in turn to two slots every few segments, says where the log was.
//! Layout: [format].
//!
//! - **Power cuts**: a push writes its record and returns; the record is on the medium when it returns. A cut during
//!   a push loses at most that record: after the reboot the store finds the end of the log by walking the segment
//!   headers from the superblock and scanning the last segment, and counts a record it finds cut short
//!   ([Recovery::torn_records]). Positions continue after the last whole record, so nothing the sender ever read
//!   from the store is missing.
//! - **Reboots**: session, positions, the released position and the clock epoch are kept. Every mount starts a new
//!   clock epoch and every boot writes into a new segment, whose header carries the epoch; the first event pushed
//!   after a mount gets `ext.clock_epoch` filled in when the sender left it empty, so the host learns where the
//!   device's clock restarted.
//! - **Full**: [FullPolicy::DropNewest] refuses the event ([PushError::Full]) until released segments make room,
//!   [FullPolicy::DropOldest] forgets the oldest segment and counts its unreleased units in
//!   [StoreState::dropped_units].
//!
//! A segment is the unit of room: it is freed when every event in it is released (or dropped), and the rest of a
//! segment is left empty when an event doesn't fit or the device reboots. Pick segments a few times bigger than the
//! biggest event, and many of them (thousands on an SD card): a mount reads at most a few segment headers more than
//! the superblock knows of, and a read finds its segment by a binary search over the headers.

mod format;
mod medium;
#[cfg(feature = "nor-flash")]
mod nor;
#[cfg(feature = "sdmmc")]
mod sd;
#[cfg(any(test, feature = "std"))]
mod sim;

pub use medium::LogMedium;
#[cfg(feature = "nor-flash")]
pub use nor::NorMedium;
#[cfg(feature = "sdmmc")]
pub use sd::SdMedium;
#[cfg(any(test, feature = "std"))]
pub use sim::{SimError, SimMedium, SimNor, SimSd};

use super::{FullPolicy, PushError, StoreState, StoredEvent, StreamStore};
use crate::StreamHead;
use format::{
    CRC, MAX_HEAD, REC_HEADER_LEN, RecHeader, RecSlot, SB_LEN, SEG_HEADER_LEN, SegHeader,
    Superblock,
};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, SerializeShrinkWrap, UVlq32};

/// How a [LogStore] lays out its medium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogConfig {
    /// Bytes per segment: a multiple of the medium's write size (and erase size, for NOR flash), a few times the
    /// biggest event. The medium holds two superblock slots and as many segments as fit, at least 2.
    pub segment_size: u32,
    /// What to do when an event doesn't fit.
    pub policy: FullPolicy,
}

/// Errors of a [LogStore].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogError<E> {
    /// The medium failed. The segment being written is closed, the next push starts a new one.
    Medium(E),
    /// The medium holds a log laid out differently ([LogConfig::segment_size], medium size), or the config doesn't
    /// fit the medium or the stage buffer.
    Geometry,
    /// [StreamStore::read] got a buffer smaller than the item.
    BufferTooSmall {
        /// Bytes the item has.
        item_len: usize,
    },
    /// The head couldn't be encoded (a position of 2⁵⁷ or more).
    Head(wire_weaver::shrink_wrap::Error),
}

/// What a [LogStore::mount] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Recovery {
    /// No log was on the medium (or [LogStore::format] was called): a new one was started.
    pub formatted: bool,
    /// Records found cut short at the end of the log: a push that a power cut interrupted. Its event was never
    /// read back, so the host never saw it; positions continue after the last whole record.
    pub torn_records: u32,
    /// Segments found after the one the superblock knew of.
    pub segments_walked: u32,
    /// Clock epoch of this boot.
    pub clock_epoch: u32,
}

/// Where a record is and what it holds.
#[derive(Debug, Clone, Copy)]
struct Rec {
    seq: u64,
    off: u64,
    header: RecHeader,
    head: StreamHead,
    /// Bytes the record takes, padding included
    len: u64,
}

/// A [StreamStore] on a [LogMedium], see the [module docs](self).
///
/// `BUF` is the stage buffer: whole medium write units are assembled in it. It is a multiple of the medium's write
/// size and at least one of them big (512 for an SD card); there are two of them in the store.
pub struct LogStore<M: LogMedium, const BUF: usize> {
    medium: M,
    policy: FullPolicy,
    log_id: u32,
    slot_size: u64,
    seg_base: u64,
    seg_size: u64,
    segments: u64,
    seg_hdr_len: u64,
    rec_align: u64,
    sb_every: u64,
    sb_write_seq: u64,
    sb_tail: u64,

    session: u32,
    epoch: u32,
    epoch_pending: bool,
    head_seq: u64,
    tail_seq: u64,
    /// The tail segment takes pushes in this boot, from `tail_off`
    open: bool,
    tail_off: u64,
    /// Released up to here: events that end at or before it are gone
    low: u64,
    oldest: Option<u64>,
    /// Position of the newest event pushed (released or not)
    newest: Option<u64>,
    next: u64,
    dropped_units: u64,

    stage: [u8; BUF],
    stage_base: u64,
    stage_fill: usize,
    scratch: [u8; BUF],
    /// The last record read
    hint: Option<Hint>,
    recovery: Recovery,
}

/// The last record read: where the one after it starts, its position and end. Every record before `off` is at or
/// before `pos` and ends at or before `end`.
#[derive(Debug, Clone, Copy)]
struct Hint {
    seq: u64,
    off: u64,
    pos: u64,
    end: u64,
}

const fn round_up(x: u64, to: u64) -> u64 {
    x.div_ceil(to) * to
}

impl<M: LogMedium, const BUF: usize> LogStore<M, BUF> {
    /// Open the log on `medium`, or start a new one (session `new_session`) when there is none. Finds the end of
    /// the log after a power cut, see [Recovery].
    pub async fn mount(
        medium: M,
        config: LogConfig,
        new_session: u32,
    ) -> Result<Self, LogError<M::Error>> {
        let mut store = Self::new(medium, config)?;
        let mut found = None;
        for slot in 0..2 {
            if let Some(sb) = store.read_superblock(slot).await?
                && found.is_none_or(|f: Superblock| sb.write_seq > f.write_seq)
            {
                found = Some(sb);
            }
        }
        match found {
            None => store.start_log(new_session, 0, 0, 0).await?,
            Some(sb) => {
                if sb.segment_size as u64 != store.seg_size || sb.segments as u64 != store.segments
                {
                    return Err(LogError::Geometry);
                }
                store.recover(sb).await?;
            }
        }
        Ok(store)
    }

    /// Start a new, empty log on `medium` in session `session`, whatever it holds.
    pub async fn format(
        medium: M,
        config: LogConfig,
        session: u32,
    ) -> Result<Self, LogError<M::Error>> {
        let mut store = Self::new(medium, config)?;
        // carry the sequence numbers on, so nothing of an earlier log reads as part of this one
        let (mut write_seq, mut seq, mut epoch) = (0, 0, 0);
        for slot in 0..2 {
            if let Some(sb) = store.read_superblock(slot).await? {
                write_seq = write_seq.max(sb.write_seq + 1);
                seq = seq.max(sb.tail_seq + sb.segments as u64 + 1);
                epoch = epoch.max(sb.epoch + 1);
            }
        }
        store.start_log(session, write_seq, seq, epoch).await?;
        Ok(store)
    }

    fn new(medium: M, config: LogConfig) -> Result<Self, LogError<M::Error>> {
        let ws = M::WRITE_SIZE as u64;
        let es = if M::NEEDS_ERASE {
            M::ERASE_SIZE as u64
        } else {
            1
        };
        let rec_align = if M::NEEDS_ERASE { ws.max(4) } else { 4 };
        let mut slot_size = round_up(SB_LEN as u64, ws);
        slot_size = round_up(slot_size, es);
        let seg_size = config.segment_size as u64;
        let seg_base = 2 * slot_size;
        let segments = medium.capacity().saturating_sub(seg_base) / seg_size.max(1);
        let seg_hdr_len = round_up(SEG_HEADER_LEN as u64, rec_align);
        if ws == 0
            || !(BUF as u64).is_multiple_of(ws)
            || (BUF as u64) < round_up(SB_LEN.max(SEG_HEADER_LEN) as u64, ws)
            || !seg_size.is_multiple_of(ws)
            || !seg_size.is_multiple_of(es)
            || !seg_size.is_multiple_of(rec_align)
            || seg_size < seg_hdr_len + rec_align + REC_HEADER_LEN as u64
            || segments < 2
        {
            return Err(LogError::Geometry);
        }
        Ok(LogStore {
            medium,
            policy: config.policy,
            log_id: 0,
            slot_size,
            seg_base,
            seg_size,
            segments,
            seg_hdr_len,
            rec_align,
            sb_every: (segments / 4).clamp(1, 8),
            sb_write_seq: 0,
            sb_tail: 0,
            session: 0,
            epoch: 0,
            epoch_pending: false,
            head_seq: 0,
            tail_seq: 0,
            open: false,
            tail_off: 0,
            low: 0,
            oldest: None,
            newest: None,
            next: 0,
            dropped_units: 0,
            stage: [0xff; BUF],
            stage_base: 0,
            stage_fill: 0,
            scratch: [0; BUF],
            hint: None,
            recovery: Recovery::default(),
        })
    }

    /// What the mount found.
    pub fn recovery(&self) -> Recovery {
        self.recovery
    }

    /// Clock epoch of this boot: one more than the boot before.
    pub fn clock_epoch(&self) -> u32 {
        self.epoch
    }

    /// Segments the medium holds.
    pub fn segments(&self) -> u64 {
        self.segments
    }

    /// The medium, to hand it back after the store is done.
    pub fn into_medium(self) -> M {
        self.medium
    }

    async fn start_log(
        &mut self,
        session: u32,
        write_seq: u64,
        seq: u64,
        epoch: u32,
    ) -> Result<(), LogError<M::Error>> {
        self.log_id = (session ^ 0x5157_4c47)
            .wrapping_mul(0x9e37_79b9)
            .wrapping_add(write_seq as u32);
        self.sb_write_seq = write_seq;
        self.epoch = epoch;
        self.epoch_pending = epoch > 0;
        self.session = session;
        self.head_seq = seq;
        self.tail_seq = seq;
        self.open_segment(seq).await?;
        self.write_superblock().await?;
        self.recovery = Recovery {
            formatted: true,
            clock_epoch: epoch,
            ..Recovery::default()
        };
        Ok(())
    }

    async fn recover(&mut self, sb: Superblock) -> Result<(), LogError<M::Error>> {
        self.log_id = sb.log_id;
        self.sb_write_seq = sb.write_seq + 1;
        self.session = sb.session;
        self.epoch = sb.epoch;
        self.head_seq = sb.head_seq;
        self.tail_seq = sb.tail_seq;
        self.low = sb.low;
        self.dropped_units = sb.dropped_units;
        let mut recovery = Recovery::default();

        let tail_header = self.read_seg_header(sb.tail_seq).await?;
        let mut tail_header = match tail_header {
            Some(h) => h,
            None => {
                // nothing in the segment the superblock names: an empty log from there on
                self.next = sb.next;
                self.finish_mount(recovery).await?;
                return Ok(());
            }
        };
        self.epoch = self.epoch.max(tail_header.epoch);
        for _ in 0..self.segments {
            let Some(h) = self.read_seg_header(self.tail_seq + 1).await? else {
                break;
            };
            self.tail_seq += 1;
            recovery.segments_walked += 1;
            self.epoch = self.epoch.max(h.epoch);
            if h.session != self.session {
                // reset after the superblock was written
                self.session = h.session;
                self.head_seq = h.seq;
                self.low = 0;
                self.dropped_units = 0;
            }
            tail_header = h;
        }
        // drop oldest may have gone round after the superblock was written
        self.head_seq = self
            .head_seq
            .max((self.tail_seq + 1).saturating_sub(self.segments));

        // the end of the log: the last whole record of the tail segment
        self.next = tail_header.first_pos;
        self.newest = tail_header.newest;
        let mut off = self.seg_hdr_len;
        loop {
            match self.record_at(self.tail_seq, off, true).await? {
                Ok(rec) => {
                    self.next = rec.head.end();
                    self.newest = Some(rec.head.pos);
                    off += rec.len;
                }
                Err(torn) => {
                    if torn {
                        recovery.torn_records += 1;
                    }
                    break;
                }
            }
        }
        self.finish_mount(recovery).await
    }

    async fn finish_mount(&mut self, mut recovery: Recovery) -> Result<(), LogError<M::Error>> {
        self.oldest = self
            .first_kept(self.head_seq, self.seg_hdr_len, self.low)
            .await?
            .map(|r| r.head.pos);
        self.epoch += 1;
        self.epoch_pending = true;
        self.open = false;
        recovery.clock_epoch = self.epoch;
        self.recovery = recovery;
        self.write_superblock().await
    }

    fn seg_off(&self, seq: u64) -> u64 {
        self.seg_base + (seq % self.segments) * self.seg_size
    }

    async fn read_superblock(
        &mut self,
        slot: u64,
    ) -> Result<Option<Superblock>, LogError<M::Error>> {
        let mut b = [0u8; SB_LEN];
        self.medium
            .read(slot * self.slot_size, &mut b)
            .await
            .map_err(LogError::Medium)?;
        Ok(Superblock::decode(&b))
    }

    async fn write_superblock(&mut self) -> Result<(), LogError<M::Error>> {
        let sb = Superblock {
            write_seq: self.sb_write_seq,
            log_id: self.log_id,
            segment_size: self.seg_size as u32,
            segments: self.segments as u32,
            session: self.session,
            epoch: self.epoch,
            head_seq: self.head_seq,
            tail_seq: self.tail_seq,
            low: self.low,
            next: self.next,
            dropped_units: self.dropped_units,
        };
        let off = (self.sb_write_seq % 2) * self.slot_size;
        let len = round_up(SB_LEN as u64, M::WRITE_SIZE as u64) as usize;
        self.scratch[..len].fill(0xff);
        self.scratch[..SB_LEN].copy_from_slice(&sb.encode());
        if M::NEEDS_ERASE {
            self.medium
                .erase(off, off + self.slot_size)
                .await
                .map_err(LogError::Medium)?;
        }
        self.medium
            .write(off, &self.scratch[..len])
            .await
            .map_err(LogError::Medium)?;
        self.sb_write_seq += 1;
        self.sb_tail = self.tail_seq;
        Ok(())
    }

    async fn read_seg_header(&mut self, seq: u64) -> Result<Option<SegHeader>, LogError<M::Error>> {
        let mut b = [0u8; SEG_HEADER_LEN];
        let off = self.seg_off(seq);
        self.medium
            .read(off, &mut b)
            .await
            .map_err(LogError::Medium)?;
        Ok(SegHeader::decode(&b, self.log_id, seq))
    }

    /// The record at `off` in segment `seq`. `Err(torn)` when there is none: `torn` when what is there is a record
    /// cut short. The CRC is checked when `verify` (otherwise when the item is read).
    async fn record_at(
        &mut self,
        seq: u64,
        off: u64,
        verify: bool,
    ) -> Result<Result<Rec, bool>, LogError<M::Error>> {
        if off + REC_HEADER_LEN as u64 > self.seg_size {
            return Ok(Err(false));
        }
        let at = self.seg_off(seq) + off;
        let mut hb = [0u8; REC_HEADER_LEN];
        self.medium
            .read(at, &mut hb)
            .await
            .map_err(LogError::Medium)?;
        let tag = format::record_tag(self.log_id, seq);
        let header = match RecHeader::decode(&hb, tag, (self.seg_size - off) as usize) {
            RecSlot::Record(h) => h,
            RecSlot::End => return Ok(Err(false)),
            RecSlot::Torn => return Ok(Err(true)),
        };
        let mut head_buf = [0u8; MAX_HEAD];
        let head_bytes = &mut head_buf[..header.head_len];
        self.medium
            .read(at + REC_HEADER_LEN as u64, head_bytes)
            .await
            .map_err(LogError::Medium)?;
        let Ok(head) = StreamHead::from_ww_bytes(head_bytes) else {
            return Ok(Err(true));
        };
        if verify {
            let mut digest = CRC.digest();
            format::record_crc_start(&mut digest, self.log_id, seq, &hb);
            digest.update(head_bytes);
            let mut done = 0;
            let item_at = at + (REC_HEADER_LEN + header.head_len) as u64;
            while done < header.item_len {
                let n = (header.item_len - done).min(BUF);
                self.medium
                    .read(item_at + done as u64, &mut self.scratch[..n])
                    .await
                    .map_err(LogError::Medium)?;
                digest.update(&self.scratch[..n]);
                done += n;
            }
            if digest.finalize() != header.crc {
                return Ok(Err(true));
            }
        }
        let len = round_up(
            (REC_HEADER_LEN + header.head_len + header.item_len) as u64,
            self.rec_align,
        );
        Ok(Ok(Rec {
            seq,
            off,
            header,
            head,
            len,
        }))
    }

    /// The first record from `off` in segment `seq` on (into later segments) with `pos >= from` and that ends after
    /// `low`, or `None`.
    async fn first_at_or_after(
        &mut self,
        mut seq: u64,
        mut off: u64,
        from: u64,
    ) -> Result<Option<Rec>, LogError<M::Error>> {
        loop {
            if seq > self.tail_seq {
                return Ok(None);
            }
            match self.record_at(seq, off, false).await? {
                Ok(rec) => {
                    if rec.head.pos >= from && rec.head.end() > self.low {
                        return Ok(Some(rec));
                    }
                    off += rec.len;
                }
                Err(_) => {
                    seq += 1;
                    off = self.seg_hdr_len;
                }
            }
        }
    }

    /// The first record that ends after `low`, from `off` in segment `seq` on.
    async fn first_kept(
        &mut self,
        seq: u64,
        off: u64,
        low: u64,
    ) -> Result<Option<Rec>, LogError<M::Error>> {
        let saved = self.low;
        self.low = low;
        let found = self.first_at_or_after(seq, off, 0).await;
        self.low = saved;
        found
    }

    /// The last segment whose first position is at or before `pos` (the head segment when there is none).
    async fn locate(&mut self, pos: u64) -> Result<u64, LogError<M::Error>> {
        let (mut lo, mut hi) = (self.head_seq, self.tail_seq);
        while lo < hi {
            let mid = lo + (hi - lo).div_ceil(2);
            match self.read_seg_header(mid).await? {
                Some(h) if h.first_pos <= pos => lo = mid,
                Some(_) => hi = mid - 1,
                // can't happen inside the ring; search before it
                None => hi = mid - 1,
            }
        }
        Ok(lo)
    }

    /// Append bytes to the stage, writing it out when full.
    async fn stage_bytes(&mut self, mut bytes: &[u8]) -> Result<(), M::Error> {
        while !bytes.is_empty() {
            let n = bytes.len().min(BUF - self.stage_fill);
            self.stage[self.stage_fill..self.stage_fill + n].copy_from_slice(&bytes[..n]);
            self.stage_fill += n;
            bytes = &bytes[n..];
            if self.stage_fill == BUF {
                self.medium.write(self.stage_base, &self.stage).await?;
                self.stage_base += BUF as u64;
                self.stage_fill = 0;
                self.stage.fill(0xff);
            }
        }
        Ok(())
    }

    /// Pad the stage to the record alignment and write out what it holds. A part of a write unit stays in the stage
    /// and is written again with the next record (only on media that overwrite, NOR records are padded to units).
    async fn stage_commit(&mut self) -> Result<(), M::Error> {
        let pad = round_up(self.stage_fill as u64, self.rec_align) as usize - self.stage_fill;
        self.stage_bytes(&[0xff; 64][..pad]).await?;
        let ws = M::WRITE_SIZE;
        let n = self.stage_fill.div_ceil(ws) * ws;
        if n > 0 {
            self.medium.write(self.stage_base, &self.stage[..n]).await?;
        }
        let whole = self.stage_fill - self.stage_fill % ws;
        self.stage.copy_within(whole..self.stage_fill, 0);
        self.stage_fill -= whole;
        self.stage[self.stage_fill..].fill(0xff);
        self.stage_base += whole as u64;
        Ok(())
    }

    /// Open segment `seq` for pushes: erase it (NOR), write its header.
    async fn open_segment(&mut self, seq: u64) -> Result<(), LogError<M::Error>> {
        self.open = false;
        let base = self.seg_off(seq);
        if M::NEEDS_ERASE {
            self.medium
                .erase(base, base + self.seg_size)
                .await
                .map_err(LogError::Medium)?;
        }
        let header = SegHeader {
            log_id: self.log_id,
            seq,
            session: self.session,
            epoch: self.epoch,
            first_pos: self.next,
            newest: self.newest,
        };
        self.stage_base = base;
        self.stage_fill = 0;
        self.stage.fill(0xff);
        let r = async {
            self.stage_bytes(&header.encode()).await?;
            self.stage_commit().await
        }
        .await;
        r.map_err(LogError::Medium)?;
        self.tail_seq = seq;
        self.tail_off = self.seg_hdr_len;
        self.open = true;
        if seq - self.sb_tail >= self.sb_every {
            self.write_superblock().await?;
        }
        Ok(())
    }

    /// Forget the head segment (drop oldest), counting its unreleased units.
    async fn drop_head(&mut self) -> Result<(), LogError<M::Error>> {
        let seq = self.head_seq;
        let mut off = self.seg_hdr_len;
        while let Ok(rec) = self.record_at(seq, off, false).await? {
            if rec.head.end() > self.low {
                self.dropped_units += rec.head.end() - rec.head.pos.max(self.low);
            }
            off += rec.len;
        }
        self.head_seq += 1;
        if self.hint.is_some_and(|h| h.seq < self.head_seq) {
            self.hint = None;
        }
        self.oldest = self
            .first_kept(self.head_seq, self.seg_hdr_len, self.low)
            .await?
            .map(|r| r.head.pos);
        Ok(())
    }

    async fn push_inner(
        &mut self,
        head: &StreamHead,
        item: &[u8],
    ) -> Result<(), PushError<LogError<M::Error>>> {
        let mut head = *head;
        if self.epoch_pending && head.ext.is_none_or(|e| e.clock_epoch.is_none()) {
            let mut ext = head.ext.unwrap_or_default();
            ext.clock_epoch = Some(UVlq32(self.epoch));
            head.ext = Some(ext);
        }
        let mut head_buf = [0u8; MAX_HEAD];
        let head_bytes = head
            .to_ww_bytes(&mut head_buf)
            .map_err(|e| PushError::Store(LogError::Head(e)))?;
        let rec_len = round_up(
            (REC_HEADER_LEN + head_bytes.len() + item.len()) as u64,
            self.rec_align,
        );
        if rec_len > self.seg_size - self.seg_hdr_len || item.len() > u32::MAX as usize {
            return Err(PushError::TooBig);
        }
        if !self.open || self.tail_off + rec_len > self.seg_size {
            while self.tail_seq + 1 - self.head_seq >= self.segments {
                match self.policy {
                    FullPolicy::DropNewest => return Err(PushError::Full),
                    FullPolicy::DropOldest => self.drop_head().await.map_err(PushError::Store)?,
                }
            }
            self.open_segment(self.tail_seq + 1)
                .await
                .map_err(PushError::Store)?;
        }

        let tag = format::record_tag(self.log_id, self.tail_seq);
        let header = RecHeader {
            head_len: head_bytes.len(),
            item_len: item.len(),
            crc: 0,
        };
        let hb = header.encode(tag);
        let mut digest = CRC.digest();
        format::record_crc_start(&mut digest, self.log_id, self.tail_seq, &hb);
        digest.update(head_bytes);
        digest.update(item);
        let hb = RecHeader {
            crc: digest.finalize(),
            ..header
        }
        .encode(tag);
        debug_assert_eq!(
            self.stage_base + self.stage_fill as u64,
            self.seg_off(self.tail_seq) + self.tail_off
        );
        let written = async {
            self.stage_bytes(&hb).await?;
            self.stage_bytes(head_bytes).await?;
            self.stage_bytes(item).await?;
            self.stage_commit().await
        }
        .await;
        if let Err(e) = written {
            // what is on the medium after the failure is unknown: continue in a new segment
            self.open = false;
            return Err(PushError::Store(LogError::Medium(e)));
        }
        self.tail_off += rec_len;
        if self.oldest.is_none() {
            self.oldest = Some(head.pos);
        }
        self.newest = Some(head.pos);
        self.next = head.end();
        self.epoch_pending = false;
        Ok(())
    }
}

impl<M: LogMedium, const BUF: usize> StreamStore for LogStore<M, BUF> {
    type Error = LogError<M::Error>;

    async fn push(&mut self, head: &StreamHead, item: &[u8]) -> Result<(), PushError<Self::Error>> {
        self.push_inner(head, item).await
    }

    async fn read<'b>(
        &mut self,
        pos: u64,
        buf: &'b mut [u8],
    ) -> Result<Option<StoredEvent<'b>>, Self::Error> {
        if self.oldest.is_none() || self.newest.is_some_and(|n| n < pos) {
            return Ok(None);
        }
        let (seq, off) = match self.hint {
            // every record before the hint is at or before its position
            Some(h) if h.pos < pos && h.seq >= self.head_seq => (h.seq, h.off),
            _ => (self.locate(pos).await?, self.seg_hdr_len),
        };
        let (mut seq, mut off) = (seq, off);
        loop {
            let Some(rec) = self.first_at_or_after(seq, off, pos).await? else {
                return Ok(None);
            };
            let item_len = rec.header.item_len;
            if buf.len() < item_len {
                return Err(LogError::BufferTooSmall { item_len });
            }
            let at = self.seg_off(rec.seq) + rec.off;
            let mut hb = [0u8; REC_HEADER_LEN];
            let mut head_buf = [0u8; MAX_HEAD];
            let head_bytes = &mut head_buf[..rec.header.head_len];
            self.medium
                .read(at, &mut hb)
                .await
                .map_err(LogError::Medium)?;
            self.medium
                .read(at + REC_HEADER_LEN as u64, head_bytes)
                .await
                .map_err(LogError::Medium)?;
            self.medium
                .read(
                    at + (REC_HEADER_LEN + rec.header.head_len) as u64,
                    &mut buf[..item_len],
                )
                .await
                .map_err(LogError::Medium)?;
            let mut digest = CRC.digest();
            format::record_crc_start(&mut digest, self.log_id, rec.seq, &hb);
            digest.update(head_bytes);
            digest.update(&buf[..item_len]);
            if digest.finalize() != rec.header.crc {
                // cut short by a power cut: the end of its segment
                seq = rec.seq + 1;
                off = self.seg_hdr_len;
                continue;
            }
            self.hint = Some(Hint {
                seq: rec.seq,
                off: rec.off + rec.len,
                pos: rec.head.pos,
                end: rec.head.end(),
            });
            return Ok(Some(StoredEvent {
                head: rec.head,
                item: &buf[..item_len],
            }));
        }
    }

    async fn release(&mut self, pos: u64) -> Result<(), Self::Error> {
        if pos <= self.low {
            return Ok(());
        }
        self.low = pos;
        while self.head_seq < self.tail_seq {
            match self.read_seg_header(self.head_seq + 1).await? {
                Some(h) if h.first_pos <= pos => self.head_seq += 1,
                _ => break,
            }
        }
        let start = match self.hint {
            Some(h) if h.end <= pos && h.seq >= self.head_seq => (h.seq, h.off),
            _ => (self.locate(pos).await?, self.seg_hdr_len),
        };
        self.oldest = self
            .first_kept(start.0, start.1, pos)
            .await?
            .map(|r| r.head.pos);
        Ok(())
    }

    fn state(&self) -> StoreState {
        let open_part = if self.open {
            self.tail_off
        } else {
            self.seg_size
        };
        StoreState {
            session: self.session,
            oldest: self.oldest,
            newest: self.oldest.and(self.newest),
            next: self.next,
            used_bytes: (self.tail_seq - self.head_seq) * self.seg_size + open_part,
            capacity_bytes: self.segments * self.seg_size,
            persistent: true,
            dropped_units: self.dropped_units,
        }
    }

    async fn reset(&mut self, session: u32) -> Result<(), Self::Error> {
        self.session = session;
        self.head_seq = self.tail_seq + 1;
        self.low = 0;
        self.oldest = None;
        self.newest = None;
        self.next = 0;
        self.dropped_units = 0;
        self.hint = None;
        self.open_segment(self.tail_seq + 1).await?;
        if self.sb_tail != self.tail_seq {
            self.write_superblock().await?;
        }
        Ok(())
    }
}
