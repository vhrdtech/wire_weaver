//! [TapeWriter::resume_on]: reopen an unfinished piece after a crash and go on writing it.
//!
//! The piece is found the way [TapeReader] finds it: by its tail, else from its newest checkpoint
//! with the stretch after it scanned up to the first block that isn't good, else scanned from its start. Its bytes are
//! then walked from the start to where the reader stopped, block by block by the headers' lengths, a damaged block
//! the index lists by its entry, and the writer's state is rebuilt as the writer had it:
//!
//! - every block is accounted as a write accounts it: the BLAKE3 content hash, the index entries and
//!   the track ranges, the newest whole describe set, the counts, the run and the next seq;
//! - what the writer added is read back: a checkpoint index block moves the indexer on as placing it did, a describe
//!   copy clears the early and checkpoint copies it stood for;
//! - in an aligned piece a checkpoint due after a block waits for the next chunk, as it did.
//!
//! The piece is kept up to the end of its last counted block; what the writer added after it (a checkpoint, a
//! describe copy, or the end of a piece: the describe backup, the final index, the finish, the tail) is dropped and
//! the writer acts after that block again, so it adds the same blocks. A piece holding no counted block keeps its
//! head and the whole describe copy after it. Since what the writer adds depends only on the blocks before it and on
//! the [WriterOptions] (give the ones it was written with), a piece resumed and given the blocks lost after the kept
//! ones ends with the bytes, and the content hash, of a piece written without the crash.
//!
//! The options are checked against the [Layout](crate::Layout) the piece describes, before anything is cut off or
//! written: a chunk alignment other than the described one is refused ([WriteError::Layout]), as it would place blocks
//! the way the file says it doesn't. A whole describe set without a layout record describes an unaligned file. The
//! layout's gaps (`gap_max`, `resume_align`) need no check: a writer never leaves a gap, which every layout allows.
//! A piece with no whole describe set kept (its head cut off inside the set, no copy whole) has nothing to check
//! against; [Resumed::layout] is then None.
//!
//! Every length and offset read back goes through [FromFile] and a cap ([crate::MAX_BLOCK_LEN]) or the piece's end;
//! the walk moves on at least one byte a step, so it ends within the piece's length.

use std::collections::BTreeMap;
use std::io;

use super::{TapeWriter, WriteError, WriterOptions};
use crate::index::{IndexBlock, IndexEntry, IndexKind, is_structural};
use crate::piece::{FinishState, PieceHeadBlock};
use crate::reader::{ByteSource, ReadError, TapeReader, wide};
use crate::sink::{TapeSink, io_err};
use crate::{
    Block, BlockKind, CRC_LEN, FLAG_COPY, FLAG_SET_FIRST, FLAG_SET_LAST, FromFile, HEADER_LEN,
    MAX_BLOCK_LEN, block_len,
};

/// Bytes the walk reads and hashes at once in a stretch that holds no block.
const JUNK_READ_MAX: usize = 1 << 20;

/// What [TapeWriter::resume_on] kept of a piece.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Resumed {
    /// Bytes of the piece kept: up to the end of its last counted block.
    pub kept_bytes: u64,
    /// Bytes dropped after them: what the writer added after the last counted block, and whatever the reader didn't
    /// take (a cut block, damage, stale data).
    pub dropped_bytes: u64,
    /// Where the reader stopped taking blocks before the end of the bytes ([crate::PieceInfo::cut]).
    pub cut: Option<u64>,
    /// The kept blocks the index lists, in file order, offsets from the piece's start: what the final index will
    /// list of them.
    pub entries: Vec<IndexEntry>,
    /// The state of a finish block found (a crash after the finish, before the name): the piece was ended, and is
    /// open again.
    pub ended: Option<FinishState>,
    /// The layout the piece describes, which the options were checked against; None when it holds no whole describe
    /// set, and nothing was checked.
    pub layout: Option<crate::Layout>,
}

impl Resumed {
    /// A piece started anew, `bytes` long (its head and describe copy).
    pub(crate) fn started(bytes: u64) -> Self {
        Resumed {
            kept_bytes: bytes,
            dropped_bytes: 0,
            cut: None,
            entries: Vec::new(),
            ended: None,
            layout: None,
        }
    }
}

/// A [TapeSink] read as a [ByteSource].
struct SinkSource<'a, S>(&'a S);

impl<S: TapeSink> ByteSource for SinkSource<'_, S> {
    type Error = io::Error;

    fn size(&self) -> io::Result<u64> {
        self.0.size()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.0.read_at(offset, buf)
    }
}

/// One step of the walk: a block (good or not) or a stretch that holds none.
#[derive(Copy, Clone, Debug)]
struct Step {
    at: u64,
    len: u64,
    what: What,
}

#[derive(Copy, Clone, Debug)]
enum What {
    /// A block by its header or its index entry: its kind, header flags (0 when unreadable) and entry.
    Block {
        kind: BlockKind,
        flags: u16,
        entry: Option<IndexEntry>,
    },
    /// Bytes no header or entry explains: a gap, damage.
    Junk,
}

impl Step {
    fn end(&self) -> u64 {
        // a step lies within the piece: at + len <= its end
        self.at.saturating_add(self.len)
    }

    /// A block the writer counts: not one of the file structure, not a copy.
    fn counted(&self) -> bool {
        match self.what {
            What::Block { kind, flags, .. } => !is_structural(kind) && flags & FLAG_COPY == 0,
            What::Junk => false,
        }
    }
}

/// The piece's blocks as the reader knows them: index entries and gaps by offset, and where the piece ends.
struct Layout {
    entries: BTreeMap<u64, IndexEntry>,
    gaps: BTreeMap<u64, u64>,
    end: u64,
}

impl Layout {
    /// The step at `at` (before [Self::end]), its header read from `src`.
    fn step<S: TapeSink>(&self, src: &S, at: u64) -> Result<Step, WriteError> {
        let left = self.end.saturating_sub(at);
        if let Some(&len) = self.gaps.get(&at) {
            return Ok(Step {
                at,
                len: len.clamp(1, left),
                what: What::Junk,
            });
        }
        let mut header = [0u8; HEADER_LEN];
        let has_header = left >= wide(HEADER_LEN);
        if has_header {
            src.read_at(at, &mut header)
                .map_err(io_err("reading back a block header"))?;
        }
        let flags = u16::from_le_bytes([header[6], header[7]]);
        if let Some(e) = self.entries.get(&at) {
            // an entry read from the file: a block's length at most, within the piece
            let len = FromFile::new(e.len)
                .at_most(MAX_BLOCK_LEN)
                .ok()
                .map(wide)
                .filter(|len| *len >= wide(HEADER_LEN + CRC_LEN) && *len <= left);
            if let Some(len) = len {
                let same = has_header && BlockKind::from_u8(header[5]) == e.kind;
                return Ok(Step {
                    at,
                    len,
                    what: What::Block {
                        kind: e.kind,
                        flags: if same { flags } else { 0 },
                        entry: Some(*e),
                    },
                });
            }
        }
        if has_header
            && let Ok(len) = block_len(&header)
            && wide(len) <= left
        {
            return Ok(Step {
                at,
                len: wide(len),
                what: What::Block {
                    kind: BlockKind::from_u8(header[5]),
                    flags,
                    entry: None,
                },
            });
        }
        // nothing reads here: up to the next block the index lists or the next gap
        let after = at.saturating_add(1);
        let to = [
            self.entries.range(after..).next().map(|(k, _)| *k),
            self.gaps.range(after..).next().map(|(k, _)| *k),
        ]
        .into_iter()
        .flatten()
        .fold(self.end, u64::min);
        Ok(Step {
            at,
            len: to.saturating_sub(at).max(1),
            what: What::Junk,
        })
    }
}

/// Where the kept part of a piece ends, from its steps: after its last counted block, or, without one, after its head
/// and the whole describe copy following it.
#[derive(Default)]
struct KeptEnd {
    last_counted: Option<u64>,
    /// End of the piece's start: the head, then whole copy sets back to back
    start: u64,
    /// Still in the piece's start
    in_start: bool,
    /// A copy set is open in the start: blocks since one marked first
    open: bool,
}

impl KeptEnd {
    fn new() -> Self {
        KeptEnd {
            in_start: true,
            ..Self::default()
        }
    }

    fn take(&mut self, s: &Step) {
        if s.counted() {
            self.last_counted = Some(s.end());
            self.in_start = false;
            return;
        }
        if !self.in_start {
            return;
        }
        match s.what {
            What::Block {
                kind: BlockKind::PieceHead,
                ..
            } if s.at == 0 => self.start = s.end(),
            What::Block {
                kind: BlockKind::Describe,
                flags,
                ..
            } if flags & FLAG_COPY != 0 && (self.open || flags & FLAG_SET_FIRST != 0) => {
                self.open = flags & FLAG_SET_LAST == 0;
                if !self.open {
                    self.start = s.end();
                }
            }
            _ => self.in_start = false,
        }
    }

    fn end(&self) -> u64 {
        self.last_counted.unwrap_or(self.start)
    }
}

impl<S: TapeSink> TapeWriter<S> {
    /// Reopen the unfinished piece in `sink` (cut off by a crash, or ended and not yet named) to go on writing it,
    /// see the [module docs](self). `options` are the ones it was written with. The piece is truncated to the end of
    /// its last counted block, and the writer acts after that block as it did; the caller then gives it the blocks
    /// from [Self::next_id] on. A piece whose head describe set is damaged takes the describe set from its next whole
    /// copy. The producer seq of [Self::adopt_block] starts again: the first block adopted shows no gap.
    pub fn resume_on(sink: S, options: WriterOptions) -> Result<(Self, Resumed), WriteError> {
        Self::resume_with(sink, options, None)
    }

    /// [Self::resume_on]; when the piece kept holds no counted block and no whole describe set, `prev_describe` (the
    /// piece before's newest set, which a cut copies to the head of the next) is written again.
    pub(crate) fn resume_with(
        sink: S,
        options: WriterOptions,
        prev_describe: Option<Vec<Vec<u8>>>,
    ) -> Result<(Self, Resumed), WriteError> {
        let size = sink
            .size()
            .map_err(io_err("reading the length to resume"))?;
        let (layout, cut, ended) = read_layout(&sink)?;
        let mut kept = KeptEnd::new();
        let mut at = 0;
        while at < layout.end {
            let step = layout.step(&sink, at)?;
            kept.take(&step);
            at = step.end();
        }
        let kept_bytes = kept.end();
        let mut w = Self::create_on(sink, options.sync);
        w.apply(&options)?;
        let mut buf = Vec::new();
        let mut at = 0;
        while at < kept_bytes {
            let step = layout.step(&w.sink, at)?;
            w.replay(&step, &mut buf)?;
            at = step.end();
        }
        let set = if w.describe.is_empty() {
            prev_describe.as_deref().unwrap_or_default()
        } else {
            &w.describe
        };
        let layout = described_layout(set);
        if let Some(described) = layout
            && described.alignment.get() != options.alignment
        {
            return Err(WriteError::Layout {
                described,
                alignment: options.alignment,
            });
        }
        if size > kept_bytes {
            w.sink
                .truncate(kept_bytes)
                .map_err(io_err("truncating the piece to resume"))?;
        }
        let entries = w.indexer.entries().to_vec();
        if kept.last_counted.is_some() {
            w.after_block()?;
        } else if let Some(prev) =
            prev_describe.filter(|_| w.describe.is_empty() && w.head.is_some())
        {
            w.describe = prev;
            w.replay_describe()?;
        }
        let resumed = Resumed {
            kept_bytes,
            dropped_bytes: size.saturating_sub(kept_bytes),
            cut,
            entries,
            ended,
            layout,
        };
        Ok((w, resumed))
    }

    /// Account the bytes of `step`, read into `buf`, as writing them did.
    fn replay(&mut self, step: &Step, buf: &mut Vec<u8>) -> Result<(), WriteError> {
        let What::Block { kind, entry, .. } = step.what else {
            return self.replay_junk(step, buf);
        };
        // a block step is at most MAX_BLOCK_LEN long
        let len = usize::try_from(step.len).map_err(|_overflow| too_long(step.len))?;
        buf.resize(len, 0);
        self.sink
            .read_at(step.at, buf)
            .map_err(io_err("reading back a block"))?;
        let good = Block::read(buf).is_ok_and(|b| b.len() == len);
        match (good, entry) {
            (true, _) => self.replay_good(step.at, buf),
            (false, Some(e)) => self.replay_damaged(e, kind, buf),
            (false, None) => self.replay_junk(step, buf),
        }
    }

    fn replay_good(&mut self, at: u64, block: &[u8]) -> Result<(), WriteError> {
        let b = Block::read(block).map_err(WriteError::Block)?;
        let (Some(header), Some(crc)) = (
            block.first_chunk::<HEADER_LEN>(),
            block.last_chunk::<CRC_LEN>(),
        ) else {
            return Err(WriteError::Block(crate::BlockError::Short {
                needed: HEADER_LEN,
            }));
        };
        match b.kind {
            BlockKind::PieceHead if at == 0 => {
                if let Ok(h) = PieceHeadBlock::read(&b) {
                    let head = h.head();
                    self.run = b.run;
                    // the head carries the seq of the last counted block before it; a first piece starts at 0
                    self.next_seq = match head.prev {
                        Some(_) => b.seq.wrapping_add(1),
                        None => 0,
                    };
                    self.head = Some((head, h.tracks().collect()));
                }
                self.account(block, header, *crc)
            }
            BlockKind::Index => {
                let checkpoint = IndexBlock::read(&b)
                    .is_ok_and(|ix| ix.kind() == IndexKind::Checkpoint && ix.self_offset() == at);
                if checkpoint {
                    // moves the indexer on as placing it did; the bytes are the file's
                    self.indexer
                        .checkpoint(at, crate::BlockId::new(b.run, b.seq))
                        .map_err(WriteError::Index)?;
                    self.index_due = false;
                }
                self.account(block, header, *crc)
            }
            BlockKind::Describe if b.is_copy() => {
                self.copy_taken();
                self.replaying = true;
                let r = self.account(block, header, *crc);
                self.replaying = false;
                self.copy_kept(block, b.flags);
                r
            }
            _ => {
                let counted = !is_structural(b.kind) && !b.is_copy();
                self.account(block, header, *crc)?;
                if counted && self.alignment != 0 && self.checkpoint_due() {
                    self.index_due = true;
                }
                Ok(())
            }
        }
    }

    /// A good copy block: the describe set it belongs to is the newest a piece after a cut holds before its own.
    fn copy_kept(&mut self, block: &[u8], flags: u16) {
        if !self.describe.is_empty() {
            return;
        }
        if flags & FLAG_SET_FIRST != 0 {
            self.describe_open.clear();
        } else if self.describe_open.is_empty() {
            return;
        }
        let open: usize = self.describe_open.iter().map(Vec::len).sum();
        if open.saturating_add(block.len()) > super::DESCRIBE_SET_LEN_MAX {
            self.describe_open.clear();
            return;
        }
        self.describe_open.push(block.to_vec());
        if flags & FLAG_SET_LAST != 0 {
            self.describe = core::mem::take(&mut self.describe_open);
        }
    }

    /// A block that doesn't read whole, which the index lists: its entry stands for it.
    fn replay_damaged(
        &mut self,
        e: IndexEntry,
        kind: BlockKind,
        bytes: &[u8],
    ) -> Result<(), WriteError> {
        let end = self.end_after(bytes.len())?;
        self.hash.update(bytes);
        self.indexer.placed_entry(e, bytes.len());
        self.describe_open.clear();
        self.bytes = end;
        self.blocks = self.blocks.saturating_add(1);
        self.since_describe = self.since_describe.saturating_add(wide(bytes.len()));
        if matches!(
            kind,
            BlockKind::Records | BlockKind::Events | BlockKind::Series
        ) {
            self.data_placed = true;
        }
        if matches!(kind, BlockKind::Events | BlockKind::Series) {
            self.chunks = self.chunks.saturating_add(1);
        }
        Ok(())
    }

    /// Bytes no block or entry explains: hashed, placed, never listed.
    fn replay_junk(&mut self, step: &Step, buf: &mut Vec<u8>) -> Result<(), WriteError> {
        let mut at = step.at;
        let end = step.end();
        while at < end {
            let n = usize::try_from(end.saturating_sub(at))
                .map_or(JUNK_READ_MAX, |n| n.min(JUNK_READ_MAX));
            buf.resize(n, 0);
            self.sink
                .read_at(at, buf)
                .map_err(io_err("reading back damaged bytes"))?;
            self.hash.update(buf);
            self.indexer.placed_unlisted(n);
            self.bytes = self.end_after(n)?;
            self.since_describe = self.since_describe.saturating_add(wide(n));
            // n is at most end - at
            at = at.saturating_add(wide(n));
        }
        self.describe_open.clear();
        Ok(())
    }
}

/// The piece at the start of `sink` as a reader finds it: its blocks by index entry and gap, where it ends, where a
/// scan was cut, and the state of a finish found.
fn read_layout<S: TapeSink>(
    sink: &S,
) -> Result<(Layout, Option<u64>, Option<FinishState>), WriteError> {
    let reader = TapeReader::open(SinkSource(sink)).map_err(WriteError::Read)?;
    let Some(p) = reader.pieces().first() else {
        return Ok((
            Layout {
                entries: BTreeMap::new(),
                gaps: BTreeMap::new(),
                end: 0,
            },
            None,
            None,
        ));
    };
    if p.start != 0 {
        return Err(WriteError::Read(ReadError::Index(
            crate::IndexError::Invalid("the piece to resume starts the bytes"),
        )));
    }
    let end = p.end;
    let entries = reader
        .entries(0)
        .map_err(WriteError::Read)?
        .into_iter()
        .filter_map(|e| Some((FromFile::new(e.offset).bounded(end).ok()?, e)))
        .collect();
    let gaps = p.gaps.iter().copied().collect();
    let ended = p.finish.as_ref().map(|(f, _)| f.state);
    Ok((Layout { entries, gaps, end }, p.cut, ended))
}

/// The layout whole describe set `set` (good blocks) states: its last layout record, else unaligned. None for no set.
fn described_layout(set: &[Vec<u8>]) -> Option<crate::Layout> {
    if set.is_empty() {
        return None;
    }
    let last = set
        .iter()
        .rev()
        .filter_map(|b| Block::read(b).ok())
        .find_map(|b| crate::walk::layout_in(&b));
    Some(last.unwrap_or(crate::Layout::UNALIGNED))
}

fn too_long(len: u64) -> WriteError {
    WriteError::Block(crate::BlockError::BadLength(
        u32::try_from(len).unwrap_or(u32::MAX),
    ))
}
