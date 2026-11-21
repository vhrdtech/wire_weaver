//! One source of byte streams, many APIs: the router behind every [StreamListener].

use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, Stream, StreamExt};
use wire_weaver::Peer;
use ww_byte_stream::{Status, Target};
use ww_device::{Clock, Instant};

use super::framing::Framing;
use crate::listener::{Accepted, Listener};
use crate::sync::locked;

/// Connections opening at once. Past it the listener takes no new stream until one is answered.
pub const HANDSHAKES_MAX: usize = 16;
/// Connections of one API answered and not yet taken by its server. Past it a client is answered [Status::Busy].
pub const QUEUED_MAX: usize = 16;
/// How long a client has to open its connection (send its preamble), and the listener to answer it.
pub const OPENING_TIMEOUT: Duration = Duration::from_secs(2);

/// Where byte streams come from: any [Stream] of streams with the peer each one is from. A `TcpListener`, a local
/// socket, a channel of streams the program accepted itself, a single stdio stream.
///
/// The stream ends when no more connections come; an error is the listener's error and ends `Server::serve`, so
/// a source retries what it can (a full descriptor table) by itself.
pub trait Incoming: Stream<Item = Result<(Self::Stream, Peer), Self::Error>> + Unpin {
    /// One byte stream to a client.
    type Stream;
    /// Why accepting failed for good.
    type Error;
}

impl<T, S, E> Incoming for T
where
    T: Stream<Item = Result<(S, Peer), E>> + Unpin,
{
    type Stream = S;
    type Error = E;
}

/// A listener on byte streams, one per API: [listen] makes the first, which serves clients that ask for a device
/// (`ClientConfig::ipc_path`, `tcp`, `command` without a service name); [service](Self::service) adds one for the
/// clients that name a service. All of them share the source, so each API gets its own `Server::serve`.
///
/// ```ignore
/// let tasks = listen(TcpIncoming::bind(addr).await?, PreambleFraming::new("cs-stored 0.1.0", 16 * 1024), clock);
/// let journal = tasks.service("journal");
/// join(server.serve(tasks_link, tasks, ..), server.serve(journal_link, journal, ..)).await;
/// ```
///
/// A listener that is dropped refuses its clients from then on; the others go on. `H` is the type of a connection
/// opening, which [listen] picks.
pub struct StreamListener<I: Incoming, F: Framing<I::Stream>, C, H> {
    router: Arc<Mutex<Router<I, F, C, H>>>,
    slots: Arc<Slots<F::Tx, F::Rx>>,
    index: usize,
}

/// Listen on `incoming`: connections open as `framing` says, within [OPENING_TIMEOUT] on `clock`. The listener
/// returned serves clients that ask for a device; add services with [StreamListener::service].
pub fn listen<I, F, C>(
    incoming: I,
    framing: F,
    clock: C,
) -> StreamListener<I, F, C, impl Future<Output = ()>>
where
    I: Incoming,
    F: Framing<I::Stream>,
    C: Clock,
{
    let slots = Arc::new(Slots::new());
    let router = Router {
        incoming,
        ended: false,
        handshakes: FuturesUnordered::new(),
        open: handshake::<I::Stream, F, C>,
        opening: Arc::new(Opening { framing, clock }),
    };
    StreamListener {
        router: Arc::new(Mutex::new(router)),
        slots,
        index: 0,
    }
}

impl<I: Incoming, F: Framing<I::Stream>, C, H> StreamListener<I, F, C, H> {
    /// Another listener on the same source, for the clients that ask for the service `name`
    /// (`ClientConfig::service`).
    ///
    /// # Panics
    ///
    /// A listener for `name` exists already: two servers can't take the same clients.
    pub fn service(&self, name: &str) -> Self {
        StreamListener {
            router: self.router.clone(),
            slots: self.slots.clone(),
            index: self.slots.add(name),
        }
    }
}

impl<I, F, C, H> Listener for StreamListener<I, F, C, H>
where
    I: Incoming,
    F: Framing<I::Stream>,
    C: Clock,
    H: Future<Output = ()>,
{
    type Tx = F::Tx;
    type Rx = F::Rx;
    type Error = I::Error;

    /// Cancel-safe: connections opening and opened stay in the listener. `None` once the source ended and every
    /// connection of this API that was opening is taken.
    async fn accept(&mut self) -> Result<Option<Accepted<F::Tx, F::Rx>>, I::Error> {
        poll_fn(|cx| self.poll_accept(cx)).await
    }
}

impl<I, F, C, H> StreamListener<I, F, C, H>
where
    I: Incoming,
    F: Framing<I::Stream>,
    C: Clock,
    H: Future<Output = ()>,
{
    fn poll_accept(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<Accepted<F::Tx, F::Rx>>, I::Error>> {
        self.slots.wakers.set(self.index, cx.waker());
        if let Some(accepted) = self.slots.take(self.index) {
            return Poll::Ready(Ok(Some(accepted)));
        }
        let waker = Waker::from(self.slots.wakers.clone());
        let mut router = locked(&self.router);
        if let Err(e) = router.advance(&mut Context::from_waker(&waker), &self.slots) {
            return Poll::Ready(Err(e));
        }
        // the router may have opened one for this API
        if let Some(accepted) = self.slots.take(self.index) {
            return Poll::Ready(Ok(Some(accepted)));
        }
        if router.ended && router.handshakes.is_empty() {
            return Poll::Ready(Ok(None));
        }
        Poll::Pending
    }
}

impl<I: Incoming, F: Framing<I::Stream>, C, H> Drop for StreamListener<I, F, C, H> {
    fn drop(&mut self) {
        self.slots.close(self.index);
    }
}

/// The source and the connections opening on it, shared by the listeners of one source.
struct Router<I: Incoming, F: Framing<I::Stream>, C, H> {
    incoming: I,
    /// The source said no more streams come.
    ended: bool,
    handshakes: FuturesUnordered<H>,
    /// Opens a connection: [handshake] for the listener's types.
    #[expect(
        clippy::type_complexity,
        reason = "a function pointer; named, its type would only move the complexity"
    )]
    open: fn(I::Stream, Peer, Arc<Opening<F, C>>, Arc<Slots<F::Tx, F::Rx>>) -> H,
    opening: Arc<Opening<F, C>>,
}

impl<I, F, C, H> Router<I, F, C, H>
where
    I: Incoming,
    F: Framing<I::Stream>,
    H: Future<Output = ()>,
{
    /// Takes new streams and drives the connections opening, until neither moves. `cx` wakes every listener.
    fn advance(
        &mut self,
        cx: &mut Context<'_>,
        slots: &Arc<Slots<F::Tx, F::Rx>>,
    ) -> Result<(), I::Error> {
        loop {
            let mut moved = false;
            while let Poll::Ready(Some(())) = self.handshakes.poll_next_unpin(cx) {
                moved = true;
            }
            while !self.ended && self.handshakes.len() < HANDSHAKES_MAX {
                match self.incoming.poll_next_unpin(cx) {
                    Poll::Ready(Some(Ok((stream, peer)))) => {
                        let opening =
                            (self.open)(stream, peer, self.opening.clone(), slots.clone());
                        self.handshakes.push(opening);
                        moved = true;
                    }
                    Poll::Ready(Some(Err(e))) => return Err(e),
                    Poll::Ready(None) => self.ended = true,
                    Poll::Pending => break,
                }
            }
            if !moved {
                return Ok(());
            }
        }
    }
}

/// What every connection opening needs.
struct Opening<F, C> {
    framing: F,
    clock: C,
}

/// Opens one connection: reads what the client asks for, finds the listener that serves it and answers.
async fn handshake<S, F: Framing<S>, C: Clock>(
    stream: S,
    peer: Peer,
    opening: Arc<Opening<F, C>>,
    slots: Arc<Slots<F::Tx, F::Rx>>,
) {
    let Opening { framing, clock } = &*opening;
    let deadline = clock.now() + OPENING_TIMEOUT;
    // no opening in time, garbage, one this side answered itself, or the client left
    let Some(Some((stream, target))) = timeout(clock, deadline, framing.read(stream)).await else {
        return;
    };
    let (status, room) = match slots.route(&target) {
        Ok(room) => (Status::Ok, Some(room)),
        Err(status) => (status, None),
    };
    let Some(Some((tx, rx))) = timeout(clock, deadline, framing.answer(stream, status)).await
    else {
        return;
    };
    if let Some(room) = room {
        room.fill(Accepted { tx, rx, peer });
    }
}

/// `f`'s output, `None` when `deadline` comes first.
async fn timeout<T>(
    clock: &impl Clock,
    deadline: Instant,
    f: impl Future<Output = T>,
) -> Option<T> {
    let mut f = pin!(f);
    let mut timer = pin!(clock.wait_until(deadline));
    poll_fn(|cx| {
        if let Poll::Ready(t) = f.as_mut().poll(cx) {
            return Poll::Ready(Some(t));
        }
        timer.as_mut().poll(cx).map(|()| None)
    })
    .await
}

/// The listeners of one source and the connections opened for each.
struct Slots<Tx, Rx> {
    slots: Mutex<Vec<Slot<Tx, Rx>>>,
    wakers: Arc<Wakers>,
}

/// One listener's connections.
struct Slot<Tx, Rx> {
    /// The service; `None` for the first listener, which serves devices.
    name: Option<String>,
    /// The listener is still there.
    open: bool,
    queue: VecDeque<Accepted<Tx, Rx>>,
    /// Connections routed here and being answered.
    reserved: usize,
}

impl<Tx, Rx> Slots<Tx, Rx> {
    fn new() -> Self {
        Slots {
            slots: Mutex::new(vec![Slot {
                name: None,
                open: true,
                queue: VecDeque::new(),
                reserved: 0,
            }]),
            wakers: Arc::new(Wakers(Mutex::new(vec![None]))),
        }
    }

    /// A listener for the service `name`: its index.
    fn add(&self, name: &str) -> usize {
        let mut slots = locked(&self.slots);
        assert!(
            !slots
                .iter()
                .any(|s| s.open && s.name.as_deref() == Some(name)),
            "a listener for the service {name:?} exists already"
        );
        slots.push(Slot {
            name: Some(name.to_owned()),
            open: true,
            queue: VecDeque::new(),
            reserved: 0,
        });
        locked(&self.wakers.0).push(None);
        slots.len() - 1
    }

    /// The listener gone: its clients are refused from now on.
    fn close(&self, index: usize) {
        let mut slots = locked(&self.slots);
        let slot = &mut slots[index];
        slot.open = false;
        slot.queue.clear();
        drop(slots);
        locked(&self.wakers.0)[index] = None;
    }

    fn take(&self, index: usize) -> Option<Accepted<Tx, Rx>> {
        locked(&self.slots)[index].queue.pop_front()
    }

    /// Room for a client asking for `target`, or the status that refuses it.
    fn route(self: &Arc<Self>, target: &Target) -> Result<Room<Tx, Rx>, Status> {
        let mut slots = locked(&self.slots);
        let index = match target {
            Target::Device(_) if slots[0].open => 0,
            Target::Device(_) | Target::Hub => return Err(Status::UnknownDevice),
            Target::Service(name) => slots
                .iter()
                .position(|s| s.open && s.name.as_deref() == Some(name.as_str()))
                .ok_or(Status::UnknownService)?,
        };
        let slot = &mut slots[index];
        if slot.queue.len() + slot.reserved >= QUEUED_MAX {
            return Err(Status::Busy);
        }
        slot.reserved += 1;
        Ok(Room {
            slots: self.clone(),
            index,
            filled: false,
        })
    }
}

/// A place in a listener's queue, kept while the client is answered; given back unless filled.
struct Room<Tx, Rx> {
    slots: Arc<Slots<Tx, Rx>>,
    index: usize,
    filled: bool,
}

impl<Tx, Rx> Room<Tx, Rx> {
    fn fill(mut self, accepted: Accepted<Tx, Rx>) {
        let mut slots = locked(&self.slots.slots);
        let slot = &mut slots[self.index];
        slot.reserved -= 1;
        if slot.open {
            slot.queue.push_back(accepted);
        }
        drop(slots);
        self.filled = true;
        self.slots.wakers.wake_one(self.index);
    }
}

impl<Tx, Rx> Drop for Room<Tx, Rx> {
    fn drop(&mut self) {
        if !self.filled {
            locked(&self.slots.slots)[self.index].reserved -= 1;
        }
    }
}

/// The wakers of the listeners of one source. As a waker it wakes them all: whichever listener polls the source
/// next moves it for every other.
struct Wakers(Mutex<Vec<Option<Waker>>>);

impl Wakers {
    fn set(&self, index: usize, waker: &Waker) {
        let mut wakers = locked(&self.0);
        match &mut wakers[index] {
            Some(w) if w.will_wake(waker) => {}
            slot => *slot = Some(waker.clone()),
        }
    }

    fn wake_one(&self, index: usize) {
        let waker = locked(&self.0)[index].clone();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Wake for Wakers {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        // woken outside the lock: a waker may poll at once
        let wakers: Vec<Waker> = locked(&self.0).iter().flatten().cloned().collect();
        for waker in wakers {
            waker.wake();
        }
    }
}
