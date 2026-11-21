//! The server: connections from a listener, each served on its own, within limits, until shutdown.

use std::future::{Future, pending, poll_fn};
use std::pin::{Pin, pin};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use wire_weaver::{ConnectionId, WireWeaverAsyncApiBackend};
use ww_device::{Clock, Instant, LinkConfig};

use crate::backend::Backend;
use crate::connection::{self, Connection};
use crate::lifecycle::{Lifecycle, Stage};
use crate::listener::{Accepted, Listener};
use crate::sync::locked;

/// How much a server and each of its connections may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Connections served at once. At the limit the server stops accepting, so further clients wait in the
    /// listener until one leaves.
    pub connections_max: usize,
    /// Requests that run at once, across all connections. A request that would go over it is answered with
    /// `ErrorKind::Overloaded` and its handler never runs, so the client may send it again later.
    pub requests_max: usize,
    /// Requests of one connection read and waiting for their turn. At the limit the connection stops reading, so
    /// the client's transport pushes back.
    pub pending_max: usize,
    /// Events one connection's handlers sent and the connection has not written out yet. At the limit a handler
    /// waits in its send.
    pub events_max: usize,
    /// How long requests in flight get to finish after shutdown was asked for, before every connection left is
    /// disconnected.
    pub shutdown_grace: Duration,
    /// How long a connection the server ends stays open after its disconnect message, for the client to read the
    /// reason and close first. A client that is writing when the connection closes under it sees a transport
    /// error instead of the reason.
    pub disconnect_linger: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            connections_max: 64,
            requests_max: 256,
            pending_max: 32,
            events_max: 64,
            shutdown_grace: Duration::from_secs(5),
            disconnect_linger: Duration::from_millis(200),
        }
    }
}

/// Makes the backend of each connection, once its link is up.
///
/// A closure `Fn(&Connection) -> B` is one, for backends whose medium is `()`. A server with a medium type of its
/// own implements the trait and says which medium a connection is.
pub trait MakeBackend {
    /// The generated server, with the state of one connection.
    type Backend: Backend;

    /// The backend of `connection`.
    fn make(&self, connection: &Connection) -> Self::Backend;

    /// What handlers of `connection` see as `cx.medium()`.
    fn medium(
        &self,
        connection: &Connection,
    ) -> <Self::Backend as WireWeaverAsyncApiBackend>::Medium;
}

impl<F, B> MakeBackend for F
where
    F: Fn(&Connection) -> B,
    B: Backend<Medium = ()>,
{
    type Backend = B;

    fn make(&self, connection: &Connection) -> B {
        self(connection)
    }

    fn medium(&self, _connection: &Connection) {}
}

/// A host server: see the [crate docs](crate).
pub struct Server<C> {
    clock: C,
    limits: Limits,
}

impl<C: Clock + Clone> Server<C> {
    /// A server with the default [Limits], its timers on `clock`.
    pub fn new(clock: C) -> Self {
        Server {
            clock,
            limits: Limits::default(),
        }
    }

    /// The same server with other limits.
    #[must_use]
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Serve the connections of `listener` until `shutdown` completes and the connections are done.
    ///
    /// Every connection goes through link setup as `link` describes the API, then `make` gives it a backend. The
    /// connections run inside this future, so it needs no spawner and takes backends that are not `Send`; it is
    /// `Send` itself when its parts are.
    ///
    /// When `shutdown` completes the server stops accepting; each connection answers the requests it has and
    /// disconnects, and what is left after [Limits::shutdown_grace] is disconnected as it is.
    ///
    /// # Errors
    ///
    /// The listener's error, when accepting fails: the connections are dropped with the future.
    pub async fn serve<L: Listener, M: MakeBackend>(
        &self,
        link: LinkConfig<'_>,
        listener: L,
        make: M,
        shutdown: impl Future<Output = ()>,
    ) -> Result<(), L::Error> {
        let life = Lifecycle::new();
        let mut gate = Gate::new(self.clock.clone(), self.limits, listener, shutdown);
        let mut running = FuturesUnordered::new();
        while let Some((accepted, id)) = gate.next(&life, &mut running).await? {
            running.push(connection::run(
                link.clone(),
                accepted.tx,
                accepted.rx,
                self.clock.clone(),
                id,
                accepted.peer,
                &make,
                self.limits,
                &life,
            ));
        }
        Ok(())
    }

    /// The same server with its connections handed out one by one, for the program to run as tasks of its own:
    /// on a multi-threaded runtime they then run on all its threads.
    ///
    /// ```ignore
    /// let mut accepting = server.accepting(link, listener, make, shutdown);
    /// while let Some(connection) = accepting.next().await? {
    ///     tokio::spawn(connection);
    /// }
    /// ```
    ///
    /// Limits and shutdown work as in [serve](Self::serve): [Accepting::next] gives no connection while
    /// [Limits::connections_max] of them run, and returns `None` once shutdown was asked for and every connection
    /// ended.
    pub fn accepting<L: Listener, M: MakeBackend, S: Future<Output = ()>>(
        &self,
        link: LinkConfig<'static>,
        listener: L,
        make: M,
        shutdown: S,
    ) -> Accepting<C, L, M, S> {
        Accepting {
            link,
            gate: Gate::new(self.clock.clone(), self.limits, listener, shutdown),
            make: Arc::new(make),
            life: Arc::new(Lifecycle::new()),
            running: Arc::new(Running::default()),
        }
    }
}

/// A server whose connections the program runs itself, see [Server::accepting].
pub struct Accepting<C, L, M, S> {
    link: LinkConfig<'static>,
    gate: Gate<C, L, S>,
    make: Arc<M>,
    life: Arc<Lifecycle>,
    running: Arc<Running>,
}

impl<C, L, M, S> Accepting<C, L, M, S>
where
    C: Clock + Clone + 'static,
    L: Listener<Tx: 'static, Rx: 'static>,
    M: MakeBackend + 'static,
    S: Future<Output = ()>,
{
    /// The next connection, to be run to its end: spawn it, or poll it with the others. `None` after shutdown,
    /// once every connection handed out has ended or was dropped.
    ///
    /// The connection is `Send` when the clock, the listener's message halves and the backend are.
    ///
    /// # Errors
    ///
    /// The listener's error, when accepting fails. Connections already handed out go on.
    pub async fn next(
        &mut self,
    ) -> Result<Option<impl Future<Output = ()> + 'static + use<C, L, M, S>>, L::Error> {
        let Some((accepted, id)) = self.gate.next(&self.life, &mut self.running).await? else {
            return Ok(None);
        };
        self.running.count.fetch_add(1, Ordering::AcqRel);
        // counted down when the connection ends, also when it is dropped unfinished
        let ended = Ended(self.running.clone());
        let connection = connection::run(
            self.link.clone(),
            accepted.tx,
            accepted.rx,
            self.gate.clock.clone(),
            id,
            accepted.peer,
            self.make.clone(),
            self.gate.limits,
            self.life.clone(),
        );
        Ok(Some(async move {
            connection.await;
            drop(ended);
        }))
    }
}

/// What both ways of running connections share: when to accept, shutdown and its grace time.
struct Gate<C, L, S> {
    clock: C,
    limits: Limits,
    listener: L,
    shutdown: Pin<Box<S>>,
    /// Set once shutdown was asked for.
    grace_until: Option<Instant>,
    next_id: u32,
}

/// The connections that run, as the gate sees them.
trait Connections {
    /// How many run now.
    fn len(&self) -> usize;

    /// Ready when one ended since the last call; the task is woken for the next.
    fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<()>;
}

impl<C: Clock, L: Listener, S: Future<Output = ()>> Gate<C, L, S> {
    fn new(clock: C, limits: Limits, listener: L, shutdown: S) -> Self {
        Gate {
            clock,
            limits,
            listener,
            shutdown: Box::pin(shutdown),
            grace_until: None,
            next_id: 0,
        }
    }

    /// The next accepted connection with its id; `None` once shutdown was asked for and no connection runs.
    async fn next(
        &mut self,
        life: &Lifecycle,
        connections: &mut impl Connections,
    ) -> Result<Option<(Accepted<L::Tx, L::Rx>, ConnectionId)>, L::Error> {
        loop {
            if self.grace_until.is_some() && connections.len() == 0 {
                return Ok(None);
            }
            match self.wait(life.stage() == Stage::Drain, connections).await {
                Next::Accepted(accepted) => {
                    let id = ConnectionId(self.next_id);
                    self.next_id = self.next_id.wrapping_add(1);
                    return Ok(Some((accepted?, id)));
                }
                Next::Closed => {}
                Next::Shutdown => {
                    self.grace_until = Some(self.clock.now() + self.limits.shutdown_grace);
                    life.set(Stage::Drain);
                }
                Next::GraceOver => life.set(Stage::Abort),
            }
        }
    }

    /// Wait for what the gate acts on next. `grace_runs`: shutdown was asked for and the grace time is not over.
    async fn wait(
        &mut self,
        grace_runs: bool,
        connections: &mut impl Connections,
    ) -> Next<Result<Accepted<L::Tx, L::Rx>, L::Error>> {
        let accepting =
            self.grace_until.is_none() && connections.len() < self.limits.connections_max;
        let grace_until = self.grace_until;
        let mut accept = pin!(self.listener.accept());
        let clock = &self.clock;
        let shutdown = &mut self.shutdown;
        let mut grace = pin!(async {
            match grace_until {
                Some(at) => clock.wait_until(at).await,
                None => pending().await,
            }
        });
        poll_fn(|cx| {
            if grace_until.is_none() && shutdown.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Next::Shutdown);
            }
            if connections.poll_closed(cx).is_ready() {
                return Poll::Ready(Next::Closed);
            }
            if accepting && let Poll::Ready(accepted) = accept.as_mut().poll(cx) {
                return Poll::Ready(Next::Accepted(accepted));
            }
            if grace_runs && grace.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Next::GraceOver);
            }
            Poll::Pending
        })
        .await
    }
}

/// What a [Gate] acts on next.
enum Next<A> {
    Accepted(A),
    /// A connection ended.
    Closed,
    /// Shutdown was asked for.
    Shutdown,
    /// The grace time after shutdown is over.
    GraceOver,
}

/// Connections polled inside the server's future.
impl<F: Future<Output = ()>> Connections for FuturesUnordered<F> {
    fn len(&self) -> usize {
        FuturesUnordered::len(self)
    }

    fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        match self.poll_next_unpin(cx) {
            Poll::Ready(Some(())) => Poll::Ready(()),
            // none runs, or none ended
            Poll::Ready(None) | Poll::Pending => Poll::Pending,
        }
    }
}

/// How many connections handed out by [Accepting] run, and who to wake when one ends.
#[derive(Default)]
struct Running {
    count: AtomicUsize,
    /// Ended and not yet reported by `poll_closed`.
    ended: AtomicUsize,
    server: Mutex<Option<Waker>>,
}

/// Counts a handed-out connection down when dropped.
struct Ended(Arc<Running>);

impl Drop for Ended {
    fn drop(&mut self) {
        self.0.count.fetch_sub(1, Ordering::AcqRel);
        self.0.ended.fetch_add(1, Ordering::AcqRel);
        let waker = locked(&self.0.server).take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Connections for Arc<Running> {
    fn len(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        // the waker first: a connection that ends after the check below finds it
        *locked(&self.server) = Some(cx.waker().clone());
        if self.ended.swap(0, Ordering::AcqRel) > 0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
