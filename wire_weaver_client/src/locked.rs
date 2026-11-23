//! The one place this crate takes a lock and decides what poison means: a poisoned lock panics, as the thread that
//! poisoned it did, and the unit restarts clean.

use std::sync::{LockResult, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// The guard of a lock just taken; panics when another thread panicked while holding it.
fn unpoisoned<G>(taken: LockResult<G>) -> G {
    taken.expect("lock poisoned: a thread panicked while holding it")
}

/// Takes a [Mutex], panicking when it is poisoned.
pub(crate) trait Locked<T: ?Sized> {
    /// The guard; panics when another thread panicked while holding the lock.
    fn locked(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> Locked<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        unpoisoned(self.lock())
    }
}

/// Takes an [RwLock], panicking when it is poisoned.
pub(crate) trait RwLocked<T: ?Sized> {
    /// The read guard; panics when another thread panicked while holding the lock.
    fn read_locked(&self) -> RwLockReadGuard<'_, T>;
    /// The write guard; panics when another thread panicked while holding the lock.
    fn write_locked(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T: ?Sized> RwLocked<T> for RwLock<T> {
    fn read_locked(&self) -> RwLockReadGuard<'_, T> {
        unpoisoned(self.read())
    }

    fn write_locked(&self) -> RwLockWriteGuard<'_, T> {
        unpoisoned(self.write())
    }
}
