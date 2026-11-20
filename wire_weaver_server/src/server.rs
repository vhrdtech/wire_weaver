//! The server: connections from a listener, each served on its own, within limits, until shutdown.

use std::future::{Future, pending, poll_fn};
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use wire_weaver::{ConnectionId, WireWeaverAsyncApiBackend};
use ww_device::{Clock, LinkConfig};

use crate::backend::Backend;
use crate::connection::{self, Connection};
use crate::lifecycle::{Lifecycle, Stage};
use crate::listener::Listener;

/// How much a server and each of its connections may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Connections served at once. At the limit the server stops accepting, so further clients wait in the
    /// listener until one leaves.
    pub connections_max: usize,
    /// Requests of one connection read and waiting for their turn. At the limit the connection stops reading, so
    /// the client's transport pushes back.
    pub pending_max: usize,
    /// Events one connection's handlers sent and the connection has not written out yet. At the limit a handler
    /// waits in its send.
    pub events_max: usize,
    /// How long requests in flight get to finish after shutdown was asked for, before every connection left is
    /// disconnected.
    pub shutdown_grace: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            connections_max: 64,
            pending_max: 32,
            events_max: 64,
            shutdown_grace: Duration::from_secs(5),
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
        mut listener: L,
        make: M,
        shutdown: impl Future<Output = ()>,
    ) -> Result<(), L::Error> {
        enum Next<A> {
            Accepted(A),
            Closed,
            Shutdown,
            GraceOver,
        }

        let life = Lifecycle::new();
        let mut connections = FuturesUnordered::new();
        let mut shutdown = pin!(shutdown);
        let mut next_id: u32 = 0;
        // set once shutdown was asked for
        let mut grace_until = None;
        loop {
            if grace_until.is_some() && connections.is_empty() {
                return Ok(());
            }
            let accepting =
                grace_until.is_none() && connections.len() < self.limits.connections_max;
            let grace_runs = life.stage() == Stage::Drain;
            let next = {
                let mut accept = pin!(listener.accept());
                let clock = &self.clock;
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
                    if let Poll::Ready(Some(())) = connections.poll_next_unpin(cx) {
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
            };
            match next {
                Next::Accepted(accepted) => {
                    let accepted = accepted?;
                    let id = ConnectionId(next_id);
                    next_id = next_id.wrapping_add(1);
                    connections.push(connection::run(
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
                Next::Closed => {}
                Next::Shutdown => {
                    grace_until = Some(self.clock.now() + self.limits.shutdown_grace);
                    life.set(Stage::Drain);
                }
                Next::GraceOver => life.set(Stage::Abort),
            }
        }
    }
}
