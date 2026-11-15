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
//! with the BLAKE3 content hash of everything before it, and the tail. The newest describe blocks are written again
//! about every [DESCRIBE_REPEAT_BYTES]. [TapeWriter::cut] ends a piece and starts the next one of the chain;
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
use crate::{BlockKind, IndexError};

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
    /// The newest run of describe blocks written back to back, for the head of a next piece, the repeats and the
    /// backup before the finish
    describe: Vec<Vec<u8>>,
    /// The last block placed was a describe block of the newest run
    describe_open: bool,
    /// Describe blocks written again by the writer itself don't start a new run
    replaying: bool,
    /// Bytes placed since the newest describe blocks
    since_describe: u64,
    /// Bytes after which the newest describe blocks are written again
    describe_repeat: u64,
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
            deferred: Vec::new(),
            deferred_bytes: 0,
            chunk_buf: Vec::new(),
            indexer: Indexer::new(IndexPolicy::default()),
            index_due: false,
            last_checkpoint: Instant::now(),
            hash: blake3::Hasher::new(),
            head: None,
            describe: Vec::new(),
            describe_open: false,
            replaying: false,
            since_describe: 0,
            describe_repeat: DESCRIBE_REPEAT_BYTES,
        }
    }

    /// Write the newest describe blocks again after about this many bytes ([DESCRIBE_REPEAT_BYTES] by default).
    pub fn set_describe_repeat(&mut self, bytes: u64) {
        self.describe_repeat = bytes;
    }

    /// When checkpoint index blocks are placed (by default every 4 MiB).
    pub fn set_index_policy(&mut self, policy: IndexPolicy) {
        self.indexer.set_policy(policy);
    }

    /// Start the file as a piece of a chain: the piece head block, first in the file, then (for a piece after the
    /// first) the describe blocks of the piece before, so the piece reads alone.
    pub fn begin_piece(
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
        let n = write_piece_head(&mut buf, self.structural_seq(), head, tracks)
            .map_err(WriteError::Index)?;
        self.write_raw(&buf[..n])?;
        self.head = Some((*head, tracks.to_vec()));
        Ok(())
    }

    /// Where the next block goes, from the start of the piece.
    fn offset(&self) -> u64 {
        self.bytes
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
    /// the file's describe set says so with a [crate::Layout] record.
    ///
    /// # Panics
    ///
    /// When `alignment` is neither 0 nor [crate::ALIGNMENT].
    pub fn set_alignment(&mut self, alignment: u32) {
        assert!(
            alignment == 0 || alignment == crate::ALIGNMENT,
            "alignment is 0 or ALIGNMENT"
        );
        self.alignment = alignment;
    }

    /// Seq the writer gives the next block it places itself ([Self::write_chunk], [Self::defer_block]): one after
    /// the last counted block written. Blocks the file structure adds take the last counted seq and are not counted
    /// ([crate::index::is_structural]).
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Seq of the blocks the file structure adds (index, finish, tail, piece head, repeated describe blocks): the
    /// seq of the last counted block before them, 0 before the first.
    fn structural_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }

    fn write_raw(&mut self, block: &[u8]) -> Result<(), WriteError> {
        self.check()?;
        assert!(block.len() >= crate::HEADER_LEN, "a block holds its header");
        if let Err(error) = self.file.write_all(block) {
            self.failed = true;
            return Err(WriteError::Io {
                what: "writing a block",
                error,
            });
        }
        self.hash.update(block);
        self.indexer.placed(self.bytes, block);
        self.note_describe(block);
        self.blocks += 1;
        self.bytes += block.len() as u64;
        self.since_describe += block.len() as u64;
        self.unsynced += 1;
        // blocks the file structure adds are not counted: seq stays the producer's count of its own blocks
        let counted = !self.replaying && !crate::index::is_structural(BlockKind::from_u8(block[5]));
        if counted && block.len() >= 24 {
            let seq = u64::from_le_bytes(block[16..24].try_into().expect("8 bytes"));
            self.next_seq = seq.wrapping_add(1);
        }
        let due = match self.policy {
            SyncPolicy::None => false,
            SyncPolicy::EveryBlocks(n) => self.unsynced >= n.max(1),
            SyncPolicy::Interval(t) => self.last_sync.elapsed() >= t,
        };
        if due { self.sync() } else { Ok(()) }
    }

    /// Keep the newest run of describe blocks.
    fn note_describe(&mut self, block: &[u8]) {
        let describe = block.len() > 5 && BlockKind::from_u8(block[5]) == BlockKind::Describe;
        if !describe {
            self.describe_open = false;
            return;
        }
        self.since_describe = 0;
        if self.replaying {
            return;
        }
        if !self.describe_open {
            self.describe.clear();
            self.describe_open = true;
        }
        self.describe.push(block.to_vec());
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
        let seq = self.structural_seq();
        let r = blocks.iter().try_for_each(|b| {
            let mut b = b.clone();
            crate::set_seq(&mut b, seq).map_err(WriteError::Block)?;
            self.write_raw(&b)
        });
        self.describe = blocks;
        self.replaying = false;
        self.describe_open = false;
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
        if self.since_describe >= self.describe_repeat && !self.describe_open {
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
            .checkpoint(self.offset(), self.structural_seq())
            .map_err(WriteError::Index)?;
        self.write_raw(&b)
    }

    /// A small block (summary, index) that may wait for the gap in front of the next aligned chunk payload. It is
    /// renumbered when placed ([crate::set_seq]), so seqs ascend in the file. In an unaligned file it is written at
    /// once; past [DEFERRED_BYTES_MAX] waiting, all are.
    pub fn defer_block(&mut self, block: Vec<u8>) -> Result<(), WriteError> {
        self.check()?;
        crate::Block::read(&block).map_err(WriteError::Block)?;
        if self.alignment == 0 {
            return self.place(block);
        }
        self.deferred_bytes += block.len();
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
        crate::set_seq(&mut block, self.next_seq).map_err(WriteError::Block)?;
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
            let mut gap =
                crate::chunk::chunk_padding(self.start + self.bytes, head_len, self.alignment);
            while let Some(next) = self.deferred.first()
                && next.len() <= gap
            {
                let b = self.deferred.remove(0);
                self.deferred_bytes -= b.len();
                gap -= b.len();
                self.place(b)?;
            }
            // a checkpoint goes before the chunk, in the gap when it fits
            if self.index_due {
                self.place_checkpoint()?;
            }
        }
        let padding =
            crate::chunk::chunk_padding(self.start + self.bytes, head_len, self.alignment);
        let specs: Vec<crate::ColumnSpec> = columns.iter().map(|(c, _)| *c).collect();
        let len = crate::chunk::chunk_len(&specs, padding);
        let mut buf = core::mem::take(&mut self.chunk_buf);
        buf.resize(len, 0);
        let r = crate::chunk::write_chunk(&mut buf, self.next_seq, head, columns, padding)
            .map_err(WriteError::Chunk)
            .and_then(|n| self.write_raw(&buf[..n]));
        self.chunk_buf = buf;
        r?;
        self.after_block()
    }

    /// Append one whole block (its bytes as [crate::Block::bytes] gives them, its seq as made), then sync if the
    /// policy says so.
    pub fn write_block(&mut self, block: &[u8]) -> Result<(), WriteError> {
        self.write_raw(block)?;
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
        self.end(state, None).map(|e| e.finished)
    }

    /// End this piece as [FinishState::Continued] and start the next piece of the chain at `next_path`: its id
    /// [PieceId::after] this one's, its head naming this piece, the track starts moved on by this piece's chunks,
    /// then the newest describe blocks. Seqs go on from this piece; policies and alignment carry over.
    pub fn cut(self, next_path: &Path) -> Result<(Finished, TapeWriter), WriteError> {
        let Some((head, starts)) = self.head.clone() else {
            return Err(WriteError::Index(IndexError::Invalid(
                "a cut needs a piece head",
            )));
        };
        let (policy, alignment, index_policy) =
            (self.policy, self.alignment, self.indexer.policy());
        let tracks = self.indexer.next_starts(&starts);
        let describe = self.describe.clone();
        let hash = self.hash.clone().finalize();
        let next_id = PieceId::after(head.ident.id, &ContentHash(*hash.as_bytes()));
        let done = self.end(FinishState::Continued, Some(next_id))?;
        let mut next = TapeWriter::create(next_path, policy)?;
        next.alignment = alignment;
        next.indexer = Indexer::new(index_policy);
        next.next_seq = done.next_seq;
        let ident = PieceIdent {
            id: next_id,
            result: head.ident.result,
            seq: head.ident.seq + 1,
        };
        next.begin_piece(
            &PieceHead {
                ident,
                prev: Some((done.finished.hash, head.ident.id)),
            },
            &tracks,
        )?;
        next.describe = describe;
        next.replay_describe()?;
        Ok((done.finished, next))
    }

    fn end(mut self, state: FinishState, next: Option<PieceId>) -> Result<Ended, WriteError> {
        self.flush_deferred()?;
        let describe = self.replay_describe()?;
        let mut prev = None;
        for part in self.indexer.final_parts() {
            let at = self.offset();
            let b = Indexer::final_block(&part, at, prev, self.structural_seq())
                .map_err(WriteError::Index)?;
            self.write_raw(&b)?;
            prev = Some(at);
        }
        let hash = ContentHash(*self.hash.clone().finalize().as_bytes());
        let ident = self.head.as_ref().map(|(h, _)| h.ident);
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
        let n = write_finish(&mut buf, self.structural_seq(), &finish, &ranges)
            .map_err(WriteError::Index)?;
        let finish_offset = self.offset();
        self.write_raw(&buf[..n])?;
        let tail = Tail {
            piece_len: self.offset() + TAIL_LEN as u64,
            finish_offset,
            finish_len: n as u32,
            state,
            hash,
            ident,
            next,
        };
        let mut tb = [0u8; TAIL_LEN];
        tail.write(&mut tb, self.structural_seq());
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

/// A piece just ended and the seq after its last block.
struct Ended {
    finished: Finished,
    next_seq: u64,
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
