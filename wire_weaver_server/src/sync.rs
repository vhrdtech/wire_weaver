//! How this crate takes its locks.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock `mutex`, also after a panic while it was held. The state behind this crate's locks is whole after every
/// single step that changes it (a queue pushed or popped, a waker stored), so a panic in between, such as one in a
/// caller's `Clone`, leaves nothing half done.
pub(crate) fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
