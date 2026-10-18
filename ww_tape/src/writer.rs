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
}

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
        let w = Self::new(file, path, None, policy);
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

    /// Append one whole block (its bytes as [crate::Block::bytes] gives them), then sync if the policy says so.
    pub fn write_block(&mut self, block: &[u8]) -> Result<(), WriteError> {
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
        let due = match self.policy {
            SyncPolicy::None => false,
            SyncPolicy::EveryBlocks(n) => self.unsynced >= n.max(1),
            SyncPolicy::Interval(t) => self.last_sync.elapsed() >= t,
        };
        if due { self.sync() } else { Ok(()) }
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

    /// Sync, give a new file its name and sync the directory; the path of the finished file.
    pub fn finish(mut self) -> Result<PathBuf, WriteError> {
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
