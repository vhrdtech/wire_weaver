//! A wake for the server task (SRV-25), raised by any task or interrupt: what lets a device's event loop sleep
//! until something is due instead of polling on a fixed tick.
//!
//! The server task selects on [Wake::wait] next to the server's `wait()` and its next-wake timer
//! (`ObserveState::next_wake`); anything that changes what the server has to send (a property changed outside the
//! server state, a batch of samples, a deferred answer) calls [Wake::wake]. The generated `ObserveSignal` wraps one
//! with a changed bit per observable property.
//!
//! **No lost wake.** A wake sets a flag, then wakes the waiting task; [wait](Wake::wait) registers the task first,
//! then takes the flag. A wake that comes between the server task's last look and its sleep either set the flag
//! before the take (the wait returns at once) or finds the task registered (it is polled again). Wakes in between
//! two waits collapse into one: the flag says that something happened, the caller looks at what.
//!
//! **Interrupt-safe.** [wake](Wake::wake) is a store and a lock-free waker take (`atomic-waker`); targets without
//! compare-exchange (`thumbv6m`) get it from `portable-atomic`'s critical section, a few instructions long. Not
//! for more than one waiter: a second task waiting replaces the first's waker.

use atomic_waker::AtomicWaker;
use core::future::poll_fn;
use core::task::Poll;
use portable_atomic::{AtomicBool, Ordering};

/// A wake flag with one waiting task, `const`-constructible for a `static`.
#[derive(Debug, Default)]
pub struct Wake {
    waker: AtomicWaker,
    pending: AtomicBool,
}

impl Wake {
    pub const fn new() -> Self {
        Wake {
            waker: AtomicWaker::new(),
            pending: AtomicBool::new(false),
        }
    }

    /// Wake the waiting task, or the next [wait](Self::wait) if none waits now. Callable from an interrupt.
    /// Everything written before it is seen by the task after its wait (Release, Acquire).
    #[inline]
    pub fn wake(&self) {
        self.pending.store(true, Ordering::Release);
        self.waker.wake();
    }

    /// Wait for a [wake](Self::wake) since the last wait returned (at once when one came meanwhile). Cancel-safe:
    /// a wake is kept until a wait takes it.
    pub async fn wait(&self) {
        poll_fn(|cx| {
            self.waker.register(cx.waker());
            if self.take() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await
    }

    /// Whether a wake came since the last [wait](Self::wait) or take, and clear it: for a loop that doesn't
    /// await (a blocking server polling its sources).
    #[inline]
    pub fn take(&self) -> bool {
        self.pending.swap(false, Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::pin::pin;
    use core::task::{Context, Waker};
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::task::Wake as StdWake;

    struct Count(AtomicUsize);

    impl StdWake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn waker() -> (Arc<Count>, Waker) {
        let count = Arc::new(Count(AtomicUsize::new(0)));
        (count.clone(), Waker::from(count))
    }

    #[test]
    fn wake_before_wait_returns_at_once() {
        let w = Wake::new();
        w.wake();
        let (_, waker) = waker();
        let mut cx = Context::from_waker(&waker);
        assert!(pin!(w.wait()).as_mut().poll(&mut cx).is_ready());
        // taken: the next wait sleeps
        assert!(pin!(w.wait()).as_mut().poll(&mut cx).is_pending());
    }

    #[test]
    fn wake_while_waiting_wakes_the_task() {
        let w = Wake::new();
        let (count, waker) = waker();
        let mut cx = Context::from_waker(&waker);
        let mut fut = pin!(w.wait());
        assert!(fut.as_mut().poll(&mut cx).is_pending());
        w.wake();
        assert_eq!(count.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(fut.as_mut().poll(&mut cx).is_ready());
    }

    #[test]
    fn wakes_collapse_and_survive_a_dropped_wait() {
        let w = Wake::new();
        let (_, waker) = waker();
        let mut cx = Context::from_waker(&waker);
        // a wait polled once, then dropped
        assert!(pin!(w.wait()).as_mut().poll(&mut cx).is_pending());
        w.wake();
        w.wake();
        assert!(pin!(w.wait()).as_mut().poll(&mut cx).is_ready());
        assert!(!w.take());
    }

    /// A wake from another thread racing the wait's registration is never lost: every round ends.
    #[test]
    fn no_lost_wake_across_threads() {
        let w = Arc::new(Wake::new());
        for _ in 0..2_000 {
            let (count, waker) = waker();
            let mut cx = Context::from_waker(&waker);
            let w2 = w.clone();
            let t = std::thread::spawn(move || w2.wake());
            let mut fut = pin!(w.wait());
            if fut.as_mut().poll(&mut cx).is_pending() {
                t.join().unwrap();
                // the wake came after the registration: it woke the waker, and the next poll is ready
                assert_eq!(count.0.load(std::sync::atomic::Ordering::SeqCst), 1);
                assert!(fut.as_mut().poll(&mut cx).is_ready());
            } else {
                t.join().unwrap();
                // taken before the thread's wake landed, or after: drain what's left
                w.take();
            }
        }
    }
}
