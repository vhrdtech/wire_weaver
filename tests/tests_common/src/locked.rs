//! The one place the test crates take a lock and decide what poison means: a poisoned lock panics, as the thread that
//! poisoned it did, and the test fails.

use std::sync::{Mutex, MutexGuard};

/// Takes a [Mutex], panicking when it is poisoned.
pub trait Locked<T: ?Sized> {
    /// The guard; panics when another thread panicked while holding the lock.
    fn locked(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> Locked<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        // spirit-level: allow(lock-unwrap) the crate's one place that decides poison: it panics
        self.lock()
            .expect("lock poisoned: a thread panicked while holding it")
    }
}
