//! [TapeReader]: a tape read by positional reads, without a scan where its index allows.
//!
//! A [ByteSource] is anything with a length that reads bytes at an offset: a byte slice, a file, an HTTP resource read
//! by range requests in a browser. The reader needs no file system and no threads, so it builds for
//! `wasm32-unknown-unknown`.
//!
//! [TapeReader::open] finds the pieces from the end: a good [tail](crate::Tail) gives the piece's start, finish and
//! final index, and the previous piece's tail sits just before that start, so concatenated pieces cost one small read
//! each. A piece without a tail (being written, or cut off by a crash) is found from its newest checkpoint
//! [index](crate::index) block, looked for backwards from the end, and the stretch after that checkpoint is scanned;
//! a piece without any checkpoint is scanned from the start of what is left. Scans stop at the first block that
//! isn't good or belongs to a run that may not start there, as [Recovery::Prefix](crate::Recovery) does, and say where
//! ([PieceInfo::cut]); a stretch the run's data goes on after within the layout's gap is read past
//! ([PieceInfo::gaps]). A raw medium ([Medium::Raw]) is walked forward from its start or a hint instead
//! ([OpenOptions::medium]).
//!
//! Index entries of a finished piece are read when asked for: all of them ([TapeReader::entries]) or one track's
//! ([TapeReader::track_entries]: the final index's head and that track's entries, each checked by its own CRC).

use alloc::vec;
use alloc::vec::Vec;

use crate::bytes::le_u8;
use crate::index::{HEAD_LEN_PREFIX, INDEX_LEN_MAX, IndexBlock, IndexEntry, IndexError, IndexKind};
use crate::piece::{
    ChainReport, Finish, FinishBlock, PieceHead, PieceHeadBlock, PieceSummary, TAIL_LEN, Tail,
    TrackRange, TrackStart, check_chain,
};
use crate::walk::{Head, RunState, Verdict, is_free};
use crate::{
    Block, BlockError, BlockKind, FLAG_SET_FIRST, FLAG_SET_LAST, GAP_CHECKS_MAX, HEADER_LEN,
    Layout, MAGIC, MAX_BLOCK_LEN, Medium, VERSION, block_len,
};

/// Most blocks [TapeReader::describe_set] skips past a scan's cut looking for a describe copy: the damaged head set,
/// the first data block and the summary blocks placed with it fit many times over.
pub const DESCRIBE_SEEK_BLOCKS_MAX: usize = 64;

/// Bytes that read at an offset.
pub trait ByteSource {
    type Error: core::fmt::Debug;

    /// Length of the bytes. (Not `len`, so it never shadows a slice's own.)
    fn size(&self) -> Result<u64, Self::Error>;

    /// Fill `buf` from `offset`; the range is within [Self::size] (the reader checks).
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), Self::Error>;
}

/// A read past the end of a byte slice.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutOfRange;

impl ByteSource for [u8] {
    type Error = OutOfRange;

    fn size(&self) -> Result<u64, OutOfRange> {
        Ok(wide(self.len()))
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), OutOfRange> {
        let from = usize::try_from(offset).map_err(|_| OutOfRange)?;
        let src = from
            .checked_add(buf.len())
            .and_then(|to| self.get(from..to))
            .ok_or(OutOfRange)?;
        buf.copy_from_slice(src);
        Ok(())
    }
}

impl<S: ByteSource + ?Sized> ByteSource for &S {
    type Error = S::Error;

    fn size(&self) -> Result<u64, S::Error> {
        (**self).size()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), S::Error> {
        (**self).read_at(offset, buf)
    }
}

impl ByteSource for Vec<u8> {
    type Error = OutOfRange;

    fn size(&self) -> Result<u64, OutOfRange> {
        Ok(wide(self.len()))
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), OutOfRange> {
        self.as_slice().read_at(offset, buf)
    }
}

#[cfg(all(feature = "std", unix))]
impl ByteSource for std::fs::File {
    type Error = std::io::Error;

    fn size(&self) -> Result<u64, std::io::Error> {
        Ok(self.metadata()?.len())
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), std::io::Error> {
        std::os::unix::fs::FileExt::read_exact_at(self, buf, offset)
    }
}

#[cfg(all(feature = "std", windows))]
impl ByteSource for std::fs::File {
    type Error = std::io::Error;

    fn size(&self) -> Result<u64, std::io::Error> {
        Ok(self.metadata()?.len())
    }

    fn read_at(&self, mut offset: u64, mut buf: &mut [u8]) -> Result<(), std::io::Error> {
        while !buf.is_empty() {
            match std::os::windows::fs::FileExt::seek_read(self, buf, offset)? {
                0 => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                n => {
                    let past_end = || std::io::Error::from(std::io::ErrorKind::InvalidData);
                    buf = core::mem::take(&mut buf)
                        .get_mut(n..)
                        .ok_or_else(past_end)?;
                    offset = offset.checked_add(wide(n)).ok_or_else(past_end)?;
                }
            }
        }
        Ok(())
    }
}

/// Why a tape can't be read.
#[derive(Debug)]
#[non_exhaustive]
pub enum ReadError<E> {
    /// The byte source failed.
    Source(E),
    /// An index, finish, tail or piece head block is bad where a good one must be.
    Index(IndexError),
    /// The block at `offset` isn't good.
    Block { offset: u64, error: BlockError },
    /// The piece has no finish block, so no content hash.
    Unfinished,
    /// No piece has this number: the reader has fewer.
    NoPiece(usize),
}

impl<E: core::fmt::Debug> core::fmt::Display for ReadError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReadError::Source(e) => write!(f, "reading the bytes: {e:?}"),
            ReadError::Index(e) => write!(f, "index or piece block: {e:?}"),
            ReadError::Block { offset, error } => write!(f, "block at byte {offset}: {error:?}"),
            ReadError::Unfinished => write!(f, "the piece has no finish block"),
            ReadError::NoPiece(i) => write!(f, "no piece {i}"),
        }
    }
}

#[cfg(feature = "std")]
impl<E: core::fmt::Debug> std::error::Error for ReadError<E> {}

/// How [TapeReader::open] looks for pieces.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OpenOptions {
    /// Most bytes looked through backwards from the end of an unfinished piece for its newest checkpoint before
    /// scanning what is left from its start instead.
    pub scan_back_max: u64,
    /// What the bytes are kept on. A file is read from its end (tails, checkpoints); a [Medium::Raw] is walked
    /// forward from [Self::start], since its end holds stale blocks or nothing and a stale tail there would pass
    /// for the newest piece.
    pub medium: Medium,
    /// Where a [Medium::Raw] walk starts: the medium's start, or a hint the medium layer gives (its own superblock).
    pub start: u64,
    /// The gap rules of a [Medium::Raw] from its start, before a describe block says them; None: none until then.
    pub layout: Option<Layout>,
}

/// Default of [OpenOptions::scan_back_max]: 256 MiB, room for a few checkpoint intervals of the largest chunks.
pub const DEFAULT_SCAN_BACK_MAX: u64 = 256 << 20;

impl Default for OpenOptions {
    fn default() -> Self {
        OpenOptions {
            scan_back_max: DEFAULT_SCAN_BACK_MAX,
            medium: Medium::File,
            start: 0,
            layout: None,
        }
    }
}

/// How a piece was found.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// By its tail.
    Tail,
    /// By its newest checkpoint (or final) index block, `scanned` bytes after it read block by block.
    Checkpoint { scanned: u64 },
    /// Without an index: `scanned` bytes read block by block.
    Scan { scanned: u64 },
}

/// A piece of the tape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PieceInfo {
    /// Where the piece starts in the byte source.
    pub start: u64,
    /// Where its good bytes end.
    pub end: u64,
    pub head: Option<(PieceHead, Vec<TrackStart>)>,
    pub finish: Option<(Finish, Vec<TrackRange>)>,
    /// The finish block's offset in the piece.
    pub finish_offset: Option<u64>,
    pub tail: Option<Tail>,
    pub found: Found,
    /// Where the bytes stopped being good blocks, when a scan stopped before the end of the piece's stretch: a cut
    /// last block, damage, something that isn't a block, or a block of another run where no run may start (stale
    /// data behind the end of data).
    pub cut: Option<u64>,
    /// Blank or bad stretches a scan read past, the run's data going on after them within the layout's
    /// [gap](Layout::gap_max): offset and length of each.
    pub gaps: Vec<(u64, u64)>,
    /// Blocks a scan took in place of the block before them: the same run and seq with other bytes (the later copy
    /// wins); their offsets. A copy with the same bytes is dropped silently.
    pub replaced: Vec<u64>,
    /// Entries a scan or the checkpoint chain gave, in file order; None for a piece read by its tail
    entries: Option<Vec<IndexEntry>>,
    /// The walk where a scan was cut: the runs and seqs a block past the cut must go on with
    cut_walk: Option<RunState>,
}

impl PieceInfo {
    fn new(start: u64, found: Found) -> Self {
        PieceInfo {
            start,
            end: start,
            head: None,
            finish: None,
            finish_offset: None,
            tail: None,
            found,
            cut: None,
            gaps: Vec::new(),
            replaced: Vec::new(),
            entries: Some(Vec::new()),
            cut_walk: None,
        }
    }

    /// What [check_chain] needs of the piece; None for one without a head.
    pub fn summary(&self) -> Option<PieceSummary> {
        let (head, _) = self.head.as_ref()?;
        let end = self
            .finish
            .as_ref()
            .map(|(f, _)| (f.state, f.hash, f.next))
            .or(self.tail.map(|t| (t.state, t.hash, t.next)));
        Some(PieceSummary {
            ident: head.ident,
            prev: head.prev,
            end,
        })
    }
}

/// `n` as a `u64`.
pub(crate) fn wide(n: usize) -> u64 {
    const { assert!(usize::BITS <= u64::BITS, "a usize fits a u64") };
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// `n` as the `u32` of [BlockError::BadLength]; `u32::MAX` for a longer one, over any block's length either way.
pub(crate) fn report_len(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A block checked once, its bytes owned: lent out as a [Block] again without a second check.
#[derive(Clone, Debug)]
pub(crate) struct OwnedBlock {
    bytes: Vec<u8>,
    /// The header of the block `bytes` holds whole
    head: Block<'static>,
}

impl OwnedBlock {
    /// Check `bytes` as one whole block.
    pub(crate) fn read(bytes: Vec<u8>) -> Result<Self, BlockError> {
        let b = Block::read(&bytes)?;
        if b.len() != bytes.len() {
            return Err(BlockError::BadLength(report_len(bytes.len())));
        }
        let head = b.detach();
        Ok(OwnedBlock { bytes, head })
    }

    #[expect(
        clippy::expect_used,
        reason = "OwnedBlock::read checked these bytes as one whole block, at least HEADER_LEN + CRC_LEN long"
    )]
    pub(crate) fn block(&self) -> Block<'_> {
        self.head
            .on(&self.bytes)
            .expect("OwnedBlock::read checked these bytes as one whole block")
    }

    #[cfg(feature = "std")]
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[cfg(feature = "std")]
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }
}

/// Longest read a [TapeReader] makes at once: a block's length. A longer length the bytes state is damage.
const READ_LEN_MAX: usize = MAX_BLOCK_LEN;

/// Bytes [TapeReader::newest_index] and the blank checks read at once.
const WINDOW: usize = 64 * 1024;

/// Bytes [TapeReader::verify] hashes per read.
const VERIFY_STEP: usize = 1 << 20;

/// A tape read by positional reads, see the [module docs](self).
pub struct TapeReader<S> {
    src: S,
    len: u64,
    pieces: Vec<PieceInfo>,
}

type R<T, S> = Result<T, ReadError<<S as ByteSource>::Error>>;

impl<S: ByteSource> TapeReader<S> {
    /// Find the pieces of `src` with the default [OpenOptions].
    pub fn open(src: S) -> R<Self, S> {
        Self::open_with(src, OpenOptions::default())
    }

    /// Find the pieces of `src` as `options` say.
    ///
    /// # Panics
    ///
    /// When [OpenOptions::layout] is not [valid](Layout::is_valid).
    pub fn open_with(src: S, options: OpenOptions) -> R<Self, S> {
        let len = src.size().map_err(ReadError::Source)?;
        let mut r = TapeReader {
            src,
            len,
            pieces: Vec::new(),
        };
        if options.medium == Medium::Raw {
            let mut walk = RunState::new(Medium::Raw);
            if let Some(l) = &options.layout {
                walk.set_layout(l);
            }
            let start = options.start.min(len);
            r.pieces = r.scan_with(
                start,
                len,
                PieceInfo::new(start, Found::Scan { scanned: 0 }),
                walk,
            )?;
            return Ok(r);
        }
        let mut hi = len;
        let mut rev: Vec<PieceInfo> = Vec::new();
        while hi > 0 {
            if let Some(p) = r.by_tail(hi)?
                && p.start < hi
            {
                hi = p.start;
                rev.push(p);
                continue;
            }
            let found = r.recover(hi, options)?;
            let Some(first) = found.first().filter(|f| f.start < hi) else {
                break;
            };
            hi = first.start;
            rev.extend(found.into_iter().rev());
        }
        rev.reverse();
        r.pieces = rev;
        Ok(r)
    }

    /// Length of the byte source.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The pieces, in file order.
    pub fn pieces(&self) -> &[PieceInfo] {
        &self.pieces
    }

    pub fn source(&self) -> &S {
        &self.src
    }

    /// How the pieces with a head link up.
    pub fn chain(&self) -> ChainReport {
        let s: Vec<PieceSummary> = self.pieces.iter().filter_map(PieceInfo::summary).collect();
        check_chain(&s)
    }

    fn piece(&self, i: usize) -> R<&PieceInfo, S> {
        self.pieces.get(i).ok_or(ReadError::NoPiece(i))
    }

    /// `n` bytes at `offset`: at most [READ_LEN_MAX], within the source.
    fn read(&self, offset: u64, n: usize) -> R<Vec<u8>, S> {
        let within = offset
            .checked_add(wide(n))
            .is_some_and(|end| end <= self.len);
        if n > READ_LEN_MAX || !within {
            return Err(ReadError::Block {
                offset,
                error: BlockError::Short { needed: n },
            });
        }
        let mut buf = vec![0u8; n];
        self.src
            .read_at(offset, &mut buf)
            .map_err(ReadError::Source)?;
        Ok(buf)
    }

    /// The whole block at `offset`, if one is there within `hi` and at most `max` long, and where it ends; not
    /// checked yet.
    fn block_bytes(
        &self,
        offset: u64,
        hi: u64,
        max: usize,
    ) -> R<Result<(Vec<u8>, u64), BlockError>, S> {
        if hi.saturating_sub(offset) < wide(HEADER_LEN) {
            return Ok(Err(BlockError::Short { needed: HEADER_LEN }));
        }
        let header = self.read(offset, HEADER_LEN)?;
        let len = match block_len(&header) {
            Ok(l) if l <= max => l,
            Ok(l) => return Ok(Err(BlockError::BadLength(report_len(l)))),
            Err(e) => return Ok(Err(e)),
        };
        let Some(end) = offset.checked_add(wide(len)).filter(|end| *end <= hi) else {
            return Ok(Err(BlockError::Short { needed: len }));
        };
        Ok(Ok((self.read(offset, len)?, end)))
    }

    /// A good block at `offset` within `hi` and where it ends, or why not.
    fn good_block(&self, offset: u64, hi: u64) -> R<Result<(OwnedBlock, u64), BlockError>, S> {
        Ok(match self.block_bytes(offset, hi, MAX_BLOCK_LEN)? {
            Ok((bytes, end)) => OwnedBlock::read(bytes).map(|b| (b, end)),
            Err(e) => Err(e),
        })
    }

    /// The finished piece whose tail ends at `hi`, when its tail and finish are good.
    fn by_tail(&self, hi: u64) -> R<Option<PieceInfo>, S> {
        let Some(tail_at) = hi.checked_sub(wide(TAIL_LEN)) else {
            return Ok(None);
        };
        let tb = self.read(tail_at, TAIL_LEN)?;
        let Ok(tail) = Tail::read(&tb) else {
            return Ok(None);
        };
        let Some(start) = hi.checked_sub(tail.piece_len) else {
            return Ok(None);
        };
        let Some(finish_at) = start.checked_add(tail.finish_offset) else {
            return Ok(None);
        };
        let fb = match self.good_block(finish_at, hi)? {
            Ok((b, _)) if wide(b.len()) == u64::from(tail.finish_len) => b,
            _ => return Ok(None),
        };
        let Ok(fin) = FinishBlock::read(&fb.block()) else {
            return Ok(None);
        };
        let finish = fin.finish();
        if finish.hash != tail.hash || finish.state != tail.state || finish.ident != tail.ident {
            return Ok(None);
        }
        let mut p = PieceInfo::new(start, Found::Tail);
        p.end = hi;
        p.head = self.head_at(start, hi)?;
        p.finish = Some((finish, fin.tracks().collect()));
        p.finish_offset = Some(tail.finish_offset);
        p.tail = Some(tail);
        p.entries = None;
        Ok(Some(p))
    }

    fn head_at(&self, offset: u64, hi: u64) -> R<Option<(PieceHead, Vec<TrackStart>)>, S> {
        let Ok((b, _)) = self.good_block(offset, hi)? else {
            return Ok(None);
        };
        Ok(PieceHeadBlock::read(&b.block())
            .ok()
            .map(|h| (h.head(), h.tracks().collect())))
    }

    /// The pieces of the unfinished stretch ending at `hi`: from the newest index block in reach, else scanned from
    /// the start.
    fn recover(&self, hi: u64, options: OpenOptions) -> R<Vec<PieceInfo>, S> {
        let lo = hi.saturating_sub(options.scan_back_max);
        let mut below = hi;
        while let Some((at, ob, end)) = self.newest_index(hi, lo, below)? {
            let block = ob.block();
            let Ok(ix) = IndexBlock::read(&block) else {
                below = at;
                continue;
            };
            return match at.checked_sub(ix.self_offset()) {
                Some(start) => self.by_checkpoint(hi, start, at, end, &ix),
                None => self.scan(0, hi, PieceInfo::new(0, Found::Scan { scanned: 0 })),
            };
        }
        self.scan(0, hi, PieceInfo::new(0, Found::Scan { scanned: 0 }))
    }

    /// The piece at `start` whose newest checkpoint `ix` is at `at` (ending at `end`), the stretch after it to `hi`
    /// scanned.
    fn by_checkpoint(
        &self,
        hi: u64,
        start: u64,
        at: u64,
        end: u64,
        ix: &IndexBlock<'_>,
    ) -> R<Vec<PieceInfo>, S> {
        let mut piece = PieceInfo::new(start, Found::Checkpoint { scanned: 0 });
        piece.head = self.head_at(start, hi)?;
        let mut lists = vec![ix.entries().collect::<Vec<_>>()];
        let mut prev = ix.prev();
        // offset in the piece and in the source of the oldest good checkpoint
        let (mut newest, mut newest_at) = (ix.self_offset(), at);
        let mut chain_whole = true;
        while let Some(off) = prev {
            let older = start.checked_add(off).filter(|_| off < newest);
            let ok = match older {
                Some(off_at) => match self.good_block(off_at, at)? {
                    Ok((b, _)) => match IndexBlock::read(&b.block()) {
                        Ok(p) if p.kind() == ix.kind() && p.self_offset() == off => {
                            lists.push(p.entries().collect());
                            prev = p.prev();
                            (newest, newest_at) = (off, off_at);
                            true
                        }
                        _ => false,
                    },
                    Err(_) => false,
                },
                None => false,
            };
            if !ok {
                chain_whole = false;
                break;
            }
        }
        let mut entries: Vec<IndexEntry> = lists.into_iter().rev().flatten().collect();
        if !chain_whole {
            // the chain broke: what is before the oldest good checkpoint is scanned
            let scanned = self.scan(
                start,
                newest_at,
                PieceInfo::new(start, Found::Scan { scanned: 0 }),
            )?;
            let mut early: Vec<IndexEntry> = scanned
                .into_iter()
                .flat_map(|p| p.entries.unwrap_or_default())
                .collect();
            early.append(&mut entries);
            entries = early;
        }
        if ix.kind() == IndexKind::Final {
            entries.sort_by_key(|e| e.offset);
        }
        // the stretch after the checkpoint is read with the gap rules the piece's describe blocks state
        let mut walk = RunState::new(Medium::File);
        if let Some(layout) = self.layout_of(start, &entries, at)? {
            walk.set_layout(&layout);
        }
        piece.entries = Some(entries);
        self.scan_with(end, hi, piece, walk)
    }

    /// The newest [Layout] the describe blocks among `entries` (of the piece at `start`, within `hi`) state.
    fn layout_of(&self, start: u64, entries: &[IndexEntry], hi: u64) -> R<Option<Layout>, S> {
        for e in entries
            .iter()
            .rev()
            .filter(|e| e.kind == BlockKind::Describe)
        {
            let Some(at) = start.checked_add(e.offset) else {
                continue;
            };
            if let Ok((b, _)) = self.good_block(at, hi)?
                && let Some(layout) = crate::walk::layout_in(&b.block())
            {
                return Ok(Some(layout));
            }
        }
        Ok(None)
    }

    /// The newest good index block that starts in `lo..below` and ends within `hi`, where it starts and ends; looked
    /// for backwards [WINDOW] bytes at a time.
    fn newest_index(&self, hi: u64, lo: u64, below: u64) -> R<Option<(u64, OwnedBlock, u64)>, S> {
        // a magic starting just before a window's end reaches into the next window's bytes
        const MAGIC_REST: usize = MAGIC.len() - 1;
        let mut end = below.min(hi);
        while end > lo {
            let from = end.saturating_sub(wide(WINDOW)).max(lo);
            let to = end.saturating_add(wide(MAGIC_REST)).min(hi);
            let win = self.read(from, read_len(to.saturating_sub(from), WINDOW + MAGIC_REST))?;
            let starts = read_len(end.saturating_sub(from), WINDOW);
            for i in (0..starts).rev() {
                if !win.get(i..).is_some_and(|w| w.starts_with(&MAGIC)) {
                    continue;
                }
                let Some(at) = from.checked_add(wide(i)) else {
                    continue;
                };
                let Ok((bytes, end)) = self.block_bytes(at, hi, INDEX_LEN_MAX)? else {
                    continue;
                };
                let index = le_u8(&bytes, 4) == Some(VERSION)
                    && le_u8(&bytes, 5).map(BlockKind::from_u8) == Some(BlockKind::Index);
                if !index {
                    continue;
                }
                if let Ok(b) = OwnedBlock::read(bytes) {
                    return Ok(Some((at, b, end)));
                }
            }
            end = from;
        }
        Ok(None)
    }

    /// Read blocks one after the other from `from` to `hi` into `piece`, starting a new piece at a piece head and
    /// closing one at a tail; stops at the first block that isn't good.
    fn scan(&self, from: u64, hi: u64, piece: PieceInfo) -> R<Vec<PieceInfo>, S> {
        self.scan_with(from, hi, piece, RunState::new(Medium::File))
    }

    /// [Self::scan] keeping to the runs `walk` takes, reading past gaps its layout allows.
    fn scan_with(
        &self,
        from: u64,
        hi: u64,
        mut piece: PieceInfo,
        mut walk: RunState,
    ) -> R<Vec<PieceInfo>, S> {
        let mut out = Vec::new();
        let mut at = from;
        piece.end = piece.end.max(from);
        // offset in the piece of the last block taken that is in the seq order: the one a replacement replaces
        let mut last_counted: Option<u64> = None;
        while at < hi {
            let mut replaces = false;
            let taken = match self.good_block(at, hi)? {
                Ok((b, end)) => match walk.verdict(&b.block(), true) {
                    Verdict::Take => Some((b, end)),
                    // the same block again: dropped
                    Verdict::Duplicate { same: true } => {
                        at = end;
                        piece.end = at;
                        continue;
                    }
                    // other bytes under the same id: the later copy wins
                    Verdict::Duplicate { same: false } => {
                        replaces = true;
                        piece.replaced.push(at);
                        Some((b, end))
                    }
                    Verdict::Stale(_) | Verdict::Backwards { .. } => None,
                },
                Err(_) => None,
            };
            let Some((ob, end)) = taken else {
                if let Some(next) = self.past_gap(at, hi, &walk)?
                    && let Some(len) = next.checked_sub(at)
                {
                    piece.gaps.push((at, len));
                    at = next;
                    continue;
                }
                // on a raw medium blank space longer than a gap is the end, whatever follows it
                let end = match walk.medium {
                    Medium::File => hi,
                    Medium::Raw => at.saturating_add(walk.gap_max()).saturating_add(1).min(hi),
                };
                if !self.blank(at, end)? {
                    piece.cut = Some(at);
                    piece.cut_walk = Some(walk);
                }
                break;
            };
            let block = ob.block();
            walk.take(&block);
            if replaces
                && let Some(gone) = last_counted
                && let Some(list) = piece.entries.as_mut()
            {
                list.retain(|e| e.offset != gone);
            }
            // the piece starts at or before every block of it the scan takes
            let in_piece = at.checked_sub(piece.start);
            if !is_free(&block) {
                last_counted = in_piece;
            }
            match block.kind {
                BlockKind::PieceHead if at != piece.start => {
                    set_scanned(&mut piece, since(at, from));
                    out.push(piece);
                    piece = PieceInfo::new(at, Found::Scan { scanned: 0 });
                    last_counted = None;
                    piece.head = PieceHeadBlock::read(&block)
                        .ok()
                        .map(|h| (h.head(), h.tracks().collect()));
                }
                BlockKind::PieceHead => {
                    piece.head = PieceHeadBlock::read(&block)
                        .ok()
                        .map(|h| (h.head(), h.tracks().collect()));
                }
                BlockKind::Finish => {
                    if let Ok(f) = FinishBlock::read(&block) {
                        piece.finish = Some((f.finish(), f.tracks().collect()));
                        piece.finish_offset = in_piece;
                    }
                }
                BlockKind::Tail => {
                    piece.tail = Tail::read(block.bytes()).ok();
                    piece.end = end;
                    set_scanned(&mut piece, since(end, from));
                    out.push(piece);
                    piece = PieceInfo::new(end, Found::Scan { scanned: 0 });
                    last_counted = None;
                    at = end;
                    continue;
                }
                _ => {
                    if let Some(off) = in_piece
                        && let Some(e) = IndexEntry::of_block(off, &block)
                        && let Some(list) = piece.entries.as_mut()
                    {
                        list.push(e);
                    }
                }
            }
            at = end;
            piece.end = at;
        }
        let scanned = since(piece.end.max(at), from);
        set_scanned(&mut piece, scanned);
        if piece.end > piece.start || piece.cut.is_some() || out.is_empty() {
            out.push(piece);
        }
        Ok(out)
    }

    /// Where the run's data goes on past the blank or bad stretch at `at` within `walk`'s gap rules: the first
    /// non-blank byte or a resume boundary holding a block that [resumes](RunState::resumes) the walk; None when
    /// nothing does (or the layout allows no gaps), so the stretch is the end of data. Each place costs a header
    /// read; at most [GAP_CHECKS_MAX] whose header [may resume](RunState::may_resume) the walk are read whole.
    fn past_gap(&self, at: u64, hi: u64, walk: &RunState) -> R<Option<u64>, S> {
        let reach = walk.gap_max().min(hi.saturating_sub(at));
        if reach == 0 {
            return Ok(None);
        }
        // a block starts with its magic: blank places hold none
        let Some(first) = self.non_blank(at, at.saturating_add(reach))? else {
            return Ok(None);
        };
        let candidates = Some(first)
            .filter(|f| *f > at)
            .into_iter()
            .chain(walk.probes(at, first));
        let mut checks = 0;
        for c in candidates {
            if hi.saturating_sub(c) < wide(HEADER_LEN) {
                break;
            }
            let Some(head) = Head::read(&self.read(c, HEADER_LEN)?) else {
                continue;
            };
            if !walk.may_resume(&head) {
                continue;
            }
            if checks == GAP_CHECKS_MAX {
                break;
            }
            checks = checks.saturating_add(1);
            if let Ok((b, _)) = self.good_block(c, hi)?
                && walk.resumes(&b.block())
            {
                return Ok(Some(c));
            }
        }
        Ok(None)
    }

    /// The first byte from `at` to `end` that isn't blank, read [WINDOW] bytes at a time.
    fn non_blank(&self, mut at: u64, end: u64) -> R<Option<u64>, S> {
        while let Some(left) = end.checked_sub(at).filter(|n| *n > 0) {
            let n = read_len(left, WINDOW);
            let bytes = self.read(at, n)?;
            if let Some(i) = bytes.iter().position(|b| !crate::is_blank(*b)) {
                return Ok(at.checked_add(wide(i)));
            }
            // n is at most left: at stays within end
            at = at.saturating_add(wide(n));
        }
        Ok(None)
    }

    /// Whether everything from `at` to `hi` is blank ([crate::is_blank]: preallocated space, an erased medium): the
    /// end, not damage.
    fn blank(&self, at: u64, hi: u64) -> R<bool, S> {
        Ok(self.non_blank(at, hi)?.is_none())
    }

    /// Every block of piece `i` its index or scan knows, in file order; offsets from the piece's start.
    pub fn entries(&self, i: usize) -> R<Vec<IndexEntry>, S> {
        let p = self.piece(i)?;
        if let Some(e) = &p.entries {
            return Ok(e.clone());
        }
        let mut out = Vec::new();
        self.for_final(p, |ix| {
            out.extend(ix.entries());
        })?;
        out.sort_by_key(|e| e.offset);
        Ok(out)
    }

    /// The blocks of track `track_id` in piece `i` (0: record and describe blocks), in file order. A finished piece
    /// reads only the final index's head and that track's entries.
    pub fn track_entries(&self, i: usize, track_id: u64) -> R<Vec<IndexEntry>, S> {
        let p = self.piece(i)?;
        if let Some(e) = &p.entries {
            return Ok(e
                .iter()
                .filter(|e| e.track_id == track_id)
                .copied()
                .collect());
        }
        let mut out = Vec::new();
        let mut at = p.finish.as_ref().and_then(|(f, _)| f.final_index);
        while let Some(off) = at {
            let abs = in_piece(p, off, HEAD_LEN_PREFIX)?;
            let header = self.read(abs, HEAD_LEN_PREFIX)?;
            let head_len = IndexBlock::head_len(&header).map_err(ReadError::Index)?;
            in_piece(p, off, head_len)?;
            let head = self.read(abs, head_len)?;
            let ix = IndexBlock::read_head(&head).map_err(ReadError::Index)?;
            if ix.kind() != IndexKind::Final || ix.self_offset() != off {
                return Err(ReadError::Index(IndexError::BadHead));
            }
            if let Some(list) = ix.track(track_id) {
                let (from, n) = ix.track_range(&list);
                // a track's entries lie in the index block
                if n > MAX_BLOCK_LEN {
                    return Err(ReadError::Index(IndexError::BadEntries));
                }
                let at = in_piece(p, off.saturating_add(wide(from)), n)?;
                let bytes = self.read(at, n)?;
                out.extend(ix.track_entries(&list, &bytes).map_err(ReadError::Index)?);
            }
            at = ix.prev().filter(|p| *p < off);
        }
        out.sort_by_key(|e| e.offset);
        Ok(out)
    }

    /// The describe set of piece `i`: the first whole set of describe blocks in it (one marked [FLAG_SET_FIRST]
    /// followed back to back by blocks up to one marked [FLAG_SET_LAST]): the head's, or, when one of its blocks is
    /// damaged or missing, the writer's next copy (after the first data block, the repeats, the backup before the
    /// finish). In a piece a scan stopped in, the blocks of its runs past the stop are looked through too
    /// ([DESCRIBE_SEEK_BLOCKS_MAX]). Offsets in the piece and the blocks, in file order; empty when no set reads
    /// whole. Damage is a reason to look further, never an error.
    pub fn describe_set(&self, i: usize) -> R<Vec<(u64, Vec<u8>)>, S> {
        let describe: Vec<IndexEntry> = self
            .track_entries(i, u64::from(crate::DESCRIBE_TRACK))?
            .into_iter()
            .filter(|e| e.kind == BlockKind::Describe)
            .collect();
        let mut buf = Vec::new();
        let mut set = SetReader::default();
        for e in &describe {
            let flags = match self.read_block(i, e, &mut buf) {
                Ok(b) => b.flags,
                Err(ReadError::Block { .. }) => {
                    set.clear();
                    continue;
                }
                Err(other) => return Err(other),
            };
            if set.add(e.offset, flags, &buf) {
                return Ok(set.blocks);
            }
        }
        // a scan stops at the first damaged block, the head's set too: the copy after the first data block is past it
        let p = self.piece(i)?;
        match (p.cut, p.cut_walk) {
            (Some(cut), Some(walk)) => self.describe_past(i, cut, walk),
            _ => Ok(Vec::new()),
        }
    }

    /// The first whole describe set after `cut` in piece `i`, the blocks before it skipped by the lengths in their
    /// headers: at most [DESCRIBE_SEEK_BLOCKS_MAX] blocks, only of the runs `walk` goes on with (never an earlier
    /// life's behind the end of data), never past the piece's stretch or into the next piece.
    fn describe_past(&self, i: usize, cut: u64, mut walk: RunState) -> R<Vec<(u64, Vec<u8>)>, S> {
        let p = self.piece(i)?;
        let hi = match i.checked_add(1).and_then(|j| self.pieces.get(j)) {
            Some(next) => next.start,
            None => self.len,
        };
        let mut set = SetReader::default();
        let mut at = cut;
        for _ in 0..DESCRIBE_SEEK_BLOCKS_MAX {
            if hi.saturating_sub(at) < wide(HEADER_LEN) {
                break;
            }
            let Ok(len) = block_len(&self.read(at, HEADER_LEN)?) else {
                break;
            };
            let Some(end) = at.checked_add(wide(len)).filter(|e| *e <= hi) else {
                break;
            };
            match self.good_block(at, hi)? {
                Ok((b, _)) => {
                    let block = b.block();
                    if !matches!(
                        walk.verdict(&block, true),
                        Verdict::Take | Verdict::Duplicate { .. }
                    ) {
                        break;
                    }
                    walk.take(&block);
                    match block.kind {
                        BlockKind::Describe => {
                            let Some(off) = at.checked_sub(p.start) else {
                                break;
                            };
                            if set.add(off, block.flags, block.bytes()) {
                                return Ok(set.blocks);
                            }
                        }
                        BlockKind::PieceHead | BlockKind::Tail => break,
                        _ => set.clear(),
                    }
                }
                // a set with a damaged block isn't whole
                Err(_) => set.clear(),
            }
            at = end;
        }
        Ok(Vec::new())
    }

    /// Call `f` with each final index block of finished piece `p`, newest first.
    fn for_final(&self, p: &PieceInfo, mut f: impl FnMut(&IndexBlock<'_>)) -> R<(), S> {
        let mut at = p.finish.as_ref().and_then(|(fin, _)| fin.final_index);
        while let Some(off) = at {
            let abs = in_piece(p, off, HEADER_LEN)?;
            let b = match self.good_block(abs, p.end)? {
                Ok((b, _)) => b,
                Err(error) => return Err(ReadError::Block { offset: abs, error }),
            };
            let ix = IndexBlock::read(&b.block()).map_err(ReadError::Index)?;
            if ix.kind() != IndexKind::Final || ix.self_offset() != off {
                return Err(ReadError::Index(IndexError::BadHead));
            }
            f(&ix);
            at = ix.prev().filter(|p| *p < off);
        }
        Ok(())
    }

    /// Read the block of `entry` in piece `i` into `buf` and check it.
    pub fn read_block<'b>(
        &self,
        i: usize,
        entry: &IndexEntry,
        buf: &'b mut Vec<u8>,
    ) -> R<Block<'b>, S> {
        let p = self.piece(i)?;
        let at = p.start.saturating_add(entry.offset);
        // a block is never longer: the entry is damaged
        let Some(len) = usize::try_from(entry.len)
            .ok()
            .filter(|len| *len <= MAX_BLOCK_LEN)
        else {
            return Err(ReadError::Block {
                offset: at,
                error: BlockError::BadLength(entry.len),
            });
        };
        let Ok(offset) = in_piece::<S::Error>(p, entry.offset, len) else {
            return Err(ReadError::Block {
                offset: at,
                error: BlockError::Short { needed: len },
            });
        };
        buf.resize(len, 0);
        self.src.read_at(offset, buf).map_err(ReadError::Source)?;
        Block::read(buf).map_err(|error| ReadError::Block { offset, error })
    }

    /// Whether piece `i`'s bytes before its finish block hash to the finish's content hash.
    pub fn verify(&self, i: usize) -> R<bool, S> {
        let p = self.piece(i)?;
        let (Some((fin, _)), Some(end)) = (&p.finish, p.finish_offset) else {
            return Err(ReadError::Unfinished);
        };
        let mut h = blake3::Hasher::new();
        let mut buf = vec![0u8; VERIFY_STEP];
        let mut at = 0u64;
        while let Some(left) = end.checked_sub(at).filter(|n| *n > 0) {
            let n = read_len(left, VERIFY_STEP);
            let abs = in_piece(p, at, n)?;
            #[expect(
                clippy::indexing_slicing,
                reason = "read_len is at most VERIFY_STEP, the buffer's length"
            )]
            let chunk = &mut buf[..n];
            self.src.read_at(abs, chunk).map_err(ReadError::Source)?;
            h.update(chunk);
            // n is at most left: at stays within end
            at = at.saturating_add(wide(n));
        }
        Ok(*h.finalize().as_bytes() == fin.hash.0)
    }
}

/// A describe set read block by block, see [TapeReader::describe_set].
#[derive(Default)]
struct SetReader {
    blocks: Vec<(u64, Vec<u8>)>,
    /// Where the open set's next block starts
    next: Option<u64>,
}

impl SetReader {
    /// Add the good describe block `bytes` at `offset` with header `flags`; true once the set is whole.
    fn add(&mut self, offset: u64, flags: u16, bytes: &[u8]) -> bool {
        if flags & FLAG_SET_FIRST != 0 {
            self.clear();
        } else if self.blocks.is_empty() || self.next != Some(offset) {
            self.clear();
            return false;
        }
        self.next = offset.checked_add(wide(bytes.len()));
        self.blocks.push((offset, bytes.to_vec()));
        flags & FLAG_SET_LAST != 0
    }

    fn clear(&mut self) {
        self.blocks.clear();
        self.next = None;
    }
}

/// Where `n` bytes at `off` from the start of piece `p` begin in the source, when they lie within the piece.
fn in_piece<E>(p: &PieceInfo, off: u64, n: usize) -> Result<u64, ReadError<E>> {
    p.start
        .checked_add(off)
        .filter(|abs| abs.checked_add(wide(n)).is_some_and(|end| end <= p.end))
        .ok_or(ReadError::Index(IndexError::BadHead))
}

/// How much of `left` bytes fits one read into a `buf`-byte buffer; `left` may pass `usize::MAX` on 32-bit targets.
fn read_len(left: u64, buf: usize) -> usize {
    usize::try_from(left).map_or(buf, |left| left.min(buf))
}

/// Bytes a scan went from `from` to `to`, a place it reached going forward.
fn since(to: u64, from: u64) -> u64 {
    debug_assert!(to >= from, "a scan only goes forward");
    to.saturating_sub(from)
}

fn set_scanned(p: &mut PieceInfo, n: u64) {
    match &mut p.found {
        Found::Checkpoint { scanned } | Found::Scan { scanned } => *scanned = n,
        Found::Tail => {}
    }
}

#[cfg(test)]
mod tests {
    use super::read_len;

    #[test]
    fn read_len_takes_the_buffer_past_4_gib() {
        assert_eq!(read_len((1 << 32) + 5, 4096), 4096);
        assert_eq!(read_len(1 << 32, usize::MAX), (1u64 << 32) as usize);
        assert_eq!(read_len(5, 4096), 5);
        assert_eq!(read_len(u64::MAX, 1 << 20), 1 << 20);
    }
}
