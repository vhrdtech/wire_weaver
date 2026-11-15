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

use crate::index::{HEAD_LEN_PREFIX, INDEX_LEN_MAX, IndexBlock, IndexEntry, IndexError, IndexKind};
use crate::piece::{
    ChainReport, Finish, FinishBlock, PieceHead, PieceHeadBlock, PieceSummary, TAIL_LEN, Tail,
    TrackRange, TrackStart, check_chain,
};
use crate::walk::{RunState, Verdict};
use crate::{
    Block, BlockError, BlockKind, HEADER_LEN, Layout, MAGIC, MAX_BLOCK_LEN, Medium, VERSION,
    block_len,
};

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
        Ok(self.len() as u64)
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
        Ok(self.len() as u64)
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
                    buf = &mut buf[n..];
                    offset += n as u64;
                }
            }
        }
        Ok(())
    }
}

/// Why a tape can't be read.
#[derive(Debug)]
pub enum ReadError<E> {
    /// The byte source failed.
    Source(E),
    /// An index, finish, tail or piece head block is bad where a good one must be.
    Index(IndexError),
    /// The block at `offset` isn't good.
    Block { offset: u64, error: BlockError },
    /// The piece has no finish block, so no content hash.
    Unfinished,
}

impl<E: core::fmt::Debug> core::fmt::Display for ReadError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReadError::Source(e) => write!(f, "reading the bytes: {e:?}"),
            ReadError::Index(e) => write!(f, "index or piece block: {e:?}"),
            ReadError::Block { offset, error } => write!(f, "block at byte {offset}: {error:?}"),
            ReadError::Unfinished => write!(f, "the piece has no finish block"),
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
    /// Entries a scan or the checkpoint chain gave, in file order; None for a piece read by its tail
    entries: Option<Vec<IndexEntry>>,
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
            entries: Some(Vec::new()),
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

    fn read(&self, offset: u64, n: usize) -> R<Vec<u8>, S> {
        let mut buf = vec![0u8; n];
        self.src
            .read_at(offset, &mut buf)
            .map_err(ReadError::Source)?;
        Ok(buf)
    }

    /// The whole block at `offset`, if one is there within `hi` and at most `max` long; not checked yet.
    fn block_bytes(&self, offset: u64, hi: u64, max: usize) -> R<Result<Vec<u8>, BlockError>, S> {
        let room = hi.saturating_sub(offset);
        if room < HEADER_LEN as u64 {
            return Ok(Err(BlockError::Short { needed: HEADER_LEN }));
        }
        let header = self.read(offset, HEADER_LEN)?;
        let len = match block_len(&header) {
            Ok(l) if l <= max => l,
            Ok(l) => return Ok(Err(BlockError::BadLength(l as u32))),
            Err(e) => return Ok(Err(e)),
        };
        if room < len as u64 {
            return Ok(Err(BlockError::Short { needed: len }));
        }
        Ok(Ok(self.read(offset, len)?))
    }

    /// A good block at `offset` within `hi`, or why not.
    fn good_block(&self, offset: u64, hi: u64) -> R<Result<Vec<u8>, BlockError>, S> {
        let bytes = match self.block_bytes(offset, hi, MAX_BLOCK_LEN)? {
            Ok(b) => b,
            Err(e) => return Ok(Err(e)),
        };
        Ok(match Block::read(&bytes) {
            Ok(_) => Ok(bytes),
            Err(e) => Err(e),
        })
    }

    /// The finished piece whose tail ends at `hi`, when its tail and finish are good.
    fn by_tail(&self, hi: u64) -> R<Option<PieceInfo>, S> {
        if hi < TAIL_LEN as u64 {
            return Ok(None);
        }
        let tb = self.read(hi - TAIL_LEN as u64, TAIL_LEN)?;
        let Ok(tail) = Tail::read(&tb) else {
            return Ok(None);
        };
        if tail.piece_len > hi {
            return Ok(None);
        }
        let start = hi - tail.piece_len;
        let fb = match self.good_block(start + tail.finish_offset, hi)? {
            Ok(b) if b.len() == tail.finish_len as usize => b,
            _ => return Ok(None),
        };
        let block = Block::read(&fb).expect("checked");
        let Ok(fin) = FinishBlock::read(&block) else {
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
        let Ok(b) = self.good_block(offset, hi)? else {
            return Ok(None);
        };
        let block = Block::read(&b).expect("checked");
        Ok(PieceHeadBlock::read(&block)
            .ok()
            .map(|h| (h.head(), h.tracks().collect())))
    }

    /// The pieces of the unfinished stretch ending at `hi`: from the newest index block in reach, else scanned from
    /// the start.
    fn recover(&self, hi: u64, options: OpenOptions) -> R<Vec<PieceInfo>, S> {
        let Some((at, bytes)) = self.newest_index(hi, options.scan_back_max)? else {
            return self.scan(0, hi, PieceInfo::new(0, Found::Scan { scanned: 0 }));
        };
        let block = Block::read(&bytes).expect("checked");
        let ix = IndexBlock::read(&block).expect("checked");
        let Some(start) = at.checked_sub(ix.self_offset()) else {
            return self.scan(0, hi, PieceInfo::new(0, Found::Scan { scanned: 0 }));
        };
        let mut piece = PieceInfo::new(start, Found::Checkpoint { scanned: 0 });
        piece.head = self.head_at(start, hi)?;
        let mut lists = vec![ix.entries().collect::<Vec<_>>()];
        let mut prev = ix.prev();
        let mut newest = ix.self_offset();
        let mut chain_whole = true;
        while let Some(off) = prev {
            let ok = off < newest
                && match self.good_block(start + off, at)? {
                    Ok(b) => {
                        let blk = Block::read(&b).expect("checked");
                        match IndexBlock::read(&blk) {
                            Ok(p) if p.kind() == ix.kind() && p.self_offset() == off => {
                                lists.push(p.entries().collect());
                                prev = p.prev();
                                newest = off;
                                true
                            }
                            _ => false,
                        }
                    }
                    Err(_) => false,
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
                start + newest,
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
        piece.entries = Some(entries);
        self.scan(at + bytes.len() as u64, hi, piece)
    }

    /// The newest good index block that ends within `hi`, looked for backwards over at most `max` bytes.
    fn newest_index(&self, hi: u64, max: u64) -> R<Option<(u64, Vec<u8>)>, S> {
        const WINDOW: u64 = 64 * 1024;
        let lo = hi.saturating_sub(max);
        let mut end = hi;
        while end > lo {
            let from = end.saturating_sub(WINDOW).max(lo);
            let to = (end + MAGIC.len() as u64 - 1).min(hi);
            let win = self.read(from, (to - from) as usize)?;
            let mut i = (end - from) as usize;
            while i > 0 {
                i -= 1;
                if win.len() < i + MAGIC.len() || win[i..i + MAGIC.len()] != MAGIC {
                    continue;
                }
                let at = from + i as u64;
                let Ok(bytes) = self.block_bytes(at, hi, INDEX_LEN_MAX)? else {
                    continue;
                };
                if bytes[4] != VERSION || BlockKind::from_u8(bytes[5]) != BlockKind::Index {
                    continue;
                }
                let good = Block::read(&bytes)
                    .ok()
                    .is_some_and(|b| IndexBlock::read(&b).is_ok());
                if good {
                    return Ok(Some((at, bytes)));
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
        let scan_start = from;
        while at < hi {
            let taken = match self.good_block(at, hi)? {
                Ok(b) => {
                    let ok =
                        walk.verdict(&Block::read(&b).expect("checked"), true) == Verdict::Take;
                    ok.then_some(b)
                }
                Err(_) => None,
            };
            let Some(bytes) = taken else {
                if let Some(next) = self.past_gap(at, hi, &walk)? {
                    piece.gaps.push((at, next - at));
                    at = next;
                    continue;
                }
                if !self.blank(at, hi)? {
                    piece.cut = Some(at);
                }
                break;
            };
            let block = Block::read(&bytes).expect("checked");
            walk.take(&block);
            let len = bytes.len() as u64;
            match block.kind {
                BlockKind::PieceHead if at != piece.start => {
                    set_scanned(&mut piece, at - scan_start);
                    out.push(piece);
                    piece = PieceInfo::new(at, Found::Scan { scanned: 0 });
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
                        piece.finish_offset = Some(at - piece.start);
                    }
                }
                BlockKind::Tail => {
                    piece.tail = Tail::read(&bytes).ok();
                    piece.end = at + len;
                    set_scanned(&mut piece, at + len - scan_start);
                    out.push(piece);
                    piece = PieceInfo::new(at + len, Found::Scan { scanned: 0 });
                    at += len;
                    continue;
                }
                _ => {
                    if let Some(e) = IndexEntry::of_block(at - piece.start, &block)
                        && let Some(list) = piece.entries.as_mut()
                    {
                        list.push(e);
                    }
                }
            }
            at += len;
            piece.end = at;
        }
        let n = piece.end.max(at) - scan_start;
        set_scanned(&mut piece, n);
        if piece.end > piece.start || piece.cut.is_some() || out.is_empty() {
            out.push(piece);
        }
        Ok(out)
    }

    /// Where the run's data goes on past the blank or bad stretch at `at` within `walk`'s gap rules: the first
    /// non-blank byte or a resume boundary holding a block that [resumes](RunState::resumes) the walk; None when
    /// nothing does (or the layout allows no gaps), so the stretch is the end of data.
    fn past_gap(&self, at: u64, hi: u64, walk: &RunState) -> R<Option<u64>, S> {
        let reach = walk.gap_max().min(hi.saturating_sub(at));
        if reach == 0 {
            return Ok(None);
        }
        let window = self.read(at, read_len(reach, usize::MAX))?;
        let first = window
            .iter()
            .position(|b| !crate::is_blank(*b))
            .map_or(at.saturating_add(reach), |i| at + i as u64);
        let mut candidates: Vec<u64> = Vec::new();
        if first > at && first < hi {
            candidates.push(first);
        }
        candidates.extend(walk.probes(at, first.max(at)).take_while(|p| *p < hi));
        for c in candidates {
            if let Ok(b) = self.good_block(c, hi)?
                && walk.resumes(&Block::read(&b).expect("checked"))
            {
                return Ok(Some(c));
            }
        }
        Ok(None)
    }

    /// Whether everything from `at` to `hi` is blank ([crate::is_blank]: preallocated space, an erased medium): the
    /// end, not damage.
    fn blank(&self, mut at: u64, hi: u64) -> R<bool, S> {
        while at < hi {
            let n = (hi - at).min(64 * 1024);
            if !self
                .read(at, n as usize)?
                .iter()
                .all(|b| crate::is_blank(*b))
            {
                return Ok(false);
            }
            at += n;
        }
        Ok(true)
    }

    /// Every block of piece `i` its index or scan knows, in file order; offsets from the piece's start.
    pub fn entries(&self, i: usize) -> R<Vec<IndexEntry>, S> {
        let p = &self.pieces[i];
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
        let p = &self.pieces[i];
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
                let at = in_piece(p, off.saturating_add(from as u64), n)?;
                let bytes = self.read(at, n)?;
                out.extend(ix.track_entries(&list, &bytes).map_err(ReadError::Index)?);
            }
            at = ix.prev().filter(|p| *p < off);
        }
        out.sort_by_key(|e| e.offset);
        Ok(out)
    }

    /// Call `f` with each final index block of finished piece `p`, newest first.
    fn for_final(&self, p: &PieceInfo, mut f: impl FnMut(&IndexBlock<'_>)) -> R<(), S> {
        let mut at = p.finish.as_ref().and_then(|(fin, _)| fin.final_index);
        while let Some(off) = at {
            let abs = in_piece(p, off, HEADER_LEN)?;
            let b = match self.good_block(abs, p.end)? {
                Ok(b) => b,
                Err(error) => return Err(ReadError::Block { offset: abs, error }),
            };
            let block = Block::read(&b).expect("checked");
            let ix = IndexBlock::read(&block).map_err(ReadError::Index)?;
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
        let p = &self.pieces[i];
        let len = entry.len as usize;
        let Ok(offset) = in_piece::<S::Error>(p, entry.offset, len) else {
            return Err(ReadError::Block {
                offset: p.start.saturating_add(entry.offset),
                error: BlockError::Short { needed: len },
            });
        };
        buf.resize(entry.len as usize, 0);
        self.src.read_at(offset, buf).map_err(ReadError::Source)?;
        Block::read(buf).map_err(|error| ReadError::Block { offset, error })
    }

    /// Whether piece `i`'s bytes before its finish block hash to the finish's content hash.
    pub fn verify(&self, i: usize) -> R<bool, S> {
        let p = &self.pieces[i];
        let (Some((fin, _)), Some(end)) = (&p.finish, p.finish_offset) else {
            return Err(ReadError::Unfinished);
        };
        let mut h = blake3::Hasher::new();
        let mut at = 0u64;
        let mut buf = vec![0u8; 1 << 20];
        while at < end {
            let n = read_len(end - at, buf.len());
            self.src
                .read_at(p.start + at, &mut buf[..n])
                .map_err(ReadError::Source)?;
            h.update(&buf[..n]);
            at += n as u64;
        }
        Ok(*h.finalize().as_bytes() == fin.hash.0)
    }
}

/// Where `n` bytes at `off` from the start of piece `p` begin in the source, when they lie within the piece.
fn in_piece<E>(p: &PieceInfo, off: u64, n: usize) -> Result<u64, ReadError<E>> {
    p.start
        .checked_add(off)
        .filter(|abs| abs.checked_add(n as u64).is_some_and(|end| end <= p.end))
        .ok_or(ReadError::Index(IndexError::BadHead))
}

/// How much of `left` bytes fits one read into a `buf`-byte buffer; `left` may pass `usize::MAX` on 32-bit targets.
fn read_len(left: u64, buf: usize) -> usize {
    left.min(buf as u64) as usize
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
