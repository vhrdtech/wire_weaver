//! One connection: link setup, a backend of its own, requests in order, until the link goes down.

use std::collections::VecDeque;
use std::future::{Future, pending, poll_fn};
use std::ops::Deref;
use std::pin::{Pin, pin};
use std::task::Poll;
use std::time::Duration;

use futures_util::future::{AbortHandle, AbortRegistration, Abortable, Either, select};
use futures_util::stream::{FuturesUnordered, StreamExt};
use wire_weaver::authorize::{Pending, Verdict};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, Error as ShrinkWrapError};
use wire_weaver::ww_version::WireVersion;
use wire_weaver::{CallContext, ConnectionId, EventWriter, Peer, Priority};
use ww_client_server::{ErrorKind, Request, RequestKind, err_seq};
use ww_device::link::{Phase, Received, Transmit};
use ww_device::{
    Clock, DeviceLink, Instant, LinkConfig, LinkEvent, MessageRx, MessageTx, PeerMeta, PeerVersion,
    SendError,
};

use crate::authorize::Authorization;
use crate::backend::Backend;
use crate::lifecycle::{Lifecycle, Running, Stage};
use crate::outbox::{Outbox, OutboxSink};
use crate::pump::{PumpSetOps, Pumps};
use crate::server::{Limits, MakeBackend};

/// A connection whose link is up, as its backend's factory sees it.
///
/// `P` is its pump handle: [Pumps] for `Send` streams, [LocalPumps](crate::LocalPumps) for streams that are not
/// (SRV-73), as the [MakeBackend] picks.
#[derive(Clone, Debug)]
pub struct Connection<P = Pumps> {
    /// Tells the connections of one server apart; never reused while the server runs.
    pub id: ConnectionId,
    /// Who connected, as the transport saw it.
    pub peer: Peer,
    /// What the client says about itself (program, user, host, version): a label for logs, nothing to decide on.
    pub client: PeerMeta,
    /// The API version the client was built against, to see version skew; `None` if it sent none.
    pub client_version: Option<PeerVersion>,
    /// The longest message either side takes on this connection, agreed in link setup.
    pub message_len_max: usize,
    /// The connection's pumps: a backend keeps a clone, so that its stream open handlers can add theirs.
    pub pumps: P,
}

/// A connection whose streams need not be `Send`, see [Local](crate::Local).
pub type LocalConnection = Connection<crate::LocalPumps>;

impl<P> Connection<P> {
    /// Who asks on this connection, as handlers read it from `cx.call()`: the peer and the connection id. Each
    /// request adds its own `seq` and priority.
    pub fn origin(&self) -> CallContext {
        let mut origin = CallContext::new(0).with_peer(self.peer);
        origin.connection = self.id;
        origin
    }
}

/// What became of a request.
struct Done {
    request: Vec<u8>,
    scratch: Vec<u8>,
    events: Vec<u8>,
    /// Where the reply is in `scratch`; `None` when the dispatcher could not even write an error.
    reply: Option<(usize, usize)>,
}

/// What the work a connection waited on came to.
enum Finished {
    /// A request ran.
    Answer(Done),
    /// A request was dropped before it finished: its deadline ran out or the client cancelled it (SRV-47).
    Dropped(Dropped),
    /// The backend has something to do on its own.
    Woke,
    /// The backend did it; the event buffer comes back.
    Polled(Vec<u8>),
}

/// A request dropped before its handler finished, with what its answer needs.
struct Dropped {
    seq: u32,
    priority: Priority,
    why: Why,
}

/// Why a request is answered without its handler's answer (SRV-12, SRV-47).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Why {
    /// The server's cap on requests across connections.
    Overloaded,
    /// The request's deadline ran out, queued or running.
    Deadline,
    /// The client sent a `Cancel` for it.
    Cancelled,
    /// The server's [Authorize](crate::Authorize) hook denied it (SRV-46).
    Denied,
}

impl Why {
    fn error(self) -> ww_client_server::Error<'static> {
        match self {
            Why::Overloaded => ww_client_server::Error::overloaded(),
            Why::Deadline => {
                ww_client_server::Error::new(err_seq::REQUEST_DEADLINE, ErrorKind::DeadlineExceeded)
            }
            Why::Cancelled => {
                ww_client_server::Error::new(err_seq::REQUEST_CANCELLED, ErrorKind::Cancelled)
            }
            Why::Denied => ww_client_server::Error::new(err_seq::REQUEST_DENIED, ErrorKind::Denied),
        }
    }
}

/// A request read and not started yet, with the terms its context sets (SRV-47).
struct Queued {
    request: Vec<u8>,
    /// 0 when the client expects no answer, or the request is unreadable (the dispatcher answers it).
    seq: u32,
    /// When the request's deadline runs out on this server's clock.
    deadline: Option<Instant>,
    /// The caller's trace, for the handler's `CallContext`.
    trace: Option<wire_weaver::TraceContext>,
}

impl Queued {
    /// Read the head and the context of `request`; one that can't be read is queued as it is, for the dispatcher
    /// to answer.
    fn read(request: &[u8], now: Instant) -> Result<Queued, u32> {
        let mut queued = Queued {
            request: request.to_vec(),
            seq: 0,
            deadline: None,
            trace: None,
        };
        let Ok(head) = Request::from_ww_bytes(request) else {
            return Ok(queued);
        };
        if let RequestKind::Cancel { seq } = head.kind {
            return Err(seq.0);
        }
        queued.seq = head.seq.0;
        if let Some(context) = head.context.and_then(|context| context.read().ok()) {
            queued.trace = context.trace;
            queued.deadline = context
                .deadline
                .map(|left| now + Duration::from_millis(left.get().0.into()));
        }
        Ok(queued)
    }

    /// The handler's `CallContext`: the connection's `origin` with the request's trace and what is left of its
    /// deadline at `now`. The generated entry adds `seq` and the priority.
    fn call(&self, origin: &CallContext, now: Instant) -> CallContext {
        let mut call = *origin;
        call.trace = self.trace;
        call.deadline = self.deadline.map(|at| {
            let left = at.saturating_duration_since(now).as_millis();
            ww_client_server::ww_si::Millis::new(u32::try_from(left).unwrap_or(u32::MAX))
        });
        call
    }
}

enum Step {
    Done(Finished),
    /// A queued request's deadline ran out.
    Expired,
    Events,
    /// A message came; false when reading failed.
    Message(bool),
    Timeout,
    Stage(Stage),
}

/// What the connection's loop does after a step.
#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    /// Link setup finished.
    Up,
    /// The work the step was given is done.
    Finished,
    /// The connection is over.
    End,
}

struct Conn<'a, Tx, Rx, C, S> {
    link: DeviceLink<'a>,
    tx: Tx,
    rx: Rx,
    clock: C,
    /// Link messages are built here.
    link_scratch: Vec<u8>,
    /// Requests read and not started yet, oldest first.
    pending: VecDeque<Queued>,
    /// The requests that run, by seq, to cancel them (SRV-47); those without a seq can't be named.
    running: Vec<(u32, AbortHandle)>,
    /// The `ww_client_server` wire version of the client, from link setup: which error kinds it reads.
    client_model: Option<WireVersion>,
    outbox: &'a Outbox,
    pumps: &'a S,
    /// Pump items are serialized here.
    pump_buffer: Vec<u8>,
    /// Replies and events are built in these; a request takes two, its answer gives them back, so there are as many
    /// as requests ran at once.
    buffers: Vec<Vec<u8>>,
    /// The length of each of `buffers`: the medium's message limit.
    buffer_len: usize,
    limits: Limits,
    life: &'a Lifecycle,
    /// The server's stage as this connection last saw it.
    stage: Stage,
}

/// No request runs.
const IDLE: Option<Pin<&mut std::future::Pending<Finished>>> = None;

impl<Tx: MessageTx, Rx: MessageRx, C: Clock, S: PumpSetOps> Conn<'_, Tx, Rx, C, S> {
    /// Wait for the next thing to do and do it. `work` is the request that runs, if one does.
    async fn step<W: Future<Output = Finished>>(&mut self, work: Option<Pin<&mut W>>) -> Flow {
        let step = self.wait(work).await;
        self.handle(step).await
    }

    async fn wait<W: Future<Output = Finished>>(&mut self, mut work: Option<Pin<&mut W>>) -> Step {
        let deadline = self.link.poll_timeout();
        let clock = &self.clock;
        // a server that shuts down takes no new requests; a full queue pushes back on the client
        let read = self.stage == Stage::Run && self.pending.len() < self.limits.pending_max;
        let mut message = pin!(self.rx.wait_message());
        let mut timer = pin!(async {
            match deadline {
                Some(at) => clock.wait_until(at).await,
                None => pending().await,
            }
        });
        let expires = self
            .pending
            .iter()
            .filter_map(|queued| queued.deadline)
            .min();
        let mut expiry = pin!(async {
            match expires {
                Some(at) => clock.wait_until(at).await,
                None => pending().await,
            }
        });
        let (outbox, life, known, pumps) = (self.outbox, self.life, self.stage, self.pumps);
        let pump_buffer = &mut self.pump_buffer;
        poll_fn(|cx| {
            let stage = life.watch(cx);
            if stage > known {
                return Poll::Ready(Step::Stage(stage));
            }
            if let Some(work) = work.as_mut()
                && let Poll::Ready(done) = work.as_mut().poll(cx)
            {
                return Poll::Ready(Step::Done(done));
            }
            if pumps.poll_into(cx, outbox, pump_buffer) || !outbox.is_empty() {
                return Poll::Ready(Step::Events);
            }
            if read && let Poll::Ready(r) = message.as_mut().poll(cx) {
                return Poll::Ready(Step::Message(r.is_ok()));
            }
            if timer.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Step::Timeout);
            }
            if expiry.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Step::Expired);
            }
            Poll::Pending
        })
        .await
    }

    async fn handle(&mut self, step: Step) -> Flow {
        let now = self.clock.now();
        let mut flow = Flow::Continue;
        match step {
            Step::Done(finished) => {
                match finished {
                    Finished::Answer(done) => {
                        self.forget(Request::peek_seq(&done.request).unwrap_or(0));
                        self.answer(done).await;
                    }
                    Finished::Dropped(dropped) => {
                        self.forget(dropped.seq);
                        self.answer_error(dropped.seq, dropped.priority, dropped.why)
                            .await;
                    }
                    Finished::Woke => {}
                    Finished::Polled(events) => {
                        self.send_events().await;
                        self.buffers.push(events);
                    }
                }
                // what the request or the backend's turn added starts after its answer
                self.pumps.start_added();
                flow = Flow::Finished;
            }
            Step::Events => self.send_events().await,
            Step::Expired => self.expire(now).await,
            Step::Message(false) => self.link.on_transport_down(),
            Step::Message(true) => self.on_message(now).await,
            Step::Timeout => self.link.handle_timeout(now),
            Step::Stage(stage) => {
                self.stage = stage;
                if stage == Stage::Abort {
                    self.disconnect().await;
                    return Flow::End;
                }
            }
        }
        self.drain_transmit().await;
        match self.link.poll_event() {
            Some(LinkEvent::Up) => Flow::Up,
            Some(LinkEvent::Down(_)) => Flow::End,
            None if self.link.phase() == Phase::Down => Flow::End,
            None => flow,
        }
    }

    /// Take in the message the medium has: a request joins the queue, a link message is acted on.
    async fn on_message(&mut self, now: ww_device::Instant) {
        let (mut loopback, mut cancel) = (None, None);
        if let Some((kind, message)) = self.rx.message() {
            match self.link.handle_message(now, kind, message) {
                Some(Received::Data(request)) => match Queued::read(request, now) {
                    Ok(queued) => self.pending.push_back(queued),
                    Err(seq) => cancel = Some(seq),
                },
                Some(Received::Loopback { repeat, seq, data }) => {
                    loopback = Some((repeat, seq, data.to_vec()));
                }
                None => {}
            }
        }
        self.rx.consume();
        if let Some((repeat, seq, data)) = loopback {
            self.loopback(repeat, seq, &data).await;
        }
        if let Some(seq) = cancel {
            self.cancel(seq).await;
        }
    }

    /// The client's `Cancel` of request `seq` (SRV-47): a queued one is answered `Cancelled` here, a running one
    /// is dropped and answered when its future ends. One that finished already, or that never was, is not
    /// answered again.
    async fn cancel(&mut self, seq: u32) {
        if seq == 0 {
            return;
        }
        if let Some(at) = self.pending.iter().position(|queued| queued.seq == seq) {
            let queued = self.pending.remove(at).expect("found above");
            let priority = Request::peek_priority(&queued.request).unwrap_or_default();
            self.answer_error(seq, priority, Why::Cancelled).await;
        } else if let Some((_, handle)) = self.running.iter().find(|(s, _)| *s == seq) {
            handle.abort();
        }
    }

    /// Answer the queued requests whose deadline ran out by `now` with `DeadlineExceeded`; their handlers never ran.
    async fn expire(&mut self, now: Instant) {
        while let Some(at) = self
            .pending
            .iter()
            .position(|queued| queued.deadline.is_some_and(|deadline| deadline <= now))
        {
            let queued = self.pending.remove(at).expect("found above");
            let priority = Request::peek_priority(&queued.request).unwrap_or_default();
            self.answer_error(queued.seq, priority, Why::Deadline).await;
        }
    }

    /// Register a request that starts as running, so a `Cancel` can drop it; `None` for one without a seq.
    fn track(&mut self, seq: u32) -> Option<AbortRegistration> {
        if seq == 0 {
            return None;
        }
        let (handle, registration) = AbortHandle::new_pair();
        self.running.push((seq, handle));
        Some(registration)
    }

    /// A request that ran is no longer cancelled by its seq.
    fn forget(&mut self, seq: u32) {
        if seq != 0
            && let Some(at) = self.running.iter().position(|(s, _)| *s == seq)
        {
            self.running.swap_remove(at);
        }
    }

    /// Send a data message; an error is left to the caller, a failed write takes the link down.
    async fn send(&mut self, message: &[u8]) -> Result<(), SendError> {
        self.link.check_send(message.len())?;
        if self
            .tx
            .write_message(DeviceLink::data_kind(), message)
            .await
            .is_err()
        {
            self.link.on_transport_down();
            return Err(SendError::Transport);
        }
        self.link.on_data_written(self.clock.now());
        Ok(())
    }

    async fn send_events(&mut self) {
        while let Some(event) = self.outbox.pop() {
            // an event the link refuses now (it went down) is dropped, as a device's sink drops it
            if self.send(&event.event).await.is_ok() && event.urgent {
                self.link.flush_now();
            }
        }
    }

    /// Send the answer of a request: the events its handler sent first, then the reply.
    async fn answer(&mut self, mut done: Done) {
        self.send_events().await;
        let sent = match done.reply {
            Some((start, len)) if len > 0 => self.send(&done.scratch[start..start + len]).await,
            Some(_) => Ok(()),
            None => {
                if let Some(error) =
                    ww_device::generic_error_reply(&done.request, &mut done.scratch)
                {
                    let _ = self.send(error).await;
                }
                Ok(())
            }
        };
        if sent == Err(SendError::TooBig)
            && let Some(error) = ww_device::over_limit_reply(&done.request, &mut done.scratch)
        {
            let _ = self.send(error).await;
        }
        // the reply to an urgent request goes out at once, the rest within the accumulation window
        if ww_client_server::Request::peek_priority(&done.request)
            .unwrap_or_default()
            .is_urgent()
        {
            self.link.flush_now();
        }
        self.buffers.push(done.scratch);
        self.buffers.push(done.events);
    }

    /// A buffer for a reply or events, back with the answer.
    fn take_buffer(&mut self) -> Vec<u8> {
        self.buffers
            .pop()
            .unwrap_or_else(|| vec![0; self.buffer_len])
    }

    /// Whether `queued` may run, as the server's hook says: `None` when it was denied, answered `Denied` here;
    /// `Some` with the answer to wait for when the hook's verdict is pending.
    async fn admit(
        &mut self,
        queued: &Queued,
        origin: &CallContext,
        authorization: Option<&Authorization>,
    ) -> Option<Option<Pending>> {
        let Some(authorization) = authorization else {
            return Some(None);
        };
        let priority = Request::peek_priority(&queued.request).unwrap_or_default();
        let mut call = queued.call(origin, self.clock.now());
        call.seq = queued.seq;
        call.priority = priority;
        match authorization.admits(&call, &queued.request) {
            Verdict::Allow => return Some(None),
            Verdict::Pending(pending) => return Some(Some(pending)),
            Verdict::Deny => {}
        }
        self.answer_error(queued.seq, priority, Why::Denied).await;
        None
    }

    /// Answer a request with `Overloaded`, its handler never ran.
    async fn refuse(&mut self, queued: Queued) {
        let priority = Request::peek_priority(&queued.request).unwrap_or_default();
        self.answer_error(queued.seq, priority, Why::Overloaded)
            .await;
    }

    /// Answer request `seq` with the error of `why`, as this client reads it ([ErrorKind::for_client]); a request
    /// without a seq expects no answer.
    async fn answer_error(&mut self, seq: u32, priority: Priority, why: Why) {
        if seq == 0 {
            return;
        }
        let mut error = why.error();
        error.kind = error.kind.for_client(self.client_model);
        let mut scratch = self.take_buffer();
        if let Ok(reply) = ww_client_server::util::ser_err_event(&mut scratch, seq, error) {
            let len = reply.len();
            let start = reply.as_ptr().addr() - scratch.as_ptr().addr();
            let _ = self.send(&scratch[start..start + len]).await;
            if priority.is_urgent() {
                self.link.flush_now();
            }
        }
        self.buffers.push(scratch);
    }

    async fn loopback(&mut self, repeat: u32, seq: u32, data: &[u8]) {
        for seq in ww_device::loopback_seqs(repeat, seq) {
            let Some((kind, bytes)) = ww_device::loopback_reply(seq, data, &mut self.link_scratch)
            else {
                return;
            };
            if self.tx.write_message(kind, bytes).await.is_err() {
                self.link.on_transport_down();
                return;
            }
            self.link.on_data_written(self.clock.now());
        }
    }

    /// Write out everything the link core wants to send.
    async fn drain_transmit(&mut self) {
        loop {
            let now = self.clock.now();
            let Some(transmit) = self.link.poll_transmit(now, &mut self.link_scratch) else {
                break;
            };
            let written = match transmit {
                Transmit::NewSession => {
                    self.tx.new_session();
                    Ok(())
                }
                Transmit::Message { kind, bytes } => self.tx.write_message(kind, bytes).await,
                Transmit::Hello(hello) => self.tx.write_hello(&hello).await,
                Transmit::Flush => self.tx.flush().await,
            };
            if written.is_err() {
                self.link.on_transport_down();
                break;
            }
        }
    }

    /// Tell the client that the server goes away, send it now, and give the client the linger time to close
    /// first: closing under a client that is writing would show it a transport error, not the reason. Stream items
    /// ready by then go out before it, so a client hears the news that came with the shutdown (the hub's
    /// `Event::Handover`).
    async fn disconnect(&mut self) {
        let (pumps, outbox, buf) = (self.pumps, self.outbox, &mut self.pump_buffer);
        poll_fn(|cx| {
            pumps.poll_into(cx, outbox, buf);
            Poll::Ready(())
        })
        .await;
        self.send_events().await;
        self.link.disconnect(self.life.reason());
        self.drain_transmit().await;
        let _ = self.tx.flush().await;
        let until = self.clock.now() + self.limits.disconnect_linger;
        loop {
            let read = {
                let message = pin!(self.rx.wait_message());
                let linger = pin!(self.clock.wait_until(until));
                match select(message, linger).await {
                    Either::Left((read, _)) => read.is_ok(),
                    Either::Right(_) => false,
                }
            };
            if !read {
                return;
            }
            // what the client still sends is not served any more
            self.rx.consume();
        }
    }
}

/// A request and the two buffers it runs with: its reply and its events are built there.
struct Work {
    request: Vec<u8>,
    scratch: Vec<u8>,
    events: Vec<u8>,
    /// The handler's `CallContext`.
    call: CallContext,
}

/// What ends a request before its handler finishes (SRV-47): its deadline, and a `Cancel` through its
/// registration.
struct Bounds<C> {
    seq: u32,
    priority: Priority,
    deadline: Option<Instant>,
    cancel: Option<AbortRegistration>,
    clock: C,
}

/// Run `work` until it finishes, its deadline runs out or the client cancels it. Dropping it drops the handler's
/// future, wherever it waits.
async fn bounded<C: Clock>(work: impl Future<Output = Finished>, bounds: Bounds<C>) -> Finished {
    let Bounds {
        seq,
        priority,
        deadline,
        cancel,
        clock,
    } = bounds;
    let timed = async {
        let timer = async {
            match deadline {
                Some(at) => clock.wait_until(at).await,
                None => pending().await,
            }
        };
        match select(pin!(work), pin!(timer)).await {
            Either::Left((finished, _)) => finished,
            Either::Right(_) => Finished::Dropped(Dropped {
                seq,
                priority,
                why: Why::Deadline,
            }),
        }
    };
    let Some(cancel) = cancel else {
        return timed.await;
    };
    match Abortable::new(timed, cancel).await {
        Ok(finished) => finished,
        Err(_) => Finished::Dropped(Dropped {
            seq,
            priority,
            why: Why::Cancelled,
        }),
    }
}

/// Run `work` once the hook's `pending` answer allows it (SRV-72); a request it denies is answered `Denied` and never
/// runs. The wait is part of the request: its deadline and a cancel end it, and drop the question.
async fn approved(
    pending: Option<Pending>,
    work: impl Future<Output = Finished>,
    seq: u32,
    priority: Priority,
) -> Finished {
    if let Some(pending) = pending
        && pending.await.is_err()
    {
        return Finished::Dropped(Dropped {
            seq,
            priority,
            why: Why::Denied,
        });
    }
    work.await
}

/// Run one request alone, through the backend's `&mut self` entry.
async fn exclusive<B: Backend>(
    backend: &mut B,
    medium: B::Medium,
    outbox: &Outbox,
    mut work: Work,
) -> Finished {
    let (base, size) = (work.scratch.as_ptr().addr(), work.scratch.len());
    let mut out = EventWriter::new(OutboxSink(outbox), &mut work.events);
    let reply = backend
        .process_exclusive_bytes(
            &work.call,
            &mut out,
            medium,
            &work.request,
            &mut work.scratch,
        )
        .await;
    let reply = reply_range(base, size, reply);
    work.done(reply)
}

/// Run one shared request through the backend's `&self` entry, beside others. `_running` counts it as running
/// until it is done.
async fn shared<B: Backend>(
    backend: &B,
    medium: B::Medium,
    outbox: &Outbox,
    mut work: Work,
    _running: Running<'_>,
) -> Finished {
    let (base, size) = (work.scratch.as_ptr().addr(), work.scratch.len());
    let mut out = EventWriter::new(OutboxSink(outbox), &mut work.events);
    let reply = backend
        .process_shared_bytes(
            &work.call,
            &mut out,
            medium,
            &work.request,
            &mut work.scratch,
        )
        .await;
    let reply = reply_range(base, size, reply);
    work.done(reply)
}

/// Where the dispatcher's reply is in its scratch buffer, which starts at `base` and is `size` long: kept as a
/// range, so the buffer can move with it. `None` when the dispatcher could not even write an error.
fn reply_range(
    base: usize,
    size: usize,
    reply: Result<&[u8], ShrinkWrapError>,
) -> Option<(usize, usize)> {
    match reply {
        Ok([]) => Some((0, 0)),
        Ok(reply) => {
            let start = reply.as_ptr().addr().wrapping_sub(base);
            let len = reply.len();
            assert!(
                start.checked_add(len).is_some_and(|end| end <= size),
                "the dispatcher's reply is not inside its scratch buffer"
            );
            Some((start, len))
        }
        Err(_) => None,
    }
}

impl Work {
    fn done(self, reply: Option<(usize, usize)>) -> Finished {
        Finished::Answer(Done {
            request: self.request,
            scratch: self.scratch,
            events: self.events,
            reply,
        })
    }
}

/// The next shared request that finished; never completes while none runs.
async fn next_finished<F: Future<Output = Finished>>(
    running: &mut FuturesUnordered<F>,
) -> Finished {
    poll_fn(|cx| match running.poll_next_unpin(cx) {
        Poll::Ready(Some(finished)) => Poll::Ready(finished),
        Poll::Ready(None) | Poll::Pending => Poll::Pending,
    })
    .await
}

/// Let the backend send what is due.
async fn poll<B: Backend>(
    backend: &mut B,
    medium: B::Medium,
    outbox: &Outbox,
    now: ww_device::Instant,
    mut events: Vec<u8>,
) -> Finished {
    let mut out = EventWriter::new(OutboxSink(outbox), &mut events);
    backend.poll(&mut out, medium, now).await;
    Finished::Polled(events)
}

/// Serve one accepted connection until its link goes down or the server stops.
#[expect(
    clippy::too_many_arguments,
    reason = "two call sites, Server::serve and Accepting::next"
)]
pub(crate) async fn run<M: MakeBackend>(
    mut config: LinkConfig<'_>,
    tx: impl MessageTx,
    mut rx: impl MessageRx,
    clock: impl Clock + Clone,
    id: ConnectionId,
    peer: Peer,
    make: impl Deref<Target = M>,
    limits: Limits,
    life: impl Deref<Target = Lifecycle>,
    authorization: Option<impl Deref<Target = Authorization>>,
) {
    let (make, life) = (&*make, &*life);
    let authorization = authorization.as_deref();
    // deadlines and cancels are acted on here (SRV-47), so clients may send them
    config.request_context = true;
    rx.wait_connected().await;
    let outbox = Outbox::new(limits.events_max);
    // the medium's limit: replies and events are built in buffers of this size, longer replies of a server
    // with an allocator in buffers of their own
    let buffer_len = rx.max_message_len();
    let (pumps, handle) = <M::Pumps as crate::pump::Kind>::new_set();
    let mut conn = Conn {
        link: DeviceLink::new(config, rx.max_message_len()),
        tx,
        rx,
        clock,
        link_scratch: vec![0; buffer_len],
        pending: VecDeque::new(),
        running: Vec::new(),
        client_model: None,
        outbox: &outbox,
        pumps: &pumps,
        pump_buffer: vec![0; buffer_len],
        buffers: vec![],
        buffer_len,
        limits,
        life,
        stage: Stage::Run,
    };
    conn.rx.reset();
    conn.tx.reset();
    conn.link.on_transport_up(conn.clock.now());
    conn.drain_transmit().await;
    if !conn.set_up().await {
        return;
    }
    outbox.set_message_len_max(conn.link.agreed_max_message_len());
    conn.client_model = conn
        .link
        .peer_version()
        .and_then(|version| version.model_wire);
    let info = Connection {
        id,
        peer,
        client: conn.link.peer_meta().cloned().unwrap_or_default(),
        client_version: conn.link.peer_version().cloned(),
        message_len_max: conn.link.agreed_max_message_len(),
        pumps: handle,
    };
    let origin = info.origin();
    let mut backend = make.make(&info);
    let medium = make.medium(&info);
    serve(&mut conn, &mut backend, medium, &origin, authorization).await;
    // the link is gone and no request runs: the backend lets go of what the client held (SRV-74)
    let until = conn.clock.now() + limits.shutdown_grace;
    select(pin!(backend.end()), pin!(conn.clock.wait_until(until))).await;
}

/// Serve the requests of a connection whose link is up, and the backend's own turns, until the connection ends.
async fn serve<Tx: MessageTx, Rx: MessageRx, C: Clock + Clone, S: PumpSetOps, B: Backend>(
    conn: &mut Conn<'_, Tx, Rx, C, S>,
    backend: &mut B,
    medium: B::Medium,
    origin: &CallContext,
    authorization: Option<&Authorization>,
) {
    let (outbox, life, limits) = (conn.outbox, conn.life, conn.limits);
    loop {
        if conn
            .pending
            .front()
            .is_some_and(|queued| B::is_shared(&queued.request))
        {
            if !conn
                .run_shared(backend, medium, origin, authorization)
                .await
            {
                return;
            }
        } else if let Some(queued) = conn.pending.pop_front() {
            let Some(pending) = conn.admit(&queued, origin, authorization).await else {
                // nothing ran: no turn for the backend either
                continue;
            };
            let Some(_running) = life.start_request(limits.requests_max) else {
                // nothing ran: no turn for the backend either
                conn.refuse(queued).await;
                continue;
            };
            let (work, bounds) = conn.work(queued, origin);
            let work = exclusive(backend, medium, outbox, work);
            let work = pin!(bounded(
                approved(pending, work, bounds.seq, bounds.priority),
                bounds
            ));
            if !conn.finish(work).await {
                return;
            }
        } else {
            match conn.idle(backend).await {
                Idle::Woke => {}
                // the request first: the backend's turn comes after it
                Idle::Request => continue,
                Idle::End => return,
            }
        }
        // the backend's own turn, to its end
        let (events, now) = (conn.take_buffer(), conn.clock.now());
        let work = pin!(poll(backend, medium, outbox, now, events));
        if !conn.finish(work).await {
            return;
        }
    }
}

/// How a wait without a request ended.
enum Idle {
    /// The backend has work of its own.
    Woke,
    /// A request came.
    Request,
    /// The connection is over.
    End,
}

impl<Tx: MessageTx, Rx: MessageRx, C: Clock + Clone, S: PumpSetOps> Conn<'_, Tx, Rx, C, S> {
    /// Link setup. False when the connection ended first; a server that shuts down meanwhile drops it.
    async fn set_up(&mut self) -> bool {
        loop {
            match self.step(IDLE).await {
                Flow::Up => return true,
                Flow::End => return false,
                Flow::Continue | Flow::Finished => {
                    if self.stage != Stage::Run {
                        return false;
                    }
                }
            }
        }
    }

    /// Run the shared requests first in line side by side, and each one that comes while they run, until none
    /// runs. A request that runs alone waits for the shared ones before it and holds those after it (D4), since it
    /// is started only once this returns. False when the connection ended first.
    async fn run_shared<B: Backend>(
        &mut self,
        backend: &B,
        medium: B::Medium,
        origin: &CallContext,
        authorization: Option<&Authorization>,
    ) -> bool {
        let (outbox, life) = (self.outbox, self.life);
        let mut running = FuturesUnordered::new();
        loop {
            while running.len() < self.limits.calls_max
                && self
                    .pending
                    .front()
                    .is_some_and(|queued| B::is_shared(&queued.request))
            {
                let queued = self.pending.pop_front().expect("checked above");
                let Some(pending) = self.admit(&queued, origin, authorization).await else {
                    continue;
                };
                let Some(counted) = life.start_request(self.limits.requests_max) else {
                    self.refuse(queued).await;
                    continue;
                };
                let (work, bounds) = self.work(queued, origin);
                let work = shared(backend, medium, outbox, work, counted);
                running.push(bounded(
                    approved(pending, work, bounds.seq, bounds.priority),
                    bounds,
                ));
            }
            if running.is_empty() {
                return true;
            }
            // a request that runs alone waits for the shared ones before it until each one finishes, runs out of
            // its deadline or is cancelled (SRV-47)
            let next = pin!(next_finished(&mut running));
            if self.step(Some(next)).await == Flow::End {
                return false;
            }
        }
    }

    /// A request with the buffers and the `CallContext` it runs with, and what ends it early.
    fn work(&mut self, queued: Queued, origin: &CallContext) -> (Work, Bounds<C>) {
        let now = self.clock.now();
        let bounds = Bounds {
            seq: queued.seq,
            priority: Request::peek_priority(&queued.request).unwrap_or_default(),
            deadline: queued.deadline,
            cancel: self.track(queued.seq),
            clock: self.clock.clone(),
        };
        let work = Work {
            call: queued.call(origin, now),
            request: queued.request,
            scratch: self.take_buffer(),
            events: self.take_buffer(),
        };
        (work, bounds)
    }

    /// Run `work` to its end. False when the connection ended first.
    async fn finish<W: Future<Output = Finished>>(&mut self, mut work: Pin<&mut W>) -> bool {
        loop {
            match self.step(Some(work.as_mut())).await {
                Flow::Finished => return true,
                Flow::End => return false,
                Flow::Continue | Flow::Up => {}
            }
        }
    }

    /// Nothing to answer: wait for a request, or for the backend's own work.
    async fn idle<B: Backend>(&mut self, backend: &mut B) -> Idle {
        if self.stage != Stage::Run {
            // shutting down and nothing left to answer
            self.disconnect().await;
            return Idle::End;
        }
        let wake = backend.next_wake(self.clock.now());
        let clock = self.clock.clone();
        let mut woke = pin!(async {
            let timer = async {
                match wake {
                    Some(at) => clock.wait_until(at).await,
                    None => pending().await,
                }
            };
            select(pin!(backend.wait()), pin!(timer)).await;
            Finished::Woke
        });
        loop {
            match self.step(Some(woke.as_mut())).await {
                Flow::Finished => return Idle::Woke,
                Flow::End => return Idle::End,
                Flow::Continue | Flow::Up => {
                    if !self.pending.is_empty() || self.stage != Stage::Run {
                        return Idle::Request;
                    }
                }
            }
        }
    }
}
