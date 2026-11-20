//! What the server tells its connections: keep going, finish up, or stop now.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
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

/// The server's stage, shared with every connection.
pub(crate) struct Lifecycle {
    stage: AtomicU8,
    watchers: Mutex<Vec<Waker>>,
}

impl Lifecycle {
    pub(crate) fn new() -> Self {
        Lifecycle {
            stage: AtomicU8::new(Stage::Run as u8),
            watchers: Mutex::default(),
        }
    }

    /// The stage now; the task of `cx` is woken when it changes.
    pub(crate) fn watch(&self, cx: &mut Context<'_>) -> Stage {
        let mut watchers = self
            .watchers
            .lock()
            .expect("the watchers lock is never held across a panic");
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
        let watchers = std::mem::take(
            &mut *self
                .watchers
                .lock()
                .expect("the watchers lock is never held across a panic"),
        );
        for waker in watchers {
            waker.wake();
        }
    }
}
