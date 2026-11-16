//! Bounds on everything the resolver reads from outside: files of the cache and of Cargo's sources, archives from the
//! network, directories it lists. A file or an answer past a bound is refused and named in the log, never read whole.

use std::time::Duration;

/// A whole-crate snapshot file (`<crate>_<x>_<y>_<z>.ron`). The largest one today, exact_units', is 604 KB.
pub const SNAPSHOT_FILE_BYTES_MAX: u64 = 32 << 20;

/// A superseded definition (`superseded/<...>.ww`): one canonical bundle, a few KB today.
pub const SUPERSEDED_FILE_BYTES_MAX: u64 = ww_self::definition_cache::ENTRY_BYTES_MAX;

/// Entries looked at in one `superseded/` directory.
pub const SUPERSEDED_FILES_MAX: usize = 4096;

/// Depth the RON reader follows into a snapshot file.
pub const RON_RECURSION_LIMIT: usize = 256;

/// Directories looked into under `CARGO_HOME` for one lookup without a lock ([crate::cargo::scan_fetched]).
pub const CARGO_DIRS_SCANNED_MAX: usize = 65_536;

/// Directories looked into in one source tree (a git checkout, a workspace) for its packages, nearest first.
pub const CARGO_TREE_DIRS_MAX: usize = 8192;

/// A `.crate` or a repository archive as received, compressed.
pub const ARCHIVE_BYTES_MAX: u64 = 64 << 20;

/// What is read out of the decompressor of an archive, so a small archive can't unpack without end.
pub const ARCHIVE_UNPACKED_BYTES_MAX: u64 = 256 << 20;

/// Entries looked at in one archive.
pub const ARCHIVE_ENTRIES_MAX: usize = 100_000;

/// Snapshot and superseded files kept in memory out of one archive.
pub const ARCHIVE_FILES_KEPT_MAX: usize = 8192;

/// A `Cargo.toml` read out of an archive, to learn which crate a directory is.
pub const MANIFEST_BYTES_MAX: u64 = 1 << 20;

/// Connecting to a host.
pub const FETCH_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// One fetch, connect to last byte.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Redirects followed: GitHub's archive URL redirects to codeload once.
pub const FETCH_REDIRECTS_MAX: u32 = 3;

/// A fetch that found nothing is remembered in the cache and not tried again before this.
pub const FETCH_RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
