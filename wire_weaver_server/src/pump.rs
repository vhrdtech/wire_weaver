//! Streams of events a connection sends on its own, whatever else runs: the items of a stream a client opened.

use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll, Waker};

use futures_util::stream::Stream;
use wire_weaver::shrink_wrap::Error as ShrinkWrapError;

use crate::outbox::Outbox;
use crate::sync::locked;

/// Names a pump of one connection, so that the handler that closes its stream can [remove](Pumps::remove) it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PumpId(pub u64);

/// The pumps of one connection: streams whose items the connection serializes and sends as they come, while
/// requests run, also calls that take long.
///
/// A stream open handler [adds](Self::add) a pump instead of sending items itself, its close handler
/// [removes](Self::remove) it. The handle comes with [Connection](crate::Connection); a backend keeps a clone. The
/// pumps belong to the connection: they end with it, whoever still holds a handle.
///
/// ```ignore
/// async fn news(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
///     let items = self.shared.news.subscribe().filter_map(|item| ready(item.ok()));
///     self.pumps.add(NEWS, items, |item, buf| api_impl::stream_data_ser().news(item, buf).map(<[u8]>::len));
///     Ready(())
/// }
/// ```
#[derive(Clone, Default)]
pub struct Pumps(Weak<Mutex<Inner>>);

/// The pumps a connection owns; [Pumps] are handles to them.
#[derive(Default)]
pub(crate) struct PumpSet(Arc<Mutex<Inner>>);

#[derive(Default)]
struct Inner {
    /// Added and not sending yet: a pump starts once the answer of the request that added it went out, so the
    /// client knows the stream before its first item.
    added: Vec<(PumpId, Box<dyn Pump>)>,
    running: Vec<(PumpId, Box<dyn Pump>)>,
    /// The connection's task, woken when a pump starts.
    connection: Option<Waker>,
}

impl Pumps {
    /// Send the items of `items`, each serialized by `ser` into the buffer it is given (the generated
    /// `stream_data_ser().<name>(item, buf)`), returning the length. A pump `id` already has is replaced. An item
    /// that doesn't serialize (longer than the link's message limit) is skipped; the pump ends with `items`.
    pub fn add<T, S, F>(&self, id: PumpId, items: S, ser: F)
    where
        T: 'static,
        S: Stream<Item = T> + Send + 'static,
        F: FnMut(&T, &mut [u8]) -> Result<usize, ShrinkWrapError> + Send + 'static,
    {
        // the connection ended: nobody would send the items
        let Some(inner) = self.0.upgrade() else {
            return;
        };
        let pump: Box<dyn Pump> = Box::new(Items {
            items: Box::pin(items),
            ser,
        });
        let mut inner = locked(&inner);
        inner.remove(id);
        inner.added.push((id, pump));
    }

    /// Stop the pump `id`: none of its items is sent from now on. Nothing happens if there is none.
    pub fn remove(&self, id: PumpId) {
        if let Some(inner) = self.0.upgrade() {
            locked(&inner).remove(id);
        }
    }
}

impl PumpSet {
    /// A handle for the connection's backend.
    pub(crate) fn handle(&self) -> Pumps {
        Pumps(Arc::downgrade(&self.0))
    }

    /// Start the pumps added since the last call.
    pub(crate) fn start_added(&self) {
        let mut inner = locked(&self.0);
        if inner.added.is_empty() {
            return;
        }
        let added = std::mem::take(&mut inner.added);
        inner.running.extend(added);
        if let Some(waker) = inner.connection.take() {
            waker.wake();
        }
    }

    /// Move the items that are ready into `outbox` while it has room; `buf` is where they are serialized. True if
    /// one was queued.
    pub(crate) fn poll_into(&self, cx: &mut Context<'_>, outbox: &Outbox, buf: &mut [u8]) -> bool {
        let mut inner = locked(&self.0);
        inner.connection = Some(cx.waker().clone());
        let mut queued = false;
        inner.running.retain_mut(|(_, pump)| {
            while outbox.has_room() {
                match pump.poll_event(cx, buf) {
                    Poll::Ready(Some(Ok(len))) => {
                        queued |= outbox.try_push(&buf[..len], false);
                    }
                    // too long for the link: the item is skipped, the stream goes on
                    Poll::Ready(Some(Err(_))) => {}
                    Poll::Ready(None) => return false,
                    Poll::Pending => return true,
                }
            }
            // the outbox wakes the connection when it has room again
            true
        });
        queued
    }
}

/// How many pumps run and wait to start; `ended` once the connection is gone.
impl std::fmt::Debug for Pumps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Some(inner) = self.0.upgrade() else {
            return f.write_str("Pumps(ended)");
        };
        let inner = locked(&inner);
        f.debug_struct("Pumps")
            .field("added", &inner.added.len())
            .field("running", &inner.running.len())
            .finish()
    }
}

impl Inner {
    fn remove(&mut self, id: PumpId) {
        self.added.retain(|(i, _)| *i != id);
        self.running.retain(|(i, _)| *i != id);
    }
}

/// A pump with its item type hidden.
trait Pump: Send {
    /// The next item, serialized into `buf`: its length.
    fn poll_event(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Option<Result<usize, ShrinkWrapError>>>;
}

struct Items<T, F> {
    items: Pin<Box<dyn Stream<Item = T> + Send>>,
    ser: F,
}

impl<T, F> Pump for Items<T, F>
where
    F: FnMut(&T, &mut [u8]) -> Result<usize, ShrinkWrapError> + Send,
{
    fn poll_event(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Option<Result<usize, ShrinkWrapError>>> {
        self.items
            .as_mut()
            .poll_next(cx)
            .map(|item| item.map(|item| (self.ser)(&item, buf)))
    }
}
