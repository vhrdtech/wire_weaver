//! Streams of events a connection sends on its own, whatever else runs: the items of a stream a client opened.
//!
//! A connection's pumps come in two kinds, picked by its [MakeBackend](crate::MakeBackend): [Pumps] take `Send`
//! streams and leave the connection `Send`, [LocalPumps] take any stream and make the connection `!Send`, for a
//! single-threaded program whose state is `Rc` / `RefCell` (SRV-73).

use std::cell::RefCell;
use std::pin::Pin;
use std::rc::{self, Rc};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll, Waker};

use futures_util::stream::Stream;
use wire_weaver::shrink_wrap::Error as ShrinkWrapError;

use crate::outbox::Outbox;
use crate::sync::locked;

/// Names a pump of one connection, so that the handler that closes its stream can [remove](Pumps::remove) it: a
/// number, and the element's indices for a stream of a resource array (`PumpId::indexed(3, &[2])` for element 2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PumpId {
    id: u64,
    indices: Vec<u32>,
}

impl PumpId {
    /// The pump `id`.
    pub const fn new(id: u64) -> Self {
        PumpId {
            id,
            indices: Vec::new(),
        }
    }

    /// The pump `id` of the element at `indices`, one index per resource array on the stream's path.
    pub fn indexed(id: u64, indices: &[u32]) -> Self {
        PumpId {
            id,
            indices: indices.to_vec(),
        }
    }
}

/// The pumps of one connection: streams whose items the connection serializes and sends as they come, while
/// requests run, also calls that take long.
///
/// A stream open handler [adds](Self::add) a pump instead of sending items itself, its close handler
/// [removes](Self::remove) it. The handle comes with [Connection](crate::Connection); a backend keeps a clone. The
/// pumps belong to the connection: they end with it, whoever still holds a handle. Streams that are not `Send` go
/// to [LocalPumps].
///
/// ```ignore
/// async fn news(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
///     let items = self.shared.news.subscribe().filter_map(|item| ready(item.ok()));
///     self.pumps.add(NEWS, items, |item, buf| api_impl::stream_data_ser().news(item, buf).map(<[u8]>::len));
///     Ready(())
/// }
/// ```
#[derive(Clone, Default)]
pub struct Pumps(Weak<Mutex<Inner<dyn Pump + Send>>>);

/// The pumps of one connection, for streams that are not `Send`: [Pumps] for a single-threaded connection.
///
/// A connection has them when its [MakeBackend](crate::MakeBackend) says `type Pumps = LocalPumps`, as
/// [Local](crate::Local) does; the connection's future is then not `Send`, so it runs in
/// [serve](crate::Server::serve) on a single-threaded executor, or is spawned as a local task.
#[derive(Clone, Default)]
pub struct LocalPumps(rc::Weak<RefCell<Inner<dyn Pump>>>);

/// The kind of a connection's pump handle: [Pumps] or [LocalPumps].
pub trait PumpHandle: Clone + Default + std::fmt::Debug + 'static + sealed::Kind {
    /// Stop the pump `id`: none of its items is sent from now on. Nothing happens if there is none.
    fn remove(&self, id: PumpId);
}

pub(crate) use sealed::Kind;

mod sealed {
    use super::PumpSetOps;

    /// What the connection needs of a pump handle's kind: the pumps it owns.
    pub trait Kind: Sized {
        /// The pumps a connection owns; the handles point at them.
        type Set: PumpSetOps;

        /// A connection's pumps and the first handle to them.
        fn new_set() -> (Self::Set, Self);
    }
}

/// What a connection does with the pumps it owns.
pub trait PumpSetOps {
    /// Start the pumps added since the last call.
    fn start_added(&self);

    /// Move the items that are ready into `outbox` while it has room; `buf` is where they are serialized. True if
    /// one was queued.
    fn poll_into(&self, cx: &mut Context<'_>, outbox: &Outbox, buf: &mut [u8]) -> bool;
}

/// The pumps of a connection with [Pumps].
pub struct PumpSet(Arc<Mutex<Inner<dyn Pump + Send>>>);

/// The pumps of a connection with [LocalPumps].
pub struct LocalPumpSet(Rc<RefCell<Inner<dyn Pump>>>);

struct Inner<P: ?Sized> {
    /// Added and not sending yet: a pump starts once the answer of the request that added it went out, so the
    /// client knows the stream before its first item.
    added: Vec<(PumpId, Box<P>)>,
    running: Vec<(PumpId, Box<P>)>,
    /// The connection's task, woken when a pump starts.
    connection: Option<Waker>,
}

impl<P: ?Sized> Default for Inner<P> {
    fn default() -> Self {
        Inner {
            added: Vec::new(),
            running: Vec::new(),
            connection: None,
        }
    }
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
        self.add_ending(id, items, ser, None::<NoEnd>);
    }

    /// [add](Self::add) a pump that sends one more event when `items` end: the one `end` serializes, the stream's
    /// `Close` sideband (the generated `stream_data_ser().<name>_sideband(StreamSideband::Close, buf)`), so the
    /// client's stream ends too.
    pub fn add_ending<T, S, F, E>(&self, id: PumpId, items: S, ser: F, end: Option<E>)
    where
        T: 'static,
        S: Stream<Item = T> + Send + 'static,
        F: FnMut(&T, &mut [u8]) -> Result<usize, ShrinkWrapError> + Send + 'static,
        E: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError> + Send + 'static,
    {
        // the connection ended: nobody would send the items
        let Some(inner) = self.0.upgrade() else {
            return;
        };
        let pump: Box<dyn Pump + Send> = Box::new(Items {
            items: Some(Box::pin(items) as Pin<Box<dyn Stream<Item = T> + Send>>),
            ser,
            end,
        });
        locked(&inner).add(id, pump);
    }

    /// Stop the pump `id`: none of its items is sent from now on. Nothing happens if there is none.
    pub fn remove(&self, id: PumpId) {
        if let Some(inner) = self.0.upgrade() {
            locked(&inner).remove(&id);
        }
    }
}

impl LocalPumps {
    /// Send the items of `items`, as [Pumps::add] does, from a stream that need not be `Send`.
    pub fn add<T, S, F>(&self, id: PumpId, items: S, ser: F)
    where
        T: 'static,
        S: Stream<Item = T> + 'static,
        F: FnMut(&T, &mut [u8]) -> Result<usize, ShrinkWrapError> + 'static,
    {
        self.add_ending(id, items, ser, None::<NoEnd>);
    }

    /// [add](Self::add) a pump that sends `end`'s event when `items` end, as [Pumps::add_ending] does.
    pub fn add_ending<T, S, F, E>(&self, id: PumpId, items: S, ser: F, end: Option<E>)
    where
        T: 'static,
        S: Stream<Item = T> + 'static,
        F: FnMut(&T, &mut [u8]) -> Result<usize, ShrinkWrapError> + 'static,
        E: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError> + 'static,
    {
        let Some(inner) = self.0.upgrade() else {
            return;
        };
        let pump: Box<dyn Pump> = Box::new(Items {
            items: Some(Box::pin(items) as Pin<Box<dyn Stream<Item = T>>>),
            ser,
            end,
        });
        inner.borrow_mut().add(id, pump);
    }

    /// Stop the pump `id`: none of its items is sent from now on. Nothing happens if there is none.
    pub fn remove(&self, id: PumpId) {
        if let Some(inner) = self.0.upgrade() {
            inner.borrow_mut().remove(&id);
        }
    }
}

impl PumpHandle for Pumps {
    fn remove(&self, id: PumpId) {
        Pumps::remove(self, id);
    }
}

impl PumpHandle for LocalPumps {
    fn remove(&self, id: PumpId) {
        LocalPumps::remove(self, id);
    }
}

impl sealed::Kind for Pumps {
    type Set = PumpSet;

    fn new_set() -> (PumpSet, Pumps) {
        let set = Arc::new(Mutex::new(Inner::default()));
        let handle = Pumps(Arc::downgrade(&set));
        (PumpSet(set), handle)
    }
}

impl sealed::Kind for LocalPumps {
    type Set = LocalPumpSet;

    fn new_set() -> (LocalPumpSet, LocalPumps) {
        let set = Rc::new(RefCell::new(Inner::default()));
        let handle = LocalPumps(Rc::downgrade(&set));
        (LocalPumpSet(set), handle)
    }
}

impl PumpSetOps for PumpSet {
    fn start_added(&self) {
        locked(&self.0).start_added();
    }

    fn poll_into(&self, cx: &mut Context<'_>, outbox: &Outbox, buf: &mut [u8]) -> bool {
        locked(&self.0).poll_into(cx, outbox, buf)
    }
}

impl PumpSetOps for LocalPumpSet {
    fn start_added(&self) {
        self.0.borrow_mut().start_added();
    }

    fn poll_into(&self, cx: &mut Context<'_>, outbox: &Outbox, buf: &mut [u8]) -> bool {
        self.0.borrow_mut().poll_into(cx, outbox, buf)
    }
}

impl<P: Pump + ?Sized> Inner<P> {
    fn add(&mut self, id: PumpId, pump: Box<P>) {
        self.remove(&id);
        self.added.push((id, pump));
    }

    fn remove(&mut self, id: &PumpId) {
        self.added.retain(|(i, _)| i != id);
        self.running.retain(|(i, _)| i != id);
    }

    fn start_added(&mut self) {
        if self.added.is_empty() {
            return;
        }
        let added = std::mem::take(&mut self.added);
        self.running.extend(added);
        if let Some(waker) = self.connection.take() {
            waker.wake();
        }
    }

    fn poll_into(&mut self, cx: &mut Context<'_>, outbox: &Outbox, buf: &mut [u8]) -> bool {
        self.connection = Some(cx.waker().clone());
        let mut queued = false;
        self.running.retain_mut(|(_, pump)| {
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

    fn counts(&self) -> (usize, usize) {
        (self.added.len(), self.running.len())
    }
}

/// How many pumps run and wait to start; `ended` once the connection is gone.
impl std::fmt::Debug for Pumps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let counts = self.0.upgrade().map(|inner| locked(&inner).counts());
        debug_counts(f, "Pumps", counts)
    }
}

/// How many pumps run and wait to start; `ended` once the connection is gone.
impl std::fmt::Debug for LocalPumps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let counts = self.0.upgrade().map(|inner| inner.borrow().counts());
        debug_counts(f, "LocalPumps", counts)
    }
}

fn debug_counts(
    f: &mut std::fmt::Formatter<'_>,
    name: &str,
    counts: Option<(usize, usize)>,
) -> std::fmt::Result {
    let Some((added, running)) = counts else {
        return write!(f, "{name}(ended)");
    };
    f.debug_struct(name)
        .field("added", &added)
        .field("running", &running)
        .finish()
}

/// A pump with its item type hidden.
trait Pump {
    /// The next item, serialized into `buf`: its length.
    fn poll_event(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Option<Result<usize, ShrinkWrapError>>>;
}

/// The end event of a pump that has none.
type NoEnd = fn(&mut [u8]) -> Result<usize, ShrinkWrapError>;

/// A pump's items, `S` a boxed stream of `T`: `None` once they ended.
struct Items<S, F, E> {
    items: Option<S>,
    ser: F,
    /// Sent once after the items ended.
    end: Option<E>,
}

impl<S, T, F, E> Pump for Items<Pin<Box<S>>, F, E>
where
    S: Stream<Item = T> + ?Sized,
    F: FnMut(&T, &mut [u8]) -> Result<usize, ShrinkWrapError>,
    E: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
{
    fn poll_event(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Option<Result<usize, ShrinkWrapError>>> {
        if let Some(items) = self.items.as_mut() {
            match items.as_mut().poll_next(cx) {
                Poll::Ready(Some(item)) => return Poll::Ready(Some((self.ser)(&item, buf))),
                Poll::Ready(None) => self.items = None,
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(self.end.take().map(|end| end(buf)))
    }
}
