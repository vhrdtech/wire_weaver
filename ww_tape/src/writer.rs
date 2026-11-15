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
//! the file up to the last block that made it. Splitting into several files by size or time, and preallocating space
//! (`fallocate`, read back as the end since zeros never start a block), would go into [TapeWriter::write_block]; neither
//! is built.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
}

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
        }
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
    /// the last block written.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    fn write_raw(&mut self, block: &[u8]) -> Result<(), WriteError> {
        self.check()?;
        if let Err(error) = self.file.write_all(block) {
            self.failed = true;
            return Err(WriteError::Io {
                what: "writing a block",
                error,
            });
        }
        self.blocks += 1;
        self.bytes += block.len() as u64;
        self.unsynced += 1;
        if block.len() >= 24 {
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
        r
    }

    /// Append one whole block (its bytes as [crate::Block::bytes] gives them, its seq as made), then sync if the
    /// policy says so.
    pub fn write_block(&mut self, block: &[u8]) -> Result<(), WriteError> {
        self.write_raw(block)
    }

    /// Sync now if blocks wait and the interval of [SyncPolicy::Interval] is up; call it when no blocks come.
    pub fn tick(&mut self) -> Result<(), WriteError> {
        self.check()?;
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

    /// Write the deferred blocks, sync, give a new file its name and sync the directory; the path of the finished
    /// file.
    pub fn finish(mut self) -> Result<PathBuf, WriteError> {
        self.flush_deferred()?;
        self.sync()?;
        if let Some(partial) = self.partial.take() {
            std::fs::rename(&partial, &self.path).map_err(io_err("renaming the finished file"))?;
            sync_dir(&self.path).map_err(io_err("syncing the directory"))?;
        }
        Ok(self.path)
    }

    fn check(&self) -> Result<(), WriteError> {
        if self.failed {
            Err(WriteError::Failed)
        } else {
            Ok(())
        }
    }
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
