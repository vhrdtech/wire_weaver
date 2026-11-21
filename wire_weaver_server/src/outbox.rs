//! Events a running handler sends, on their way to the connection's writer.

use crate::sync::locked;
use std::collections::VecDeque;
use std::future::poll_fn;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use wire_weaver::{MessageSink, Priority};

/// A bounded queue of serialized events. A handler that fills it waits until the connection wrote some out, so a
/// client that reads slowly slows its own handlers down and takes no memory beyond the bound.
pub(crate) struct Outbox {
    inner: Mutex<Inner>,
    events_max: usize,
    /// The longest message the link takes, once it is up: a longer event is refused where it is sent, so its
    /// handler can answer with an error.
    message_len_max: AtomicUsize,
}

/// A queued event.
pub(crate) struct Queued {
    pub(crate) event: Vec<u8>,
    /// A reply to an urgent request: sent at once, not within the accumulation window.
    pub(crate) urgent: bool,
}

#[derive(Default)]
struct Inner {
    events: VecDeque<Queued>,
    /// Handlers waiting for room.
    waiting: Vec<Waker>,
}

impl Outbox {
    pub(crate) fn new(events_max: usize) -> Self {
        Outbox {
            inner: Mutex::default(),
            events_max,
            message_len_max: AtomicUsize::new(usize::MAX),
        }
    }

    /// The link is up and takes messages of up to `len` bytes.
    pub(crate) fn set_message_len_max(&self, len: usize) {
        self.message_len_max.store(len, Ordering::Release);
    }

    fn fits(&self, event: &[u8]) -> bool {
        event.len() <= self.message_len_max.load(Ordering::Acquire)
    }

    fn poll_push(&self, cx: &mut Context<'_>, event: &[u8], urgent: bool) -> Poll<()> {
        let mut inner = locked(&self.inner);
        if inner.events.len() >= self.events_max {
            if !inner.waiting.iter().any(|w| w.will_wake(cx.waker())) {
                inner.waiting.push(cx.waker().clone());
            }
            return Poll::Pending;
        }
        inner.events.push_back(Queued {
            event: event.to_vec(),
            urgent,
        });
        Poll::Ready(())
    }

    /// Whether an event can be queued now.
    pub(crate) fn has_room(&self) -> bool {
        locked(&self.inner).events.len() < self.events_max
    }

    /// Queue `event` if there is room and it fits the link; false if it was not queued.
    pub(crate) fn try_push(&self, event: &[u8], urgent: bool) -> bool {
        if !self.fits(event) {
            return false;
        }
        let mut inner = locked(&self.inner);
        if inner.events.len() >= self.events_max {
            return false;
        }
        inner.events.push_back(Queued {
            event: event.to_vec(),
            urgent,
        });
        true
    }

    /// The oldest event; handlers waiting for room are woken.
    pub(crate) fn pop(&self) -> Option<Queued> {
        let mut inner = locked(&self.inner);
        let event = inner.events.pop_front();
        for waker in inner.waiting.drain(..) {
            waker.wake();
        }
        event
    }

    pub(crate) fn is_empty(&self) -> bool {
        locked(&self.inner).events.is_empty()
    }
}

/// What a handler's `EventWriter` writes to.
pub(crate) struct OutboxSink<'o>(pub(crate) &'o Outbox);

impl OutboxSink<'_> {
    async fn push(&mut self, message: &[u8], urgent: bool) -> Result<(), ()> {
        if !self.0.fits(message) {
            return Err(());
        }
        poll_fn(|cx| self.0.poll_push(cx, message, urgent)).await;
        Ok(())
    }
}

impl MessageSink for OutboxSink<'_> {
    async fn send(&mut self, message: &[u8]) -> Result<(), ()> {
        self.push(message, false).await
    }

    async fn send_reply(&mut self, message: &[u8], priority: Priority) -> Result<(), ()> {
        self.push(message, priority.is_urgent()).await
    }
}
