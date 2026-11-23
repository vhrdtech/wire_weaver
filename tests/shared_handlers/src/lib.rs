//! Shared handlers (SRV-65): servers whose shared items run through `&self` handlers in `process_shared_bytes`,
//! next to the exclusive entry. Every request of [requests::cases] is checked against `is_shared`; a shared one
//! answers the same through both entries, an exclusive one is refused by the shared entry. A blocking `no_alloc`
//! server names its shared items, an async allocating one shares everything but a few.

pub mod async_server;
pub mod blocking;
pub mod requests;

#[cfg(test)]
mod tests;

use std::sync::{Mutex, MutexGuard};

/// `slot` locked for a handler that records what it saw.
///
/// # Panics
///
/// When a handler panicked while holding it: that already failed the test that ran it.
pub(crate) fn locked<T>(slot: &Mutex<T>) -> MutexGuard<'_, T> {
    slot.lock()
        .expect("a handler panicked while holding the lock")
}
