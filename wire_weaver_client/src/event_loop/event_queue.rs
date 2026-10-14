//! Per-stream queue between the event loop and a plain stream's (or sink's, or property observer's) handle, bounded
//! in bytes of data (CLT-23): a program that stops reading keeps the host's memory bounded. Data that doesn't fit
//! is dropped (the newest) and reported once, as [RawStreamEvent::Dropped], before the next event that gets in.
//! Sideband and connection events always get in.

use std::collections::VecDeque;
use std::pin::pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

pub(crate) use tokio::sync::mpsc::error::TryRecvError;
use tracing::warn;

use crate::RawStreamEvent;

struct Shared {
    inner: Mutex<Inner>,
    notify: tokio::sync::Notify,
    cond: Condvar,
}

struct Inner {
    queue: VecDeque<RawStreamEvent>,
    bytes: usize,
    limit: usize,
    /// Dropped since the last event that got in: events, bytes
    dropped: Option<(u64, u64)>,
    dropped_total: u64,
    tx_alive: bool,
    rx_alive: bool,
}

/// A queue holding up to `limit` bytes of data (`usize::MAX`: unbounded, e.g., introspection data).
pub(crate) fn event_queue(limit: usize) -> (EventSender, EventReceiver) {
    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner {
            queue: VecDeque::new(),
            bytes: 0,
            limit,
            dropped: None,
            dropped_total: 0,
            tx_alive: true,
            rx_alive: true,
        }),
        notify: tokio::sync::Notify::new(),
        cond: Condvar::new(),
    });
    (EventSender(shared.clone()), EventReceiver(shared))
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wake(&self) {
        self.notify.notify_waiters();
        self.cond.notify_all();
    }
}

fn data_len(ev: &RawStreamEvent) -> usize {
    match ev {
        RawStreamEvent::Data(bytes) => bytes.len(),
        _ => 0,
    }
}

/// The event loop's end.
#[doc(hidden)]
pub struct EventSender(Arc<Shared>);

impl std::fmt::Debug for EventSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EventSender")
    }
}

impl EventSender {
    /// Queue `ev`, or drop it when it is data and the queue is full. Err when the receiver is gone.
    pub(crate) fn send(&self, ev: RawStreamEvent) -> Result<(), ()> {
        let mut inner = self.0.lock();
        if !inner.rx_alive {
            return Err(());
        }
        let len = data_len(&ev);
        if len > 0 && inner.bytes + len > inner.limit {
            let (events, bytes) = inner.dropped.get_or_insert((0, 0));
            *events += 1;
            *bytes += len as u64;
            inner.dropped_total += 1;
            return Ok(());
        }
        if let Some((events, bytes)) = inner.dropped.take() {
            warn!("stream queue full, dropped {events} events ({bytes} bytes)");
            inner
                .queue
                .push_back(RawStreamEvent::Dropped { events, bytes });
        }
        inner.bytes += len;
        inner.queue.push_back(ev);
        drop(inner);
        self.0.wake();
        Ok(())
    }
}

impl Drop for EventSender {
    fn drop(&mut self) {
        self.0.lock().tx_alive = false;
        self.0.wake();
    }
}

/// The handle's end, with the same calls as a tokio receiver.
pub(crate) struct EventReceiver(Arc<Shared>);

impl EventReceiver {
    pub(crate) async fn recv(&mut self) -> Option<RawStreamEvent> {
        let shared = self.0.clone();
        loop {
            let mut notified = pin!(shared.notify.notified());
            notified.as_mut().enable();
            match self.try_recv() {
                Ok(ev) => return Some(ev),
                Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) => notified.await,
            }
        }
    }

    pub(crate) fn blocking_recv(&mut self) -> Option<RawStreamEvent> {
        let mut inner = self.0.lock();
        loop {
            if let Some(ev) = pop(&mut inner) {
                return Some(ev);
            }
            if !inner.tx_alive {
                return None;
            }
            inner = self.0.cond.wait(inner).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub(crate) fn try_recv(&mut self) -> Result<RawStreamEvent, TryRecvError> {
        let mut inner = self.0.lock();
        match pop(&mut inner) {
            Some(ev) => Ok(ev),
            None if !inner.tx_alive => Err(TryRecvError::Disconnected),
            None => Err(TryRecvError::Empty),
        }
    }

    /// Data events dropped because the queue was full, since the stream was opened.
    pub(crate) fn dropped(&self) -> u64 {
        self.0.lock().dropped_total
    }
}

fn pop(inner: &mut Inner) -> Option<RawStreamEvent> {
    let ev = inner.queue.pop_front()?;
    inner.bytes -= data_len(&ev);
    Some(ev)
}

impl Drop for EventReceiver {
    fn drop(&mut self) {
        let mut inner = self.0.lock();
        inner.rx_alive = false;
        inner.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_newest_data_and_reports_it_once() {
        let (tx, mut rx) = event_queue(10);
        tx.send(RawStreamEvent::Data(vec![0; 6])).unwrap();
        tx.send(RawStreamEvent::Data(vec![1; 6])).unwrap();
        tx.send(RawStreamEvent::Data(vec![2; 6])).unwrap();
        // control events always get in, after the report
        tx.send(RawStreamEvent::Connected).unwrap();
        tx.send(RawStreamEvent::Data(vec![3; 4])).unwrap();
        assert_eq!(rx.try_recv().unwrap(), RawStreamEvent::Data(vec![0; 6]));
        assert_eq!(
            rx.try_recv().unwrap(),
            RawStreamEvent::Dropped {
                events: 2,
                bytes: 12
            }
        );
        assert_eq!(rx.try_recv().unwrap(), RawStreamEvent::Connected);
        assert_eq!(rx.try_recv().unwrap(), RawStreamEvent::Data(vec![3; 4]));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(rx.dropped(), 2);
        drop(tx);
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
    }

    #[test]
    fn sender_errs_once_the_receiver_is_gone() {
        let (tx, rx) = event_queue(10);
        drop(rx);
        assert!(tx.send(RawStreamEvent::Connected).is_err());
    }
}
