//! One sender, a queue per subscriber: what many connections follow at once.

use std::collections::{BTreeMap, VecDeque};
use std::future::poll_fn;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::sync::locked;

/// Sends each item to every [Subscription]. A subscriber that reads slowly loses its oldest items and is told how
/// many; it never holds up the sender or the other subscribers.
///
/// Clones send to the same subscribers. When the last one is dropped, subscribers get what is queued, then `None`.
pub struct Fanout<T> {
    shared: Arc<Mutex<Shared<T>>>,
}

/// One subscriber's end of a [Fanout]. Dropping it unsubscribes.
pub struct Subscription<T> {
    shared: Arc<Mutex<Shared<T>>>,
    id: u64,
}

/// Items a subscriber lost because its queue was full: the oldest ones, dropped to make room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lagged(pub u64);

struct Shared<T> {
    subscribers: BTreeMap<u64, Queue<T>>,
    next_id: u64,
    queue_max: usize,
    senders: usize,
}

struct Queue<T> {
    items: VecDeque<T>,
    /// Lost since the subscriber was last told.
    lagged: u64,
    waker: Option<Waker>,
}

impl<T: Clone> Fanout<T> {
    /// A fan-out whose subscribers each queue up to `queue_max` items.
    ///
    /// # Panics
    ///
    /// When `queue_max` is 0: a subscriber could never get an item.
    pub fn new(queue_max: usize) -> Self {
        assert!(
            queue_max > 0,
            "a subscriber's queue holds at least one item"
        );
        Fanout {
            shared: Arc::new(Mutex::new(Shared {
                subscribers: BTreeMap::new(),
                next_id: 0,
                queue_max,
                senders: 1,
            })),
        }
    }

    /// Give `item` to every subscriber. Never waits: a full queue drops its oldest item.
    pub fn send(&self, item: T) {
        let mut shared = locked(&self.shared);
        let queue_max = shared.queue_max;
        for queue in shared.subscribers.values_mut() {
            if queue.items.len() == queue_max {
                queue.items.pop_front();
                queue.lagged += 1;
            }
            queue.items.push_back(item.clone());
            if let Some(waker) = queue.waker.take() {
                waker.wake();
            }
        }
    }

    /// Items sent from now on.
    pub fn subscribe(&self) -> Subscription<T> {
        let mut shared = locked(&self.shared);
        let id = shared.next_id;
        shared.next_id += 1;
        shared.subscribers.insert(
            id,
            Queue {
                items: VecDeque::new(),
                lagged: 0,
                waker: None,
            },
        );
        Subscription {
            shared: self.shared.clone(),
            id,
        }
    }

    /// How many subscribers there are now.
    pub fn subscribers(&self) -> usize {
        locked(&self.shared).subscribers.len()
    }
}

impl<T> Clone for Fanout<T> {
    fn clone(&self) -> Self {
        locked(&self.shared).senders += 1;
        Fanout {
            shared: self.shared.clone(),
        }
    }
}

impl<T> Drop for Fanout<T> {
    fn drop(&mut self) {
        let mut shared = locked(&self.shared);
        shared.senders -= 1;
        if shared.senders == 0 {
            for queue in shared.subscribers.values_mut() {
                if let Some(waker) = queue.waker.take() {
                    waker.wake();
                }
            }
        }
    }
}

impl<T> Subscription<T> {
    /// The next item, `Err(Lagged)` first when items were lost before it, `None` when every sender is gone and
    /// the queue is empty.
    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<T, Lagged>>> {
        let mut shared = locked(&self.shared);
        let senders = shared.senders;
        let queue = shared
            .subscribers
            .get_mut(&self.id)
            .expect("a subscription's queue lives as long as the subscription");
        if queue.lagged > 0 {
            let lagged = std::mem::take(&mut queue.lagged);
            return Poll::Ready(Some(Err(Lagged(lagged))));
        }
        if let Some(item) = queue.items.pop_front() {
            return Poll::Ready(Some(Ok(item)));
        }
        if senders == 0 {
            return Poll::Ready(None);
        }
        queue.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    /// [poll_recv](Self::poll_recv) as a future. Cancel-safe: an item is taken only when the future completes.
    pub async fn recv(&mut self) -> Option<Result<T, Lagged>> {
        poll_fn(|cx| self.poll_recv(cx)).await
    }
}

/// The items of [poll_recv](Subscription::poll_recv) as a stream, for a [pump](crate::Pumps::add).
impl<T> futures_util::Stream for Subscription<T> {
    type Item = Result<T, Lagged>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<T, Lagged>>> {
        self.get_mut().poll_recv(cx)
    }
}

impl<T> Drop for Subscription<T> {
    fn drop(&mut self) {
        locked(&self.shared).subscribers.remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[test]
    fn every_subscriber_gets_every_item_in_order() {
        let fanout = Fanout::new(4);
        let mut a = fanout.subscribe();
        let mut b = fanout.subscribe();
        fanout.send(1);
        fanout.send(2);
        for sub in [&mut a, &mut b] {
            assert_eq!(sub.recv().now_or_never(), Some(Some(Ok(1))));
            assert_eq!(sub.recv().now_or_never(), Some(Some(Ok(2))));
            assert_eq!(sub.recv().now_or_never(), None, "nothing more yet");
        }
    }

    #[test]
    fn a_slow_subscriber_loses_its_oldest_items_and_is_told() {
        let fanout = Fanout::new(2);
        let mut slow = fanout.subscribe();
        let mut fast = fanout.subscribe();
        for n in 0..5 {
            fanout.send(n);
            assert_eq!(fast.recv().now_or_never(), Some(Some(Ok(n))));
        }
        assert_eq!(slow.recv().now_or_never(), Some(Some(Err(Lagged(3)))));
        assert_eq!(slow.recv().now_or_never(), Some(Some(Ok(3))));
        assert_eq!(slow.recv().now_or_never(), Some(Some(Ok(4))));
    }

    #[test]
    fn subscribers_end_after_the_last_sender_and_leave_on_drop() {
        let fanout = Fanout::new(2);
        let other = fanout.clone();
        let mut sub = fanout.subscribe();
        assert_eq!(fanout.subscribers(), 1);
        fanout.send(7);
        drop(fanout);
        assert_eq!(sub.recv().now_or_never(), Some(Some(Ok(7))));
        assert_eq!(sub.recv().now_or_never(), None, "a sender is left");
        assert_eq!(other.subscribers(), 1);
        let again = other.subscribe();
        drop(again);
        assert_eq!(other.subscribers(), 1);
        drop(other);
        assert_eq!(sub.recv().now_or_never(), Some(None));
    }
}
