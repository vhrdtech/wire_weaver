//! What a walk read from disk, so a [CrateCache](super::CrateCache) entry built from it is used only while all of it
//! is unchanged.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A file or directory as it was when looked at: its size and modification time, `None` when it didn't exist. A
/// directory's time changes when an entry is added to it or removed, which is what a walk that lists it or checks
/// whether a file is there depends on.
pub(crate) type Stamp = Option<(u64, Option<SystemTime>)>;

pub(crate) fn stamp(path: &Path) -> Stamp {
    fs::metadata(path)
        .ok()
        .map(|meta| (meta.len(), meta.modified().ok()))
}

/// Files and directories a walk depended on, each with its [Stamp] when first looked at.
#[derive(Clone, Debug, Default)]
pub(crate) struct Reads(HashMap<PathBuf, Stamp>);

impl Reads {
    /// Whether everything is as it was when read.
    pub(crate) fn unchanged(&self) -> bool {
        self.0.iter().all(|(path, was)| stamp(path) == *was)
    }

    pub(crate) fn extend(&mut self, other: Reads) {
        for (path, was) in other.0 {
            self.0.entry(path).or_insert(was);
        }
    }
}

thread_local! {
    /// The reads of the walks running on this thread, innermost last.
    static RECORDING: RefCell<Vec<Reads>> = const { RefCell::new(Vec::new()) };
}

/// Runs `f`, returning what it read along with its result ([note]).
pub(crate) fn record<T>(f: impl FnOnce() -> T) -> (T, Reads) {
    RECORDING.with(|r| r.borrow_mut().push(Reads::default()));
    let result = f();
    let reads = RECORDING.with(|r| r.borrow_mut().pop()).unwrap_or_default();
    // an outer walk depends on what the inner one read
    RECORDING.with(|r| {
        if let Some(outer) = r.borrow_mut().last_mut() {
            outer.extend(reads.clone());
        }
    });
    (result, reads)
}

/// Note that the walk depends on `path` as it is now: a file read, a directory listed or one where a file was looked
/// for. Nothing when no walk records.
pub(crate) fn note(path: &Path) {
    note_stamp(path, || stamp(path));
}

/// [note] with the stamp already taken (before reading the file).
pub(crate) fn note_stamp(path: &Path, stamp: impl FnOnce() -> Stamp) {
    RECORDING.with(|r| {
        if let Some(reads) = r.borrow_mut().last_mut()
            && !reads.0.contains_key(path)
        {
            reads.0.insert(path.to_path_buf(), stamp());
        }
    });
}

/// [note] of the directory `path` is in: whether a file is there.
pub(crate) fn note_parent(path: &Path) {
    if let Some(dir) = path.parent() {
        note(dir);
    }
}
