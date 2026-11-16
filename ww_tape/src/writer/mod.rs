//! [TapeWriter]: blocks appended to a file, durable the way databases make their logs durable.
//!
//! - A new file is written as `NAME.partial` and renamed to `NAME` when it is finished; the file and its directory are
//!   synced after it is created and after the rename, so neither the data nor the name can vanish in a crash.
//! - The data is synced by a [SyncPolicy] (none, every N blocks, or at most an interval apart, by default 1 s), like
//!   SQLite's `synchronous` or Postgres' `synchronous_commit`: what a crash can lose is bounded by the policy.
//!   [std::fs::File::sync_data] is `fdatasync` on Linux, `fcntl(F_FULLFSYNC)` on macOS (a plain `fsync` there doesn't
//!   flush the drive's cache) and `FlushFileBuffers` on Windows.
//! - A failed write or sync is the end of the file: the writer stops, reports it and refuses every later write. It
//!   never retries: after a failed `fsync` Linux may drop the dirty pages and a second `fsync` succeeds without them
//!   (Postgres "fsyncgate", 2018).
//!
//! A reader takes the longest valid prefix of a file ([crate::Recovery::Prefix]), so whatever a crash leaves reads as
//! the file up to the last block that made it. Preallocating space (`fallocate`, read back as the end since zeros never
//! start a block) would go into [TapeWriter::write_block]; it is not built.
//!
//! The writer indexes what it places: a checkpoint [index](crate::index) block every few MiB ([IndexPolicy]), so a
//! reader after a crash rescans only the stretch after the newest one. [TapeWriter::finish] ends the file as a
//! [piece](crate::piece): the final index sorted by track, a backup of the newest describe blocks, the finish block
//! with the BLAKE3 content hash of everything before it, and the tail.
//!
//! The newest describe blocks are written again as [copies](crate::FLAG_COPY), so a piece whose head describe set is
//! damaged still reads: once right after the piece's first records or chunk block, then about every
//! [DESCRIBE_REPEAT_BYTES] ([TapeWriter::set_describe_repeat]), and as the backup before the finish. A piece so holds
//! one describe set while it holds no data, two, apart by that block, from its first data on, one more per repeat,
//! and one more once finished; a piece after a cut starts with a copy of the one before's.
//! [TapeWriter::cut] ends a piece and starts the next one of the chain;
//! [crate::PieceChain] cuts by size, chunk count or time.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::index::{IndexPolicy, Indexer};
use crate::piece::{
    ContentHash, Finish, FinishState, PieceHead, PieceId, PieceIdent, TAIL_LEN, Tail, TrackStart,
    finish_len, piece_head_len, write_finish, write_piece_head,
};
use crate::reader::wide;
use crate::{BlockId, BlockKind, IndexError, RunTag};

/// When a [TapeWriter] syncs the data to the disk, besides at [TapeWriter::finish].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPolicy {
    /// Only at the end: a crash may lose everything the OS hadn't written yet.
    None,
    /// After every N blocks.
    EveryBlocks(u32),
    /// When blocks are waiting and the last sync is this long ago, at a block or at [TapeWriter::tick].
    Interval(Duration),
}

impl Default for SyncPolicy {
    fn default() -> Self {
        SyncPolicy::Interval(DEFAULT_SYNC_INTERVAL)
    }
}

/// Default of [SyncPolicy::Interval].
pub const DEFAULT_SYNC_INTERVAL: Duration = Duration::from_secs(1);

/// Why a [TapeWriter] stopped.
#[derive(Debug)]
#[non_exhaustive]
pub enum WriteError {
    /// Opening, writing, syncing or renaming failed, with what was being done.
    Io {
        what: &'static str,
        error: io::Error,
    },
    /// An earlier write or sync failed (said then); the file takes no more blocks.
    Failed,
    /// `create` of a file that is already there.
    Exists(PathBuf),
    /// A deferred block isn't a good block.
    Block(crate::BlockError),
    /// A chunk doesn't encode.
    Chunk(crate::ChunkError),
    /// An alignment that is neither 0 nor [crate::ALIGNMENT].
    Alignment(u32),
    /// A run tag a writer may not use ([RunTag::is_run]).
    Run(RunTag),
    /// An index, finish, tail or piece head block doesn't encode, or a piece call came out of order (a piece head
    /// after other blocks, a cut of a file without a piece head).
    Index(IndexError),
}

impl core::fmt::Display for WriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WriteError::Io { what, error } => write!(f, "{what}: {error}"),
            WriteError::Failed => write!(
                f,
                "an earlier write or sync failed, the file takes no more blocks"
            ),
            WriteError::Exists(p) => write!(f, "{} exists", p.display()),
            WriteError::Block(e) => write!(f, "not a good block: {e:?}"),
            WriteError::Chunk(e) => write!(f, "chunk doesn't encode: {e:?}"),
            WriteError::Index(e) => write!(f, "index or piece block: {e:?}"),
            WriteError::Alignment(a) => {
                write!(f, "alignment {a} is neither 0 nor {}", crate::ALIGNMENT)
            }
            WriteError::Run(r) => write!(f, "run tag {r:?} is not one a writer may use"),
        }
    }
}

impl std::error::Error for WriteError {}

/// Appends blocks to a file, see the [module docs](self).
pub struct TapeWriter {
    file: File,
    path: PathBuf,
    /// Some while the file is written under its `.partial` name
    partial: Option<PathBuf>,
    policy: SyncPolicy,
    unsynced: u32,
    last_sync: Instant,
    failed: bool,
    blocks: u64,
    bytes: u64,
    /// Length of the file before this writer appended to it
    start: u64,
    /// 0 or [crate::ALIGNMENT]: where chunk payloads go
    alignment: u32,
    /// Seq of the next block placed by the writer: after the last block written
    next_seq: u64,
    /// Run of the blocks the writer makes: its own, or the run of the last counted block written
    run: RunTag,
    /// Small blocks waiting for the gap before an aligned chunk payload
    deferred: Vec<Vec<u8>>,
    deferred_bytes: usize,
    /// Reused for encoding chunk blocks
    chunk_buf: Vec<u8>,
    /// What is placed in the piece, for its checkpoints, final index and finish
    indexer: Indexer,
    /// A checkpoint waits for the gap before the next aligned chunk
    index_due: bool,
    last_checkpoint: Instant,
    /// BLAKE3 of every byte of the piece so far
    hash: blake3::Hasher,
    /// The piece head written first, and its track starts
    head: Option<(PieceHead, Vec<TrackStart>)>,
    /// The newest whole describe set (first to last mark), for the head of a next piece, the repeats and the backup
    /// before the finish
    describe: Vec<Vec<u8>>,
    /// The blocks of a describe set written so far, not yet up to its last block: never copied
    describe_open: Vec<Vec<u8>>,
    /// Describe blocks written again by the writer itself don't start a new run
    replaying: bool,
    /// Bytes placed since the newest describe blocks
    since_describe: u64,
    /// Bytes after which the newest describe blocks are written again
    describe_repeat: u64,
    /// The describe copy after the piece's first records or chunk block is still to be written
    data_copy_due: bool,
    /// A records or chunk block was placed while that copy is due
    data_placed: bool,
    /// The last counted block written: its id, length, CRC and offset, to tell a block written again
    last_counted: Option<(BlockId, usize, [u8; 4], u64)>,
    /// Id of the last block [Self::adopt_block] took, as its producer numbered it
    adopted: Option<BlockId>,
}

/// What a finished piece is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub path: PathBuf,
    /// BLAKE3 of the piece's bytes before its finish block.
    pub hash: ContentHash,
    /// Bytes of the piece, its tail included.
    pub len: u64,
    pub ident: Option<PieceIdent>,
    /// The next piece's id when the piece was [cut](TapeWriter::cut).
    pub next: Option<PieceId>,
}

/// About how many bytes apart a writer writes the newest describe blocks again: 128 MiB.
pub const DESCRIBE_REPEAT_BYTES: u64 = 128 << 20;

/// Most bytes of deferred blocks a [TapeWriter] keeps before writing them out anyway.
pub const DEFERRED_BYTES_MAX: usize = 1 << 20;

impl TapeWriter {
    /// A new file at `path`, written as `path.partial` until [Self::finish]; fails if `path` is there.
    pub fn create(path: &Path, policy: SyncPolicy) -> Result<Self, WriteError> {
        if path.exists() {
            return Err(WriteError::Exists(path.to_path_buf()));
        }
        let partial = partial_name(path);
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&partial)
            .map_err(io_err("creating the file"))?;
        let w = Self::new(file, path, Some(partial), policy);
        w.file.sync_all().map_err(io_err("syncing the new file"))?;
        sync_dir(path).map_err(io_err("syncing the directory"))?;
        Ok(w)
    }

    /// Append to the file at `path` in place, created if it isn't there. The blocks of another producer run start
    /// with a describe block, which a reader takes as the next part of the file.
    pub fn append(path: &Path, policy: SyncPolicy) -> Result<Self, WriteError> {
        let existed = path.exists();
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .map_err(io_err("opening the file"))?;
        let mut w = Self::new(file, path, None, policy);
        w.start = w
            .file
            .metadata()
            .map_err(io_err("reading the file's length"))?
            .len();
        if !existed {
            w.file.sync_all().map_err(io_err("syncing the new file"))?;
            sync_dir(path).map_err(io_err("syncing the directory"))?;
        }
        Ok(w)
    }

    fn new(file: File, path: &Path, partial: Option<PathBuf>, policy: SyncPolicy) -> Self {
        TapeWriter {
            file,
            path: path.to_path_buf(),
            partial,
            policy,
            unsynced: 0,
            last_sync: Instant::now(),
            failed: false,
            blocks: 0,
            bytes: 0,
            start: 0,
            alignment: 0,
            next_seq: 0,
            run: RunTag::random(),
            deferred: Vec::new(),
            deferred_bytes: 0,
            chunk_buf: Vec::new(),
            indexer: Indexer::new(IndexPolicy::default()),
            index_due: false,
            last_checkpoint: Instant::now(),
            hash: blake3::Hasher::new(),
            head: None,
            describe: Vec::new(),
            describe_open: Vec::new(),
            replaying: false,
            since_describe: 0,
            describe_repeat: DESCRIBE_REPEAT_BYTES,
            data_copy_due: true,
            data_placed: false,
            last_counted: None,
            adopted: None,
        }
    }

    /// Write the newest describe blocks again after about this many bytes ([DESCRIBE_REPEAT_BYTES] by default; 512 MiB
    /// or 1 GiB trade copies for space). It carries over to the next piece of a [cut](Self::cut).
    pub fn set_describe_repeat(&mut self, bytes: u64) {
        self.describe_repeat = bytes;
    }

    /// When checkpoint index blocks are placed (by default every 4 MiB).
    pub fn set_index_policy(&mut self, policy: IndexPolicy) {
        self.indexer.set_policy(policy);
    }

    /// Start the file as a piece of a chain: the piece head block, first in the file, then (for a piece after the
    /// first) the describe blocks of the piece before, so the piece reads alone. The run of a chain's first piece is
    /// [RunTag::of_piece] its id, so the same input gives the same bytes.
    pub fn begin_piece(
        &mut self,
        head: &PieceHead,
        tracks: &[TrackStart],
    ) -> Result<(), WriteError> {
        if head.prev.is_none() {
            self.run = RunTag::of_piece(&head.ident.id);
        }
        self.write_piece_head(head, tracks)
    }

    fn write_piece_head(
        &mut self,
        head: &PieceHead,
        tracks: &[TrackStart],
    ) -> Result<(), WriteError> {
        self.check()?;
        if self.bytes != 0 {
            return Err(WriteError::Index(IndexError::Invalid(
                "a piece head is the first block of a file",
            )));
        }
        let mut buf = vec![0u8; piece_head_len(tracks.len())];
        let n = write_piece_head(&mut buf, self.structural_id(), head, tracks)
            .map_err(WriteError::Index)?;
        buf.truncate(n);
        self.write_raw(&buf)?;
        self.head = Some((*head, tracks.to_vec()));
        Ok(())
    }

    /// Where the next block goes, from the start of the piece.
    fn offset(&self) -> u64 {
        self.bytes
    }

    /// Where the next block goes, from the start of the file.
    fn at(&self) -> Result<u64, WriteError> {
        self.start
            .checked_add(self.bytes)
            .ok_or(WriteError::Index(IndexError::Invalid(
                "a file past u64::MAX bytes",
            )))
    }

    /// Where the file is while it is written (`NAME.partial` for a new file).
    pub fn current_path(&self) -> &Path {
        self.partial.as_deref().unwrap_or(&self.path)
    }

    pub fn blocks(&self) -> u64 {
        self.blocks
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Place chunk payloads on multiples of `alignment` (0 or [crate::ALIGNMENT]), counted from the start of the file;
    /// the file's describe set says so with a [crate::Layout] record. Another alignment is refused.
    pub fn set_alignment(&mut self, alignment: u32) -> Result<(), WriteError> {
        if alignment != 0 && alignment != crate::ALIGNMENT {
            return Err(WriteError::Alignment(alignment));
        }
        self.alignment = alignment;
        Ok(())
    }

    /// Seq the writer gives the next block it places itself ([Self::write_chunk], [Self::defer_block]): one after
    /// the last counted block written. Blocks the file structure adds take the last counted seq and are not counted
    /// ([crate::index::is_structural]).
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Run and seq of the next block the writer places itself, see [Self::next_seq]: what a caller gives a block it
    /// makes for [Self::write_block].
    pub fn next_id(&self) -> BlockId {
        BlockId::new(self.run, self.next_seq)
    }

    /// The run of the blocks the writer makes: random for a new writer, [RunTag::of_piece] for a chain's first piece,
    /// else the run of the last counted block written.
    pub fn run(&self) -> RunTag {
        self.run
    }

    /// Make the blocks from here on part of run `run` (a deterministic producer's own tag), its seqs from 0. A tag
    /// a writer may not use ([RunTag::is_run]) is refused.
    pub fn set_run(&mut self, run: RunTag) -> Result<(), WriteError> {
        if !run.is_run() {
            return Err(WriteError::Run(run));
        }
        self.run = run;
        self.next_seq = 0;
        Ok(())
    }

    /// Id of the blocks the file structure adds (index, finish, tail, piece head, repeated describe blocks): the
    /// seq of the last counted block before them, 0 before the first.
    fn structural_id(&self) -> BlockId {
        BlockId::new(self.run, self.next_seq.saturating_sub(1))
    }

    fn write_raw(&mut self, block: &[u8]) -> Result<(), WriteError> {
        self.check()?;
        let (Some(header), Some(crc)) = (
            block.first_chunk::<{ crate::HEADER_LEN }>(),
            block.last_chunk::<{ crate::CRC_LEN }>(),
        ) else {
            return Err(WriteError::Block(crate::BlockError::Short {
                needed: crate::HEADER_LEN,
            }));
        };
        let len = wide(block.len());
        let bytes = self
            .bytes
            .checked_add(len)
            .ok_or(WriteError::Index(IndexError::Invalid(
                "a file past u64::MAX bytes",
            )))?;
        if let Err(error) = self.file.write_all(block) {
            self.failed = true;
            return Err(WriteError::Io {
                what: "writing a block",
                error,
            });
        }
        self.hash.update(block);
        // blocks the file structure adds are not counted: seq stays the producer's count of its own blocks
        let kind = BlockKind::from_u8(header[5]);
        let counted = !self.replaying && !crate::index::is_structural(kind);
        self.index(block, *crc, counted);
        self.note_describe(block, header);
        self.bytes = bytes;
        // counts against thresholds: past u64::MAX they stay due
        self.blocks = self.blocks.saturating_add(1);
        self.since_describe = self.since_describe.saturating_add(len);
        self.unsynced = self.unsynced.saturating_add(1);
        if counted
            && matches!(
                kind,
                BlockKind::Records | BlockKind::Events | BlockKind::Series
            )
        {
            self.data_placed = true;
        }
        if counted && let Some(id) = block_id(block) {
            self.run = id.run;
            self.next_seq = id.seq.wrapping_add(1);
        }
        let due = match self.policy {
            SyncPolicy::None => false,
            SyncPolicy::EveryBlocks(n) => self.unsynced >= n.max(1),
            SyncPolicy::Interval(t) => self.last_sync.elapsed() >= t,
        };
        if due { self.sync() } else { Ok(()) }
    }

    /// List `block`, placed here, in the index as readers take it: a counted block with the id of the last one is
    /// that block again, unlisted with the same bytes, in its place with other bytes.
    fn index(&mut self, block: &[u8], crc: [u8; crate::CRC_LEN], counted: bool) {
        let id = block_id(block);
        let again = self
            .last_counted
            .filter(|(last, ..)| counted && id == Some(*last));
        match again {
            Some((_, len, c, _)) if len == block.len() && c == crc => {
                self.indexer.placed_unlisted(block.len());
            }
            Some((.., offset)) => {
                self.indexer.replaced(offset);
                self.indexer.placed(self.bytes, block);
            }
            None => self.indexer.placed(self.bytes, block),
        }
        if counted && let Some(id) = id {
            self.last_counted = Some((id, block.len(), crc, self.bytes));
        }
    }

    /// Keep the newest whole describe set: blocks from one marked [crate::FLAG_SET_FIRST] up to one marked
    /// [crate::FLAG_SET_LAST], back to back. A set another block interrupts is dropped.
    fn note_describe(&mut self, block: &[u8], header: &[u8; crate::HEADER_LEN]) {
        if BlockKind::from_u8(header[5]) != BlockKind::Describe {
            self.describe_open.clear();
            return;
        }
        self.since_describe = 0;
        if self.replaying {
            return;
        }
        let flags = u16::from_le_bytes([header[6], header[7]]);
        if flags & crate::FLAG_SET_FIRST != 0 {
            self.describe_open.clear();
        } else if self.describe_open.is_empty() {
            return;
        }
        self.describe_open.push(block.to_vec());
        if flags & crate::FLAG_SET_LAST != 0 {
            self.describe = core::mem::take(&mut self.describe_open);
        }
    }

    /// Write the newest describe blocks again with the seq of the last counted block, not counted themselves; the
    /// offset of the first. None when there are none.
    fn replay_describe(&mut self) -> Result<Option<u64>, WriteError> {
        if self.describe.is_empty() {
            return Ok(None);
        }
        let at = self.offset();
        self.replaying = true;
        let blocks = core::mem::take(&mut self.describe);
        let id = self.structural_id();
        let r = blocks.iter().try_for_each(|b| {
            let mut b = b.clone();
            crate::mark_copy(&mut b, id).map_err(WriteError::Block)?;
            self.write_raw(&b)
        });
        self.describe = blocks;
        self.replaying = false;
        r.map(|()| Some(at))
    }

    /// After a block placed by the caller: a checkpoint or the describe repeat when due.
    fn after_block(&mut self) -> Result<(), WriteError> {
        let by_time = self
            .indexer
            .policy()
            .interval
            .is_some_and(|t| self.indexer.pending() > 0 && self.last_checkpoint.elapsed() >= t);
        if self.indexer.due() || by_time {
            if self.alignment == 0 {
                self.place_checkpoint()?;
            } else {
                self.index_due = true;
            }
        }
        // a second describe set as soon as the piece holds data, so a damaged head set doesn't make it unreadable;
        // never inside a set being written, which would split it
        let open = !self.describe_open.is_empty();
        let early = self.data_copy_due && self.data_placed && !self.describe.is_empty() && !open;
        if early || (self.since_describe >= self.describe_repeat && !open) {
            if early {
                self.data_copy_due = false;
            }
            self.replay_describe()?;
        }
        Ok(())
    }

    fn place_checkpoint(&mut self) -> Result<(), WriteError> {
        self.index_due = false;
        self.last_checkpoint = Instant::now();
        if self.indexer.pending() == 0 {
            return Ok(());
        }
        let b = self
            .indexer
            .checkpoint(self.offset(), self.structural_id())
            .map_err(WriteError::Index)?;
        self.write_raw(&b)
    }

    /// A small summary or data block that may wait for the gap in front of the next aligned chunk payload. It is
    /// renumbered into the writer's run when placed ([crate::set_id]), so seqs ascend in the file. In an unaligned file it is written at
    /// once; past [DEFERRED_BYTES_MAX] waiting, all are. Index, finish, tail and piece head blocks are the writer's
    /// own: refused with [WriteError::Index].
    pub fn defer_block(&mut self, block: Vec<u8>) -> Result<(), WriteError> {
        self.check()?;
        let b = crate::Block::read(&block).map_err(WriteError::Block)?;
        if crate::index::is_structural(b.kind) {
            return Err(WriteError::Index(IndexError::Invalid(
                "index, finish, tail and piece head blocks are written by the writer",
            )));
        }
        if self.alignment == 0 {
            return self.place(block);
        }
        // flushed once over DEFERRED_BYTES_MAX: never near usize::MAX
        self.deferred_bytes = self.deferred_bytes.saturating_add(block.len());
        self.deferred.push(block);
        if self.deferred_bytes > DEFERRED_BYTES_MAX {
            self.flush_deferred()?;
        }
        Ok(())
    }

    /// Write every deferred block now.
    pub fn flush_deferred(&mut self) -> Result<(), WriteError> {
        for b in core::mem::take(&mut self.deferred) {
            self.place(b)?;
        }
        self.deferred_bytes = 0;
        Ok(())
    }

    fn place(&mut self, mut block: Vec<u8>) -> Result<(), WriteError> {
        crate::set_id(&mut block, self.next_id()).map_err(WriteError::Block)?;
        self.write_raw(&block)
    }

    /// Encode and append a chunk block numbered [Self::next_seq]. In an aligned file its payload lands on a multiple
    /// of the alignment: deferred blocks fill the gap in front of it while they fit, in the order they came, the
    /// rest of the gap is zeros in its head.
    pub fn write_chunk(
        &mut self,
        head: &crate::ChunkHead,
        columns: &[(crate::ColumnSpec, &[u8])],
    ) -> Result<(), WriteError> {
        self.check()?;
        let payload: usize = columns.iter().map(|(c, _)| c.len).sum();
        let head_len = crate::chunk::chunk_head_len(columns.len(), payload);
        if self.alignment != 0 {
            let mut gap = crate::chunk::chunk_padding(self.at()?, head_len, self.alignment)
                .ok_or(WriteError::Alignment(self.alignment))?;
            while let Some(rest) = self
                .deferred
                .first()
                .and_then(|next| gap.checked_sub(next.len()))
            {
                let b = self.deferred.remove(0);
                // the sum of the deferred blocks' lengths
                self.deferred_bytes = self.deferred_bytes.saturating_sub(b.len());
                gap = rest;
                self.place(b)?;
            }
            // a checkpoint goes before the chunk, in the gap when it fits
            if self.index_due {
                self.place_checkpoint()?;
            }
        }
        let padding = crate::chunk::chunk_padding(self.at()?, head_len, self.alignment)
            .ok_or(WriteError::Alignment(self.alignment))?;
        let specs: Vec<crate::ColumnSpec> = columns.iter().map(|(c, _)| *c).collect();
        let len = crate::chunk::chunk_len(&specs, padding);
        let mut buf = core::mem::take(&mut self.chunk_buf);
        buf.resize(len, 0);
        let r = crate::chunk::write_chunk(&mut buf, self.next_id(), head, columns, padding)
            .map_err(WriteError::Chunk)
            .and_then(|n| {
                buf.truncate(n);
                self.write_raw(&buf)
            });
        self.chunk_buf = buf;
        r?;
        self.after_block()
    }

    /// Append one whole block (its bytes as [crate::Block::bytes] gives them, its run and seq as made, see
    /// [Self::next_id]), then sync if the policy says so. A block of another run starts that run: the blocks the
    /// writer makes from there on are in it.
    pub fn write_block(&mut self, block: &[u8]) -> Result<(), WriteError> {
        self.write_raw(block)?;
        self.after_block()
    }

    /// Append a block of any run as the writer's own: renumbered into the writer's run and next seq ([crate::set_id]),
    /// then as [Self::write_block]. On a raw medium a run starts only at a piece head naming the run before it, so a
    /// piece there is one run: a writer for raw media passes a producer's blocks (a tap's, of its own run) this way. A
    /// block passed twice gets two seqs, both data. Index, finish, tail and piece head blocks are the writer's own:
    /// refused with [WriteError::Index].
    ///
    /// Lost blocks stay visible: a jump in the producer's seq (a subscriber that fell behind) leaves the same jump in
    /// the writer's, so a reader of the adopted run sees the gap. A block of another producer run is a restart, with
    /// no gap.
    pub fn adopt_block(&mut self, block: &[u8]) -> Result<(), WriteError> {
        let b = crate::Block::read(block).map_err(WriteError::Block)?;
        let (kind, theirs) = (b.kind, BlockId::new(b.run, b.seq));
        if crate::index::is_structural(kind) {
            return Err(WriteError::Index(IndexError::Invalid(
                "index, finish, tail and piece head blocks are written by the writer",
            )));
        }
        let free = b.is_copy();
        if !free {
            if let Some(last) = self.adopted
                && last.run == theirs.run
            {
                let lost = theirs.seq.saturating_sub(last.seq).saturating_sub(1);
                self.next_seq = self.next_seq.wrapping_add(lost);
            }
            self.adopted = Some(theirs);
        }
        let mut own = block.to_vec();
        crate::set_id(&mut own, self.next_id()).map_err(WriteError::Block)?;
        self.write_raw(&own)?;
        self.after_block()
    }

    /// Sync now if blocks wait and the interval of [SyncPolicy::Interval] is up; call it when no blocks come.
    pub fn tick(&mut self) -> Result<(), WriteError> {
        self.check()?;
        let by_time = self
            .indexer
            .policy()
            .interval
            .is_some_and(|t| self.indexer.pending() > 0 && self.last_checkpoint.elapsed() >= t);
        if by_time && self.alignment == 0 {
            self.place_checkpoint()?;
        } else if by_time {
            self.index_due = true;
        }
        match self.policy {
            SyncPolicy::Interval(t) if self.unsynced > 0 && self.last_sync.elapsed() >= t => {
                self.sync()
            }
            _ => Ok(()),
        }
    }

    /// Sync the data written so far to the disk.
    pub fn sync(&mut self) -> Result<(), WriteError> {
        self.check()?;
        if let Err(error) = self.file.sync_data() {
            // never retried, see the module docs
            self.failed = true;
            return Err(WriteError::Io {
                what: "syncing the file",
                error,
            });
        }
        self.unsynced = 0;
        self.last_sync = Instant::now();
        Ok(())
    }

    /// End the file as a complete piece ([FinishState::Complete]) and sync it; a new file gets its name, and its
    /// directory is synced.
    pub fn finish(self) -> Result<Finished, WriteError> {
        self.finish_as(FinishState::Complete)
    }

    /// End the file as a piece in `state`: the deferred blocks, the final index, the describe backup, the finish
    /// and the tail; then sync, rename and sync the directory as [Self::finish].
    pub fn finish_as(self, state: FinishState) -> Result<Finished, WriteError> {
        if state == FinishState::Continued {
            return Err(WriteError::Index(IndexError::Invalid(
                "a continued piece ends by cut",
            )));
        }
        self.end(state).map(|e| e.finished)
    }

    /// End this piece as [FinishState::Continued] and start the next piece of the chain at `next_path`: its id
    /// [PieceId::after] this one's id and final content hash (the finish's), its head naming this piece, the track starts moved on by this piece's chunks,
    /// then the newest describe blocks. Seqs go on from this piece; policies, alignment and the describe repeat carry over.
    pub fn cut(self, next_path: &Path) -> Result<(Finished, TapeWriter), WriteError> {
        let Some((head, starts)) = self.head.clone() else {
            return Err(WriteError::Index(IndexError::Invalid(
                "a cut needs a piece head",
            )));
        };
        let (policy, alignment, index_policy, describe_repeat) = (
            self.policy,
            self.alignment,
            self.indexer.policy(),
            self.describe_repeat,
        );
        let seq = head
            .ident
            .seq
            .checked_add(1)
            .ok_or(WriteError::Index(IndexError::Invalid(
                "piece seq past u64::MAX",
            )))?;
        let tracks = self.indexer.next_starts(&starts);
        let describe = self.describe.clone();
        let done = self.end(FinishState::Continued)?;
        // what end names in the finish and tail of a continued piece with a head
        let next_id = PieceId::after(head.ident.id, &done.finished.hash);
        let mut next = TapeWriter::create(next_path, policy)?;
        next.alignment = alignment;
        next.indexer = Indexer::new(index_policy);
        next.describe_repeat = describe_repeat;
        next.next_seq = done.next_seq;
        next.run = done.run;
        let ident = PieceIdent {
            id: next_id,
            result: head.ident.result,
            seq,
        };
        next.write_piece_head(
            &PieceHead {
                ident,
                prev: Some((done.finished.hash, head.ident.id)),
                prev_run: done.run,
            },
            &tracks,
        )?;
        next.describe = describe;
        next.replay_describe()?;
        Ok((done.finished, next))
    }

    /// End the piece in `state`; a continued piece with a head names the next piece's id, [PieceId::after] its own
    /// and the content hash in its finish.
    fn end(mut self, state: FinishState) -> Result<Ended, WriteError> {
        self.flush_deferred()?;
        let describe = self.replay_describe()?;
        let mut prev = None;
        for part in self.indexer.final_parts() {
            let at = self.offset();
            let b = Indexer::final_block(&part, at, prev, self.structural_id())
                .map_err(WriteError::Index)?;
            self.write_raw(&b)?;
            prev = Some(at);
        }
        let hash = ContentHash(*self.hash.clone().finalize().as_bytes());
        let ident = self.head.as_ref().map(|(h, _)| h.ident);
        let next = ident
            .filter(|_| state == FinishState::Continued)
            .map(|i| PieceId::after(i.id, &hash));
        let finish = Finish {
            state,
            hash,
            checkpoint: self.indexer.last_checkpoint(),
            final_index: prev,
            describe,
            ident,
            next,
        };
        let ranges = self.indexer.track_ranges();
        let mut buf = vec![0u8; finish_len(ranges.len())];
        let n = write_finish(&mut buf, self.structural_id(), &finish, &ranges)
            .map_err(WriteError::Index)?;
        let finish_offset = self.offset();
        buf.truncate(n);
        self.write_raw(&buf)?;
        let too_long = WriteError::Index(IndexError::Invalid("a piece past u64::MAX bytes"));
        let tail = Tail {
            piece_len: self.offset().checked_add(wide(TAIL_LEN)).ok_or(too_long)?,
            finish_offset,
            finish_len: u32::try_from(n)
                .ok()
                .ok_or(WriteError::Index(IndexError::Invalid(
                    "finish longer than a block",
                )))?,
            state,
            hash,
            ident,
            next,
        };
        let mut tb = [0u8; TAIL_LEN];
        tail.write(&mut tb, self.structural_id());
        self.write_raw(&tb)?;
        self.sync()?;
        if let Some(partial) = self.partial.take() {
            std::fs::rename(&partial, &self.path).map_err(io_err("renaming the finished file"))?;
            sync_dir(&self.path).map_err(io_err("syncing the directory"))?;
        }
        Ok(Ended {
            finished: Finished {
                path: self.path.clone(),
                hash,
                len: self.bytes,
                ident,
                next,
            },
            next_seq: self.next_seq,
            run: self.run,
        })
    }

    /// The index entries a checkpoint would list now, for tests and tools.
    pub fn pending_entries(&self) -> usize {
        self.indexer.pending()
    }

    fn check(&self) -> Result<(), WriteError> {
        if self.failed {
            Err(WriteError::Failed)
        } else {
            Ok(())
        }
    }
}

/// A piece just ended, and the run and seq after its last block.
struct Ended {
    finished: Finished,
    next_seq: u64,
    run: RunTag,
}

/// Run and seq of the block whose bytes start `block`, None when it is shorter than a header.
fn block_id(block: &[u8]) -> Option<BlockId> {
    let seq = u64::from_le_bytes(block.get(16..24)?.try_into().ok()?);
    let run = u32::from_le_bytes(block.get(24..28)?.try_into().ok()?);
    Some(BlockId::new(RunTag::from_u32(run), seq))
}

/// `NAME.partial` next to `NAME`.
pub fn partial_name(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".partial");
    PathBuf::from(name)
}

fn io_err(what: &'static str) -> impl FnOnce(io::Error) -> WriteError {
    move |error| WriteError::Io { what, error }
}

/// Sync the directory a file is in, so a new or renamed name survives a crash. Windows has no such call (NTFS
/// journals names itself).
fn sync_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let dir = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d,
            _ => Path::new("."),
        };
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
