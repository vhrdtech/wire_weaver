//! What the server shares with its connections: its stage (keep going, finish up, or stop now) and its load.

use crate::sync::locked;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::task::{Context, Waker};

/// Where the server is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Stage {
    /// Serving.
    Run = 0,
    /// Shutting down: connections finish the requests they have and disconnect.
    Drain = 1,
    /// The grace time is over: connections disconnect at once.
    Abort = 2,
}

/// The server's stage and load, shared with every connection.
pub(crate) struct Lifecycle {
    stage: AtomicU8,
    watchers: Mutex<Vec<Waker>>,
    /// Requests that run now, across all connections.
    running: AtomicUsize,
}

/// A request counted as running; counted down when dropped.
pub(crate) struct Running<'a>(&'a AtomicUsize);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Lifecycle {
    pub(crate) fn new() -> Self {
        Lifecycle {
            stage: AtomicU8::new(Stage::Run as u8),
            watchers: Mutex::default(),
            running: AtomicUsize::new(0),
        }
    }

    /// Count a request as running, unless `max` of them run already.
    pub(crate) fn start_request(&self, max: usize) -> Option<Running<'_>> {
        self.running
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < max).then_some(n + 1)
            })
            .ok()
            .map(|_| Running(&self.running))
    }

    /// The stage now; the task of `cx` is woken when it changes.
    pub(crate) fn watch(&self, cx: &mut Context<'_>) -> Stage {
        let mut watchers = locked(&self.watchers);
        if !watchers.iter().any(|w| w.will_wake(cx.waker())) {
            watchers.push(cx.waker().clone());
        }
        self.stage()
    }

    pub(crate) fn stage(&self) -> Stage {
        match self.stage.load(Ordering::Acquire) {
            0 => Stage::Run,
            1 => Stage::Drain,
            _ => Stage::Abort,
        }
    }

    /// Move on to `stage` and wake every connection.
    pub(crate) fn set(&self, stage: Stage) {
        self.stage.store(stage as u8, Ordering::Release);
        let watchers = std::mem::take(&mut *locked(&self.watchers));
        for waker in watchers {
            waker.wake();
        }
    }
}
