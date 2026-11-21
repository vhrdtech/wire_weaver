//! One connection: link setup, a backend of its own, requests in order, until the link goes down.

use std::collections::VecDeque;
use std::future::{Future, Pending, pending, poll_fn};
use std::ops::Deref;
use std::pin::{Pin, pin};
use std::task::Poll;

use futures_util::future::{Either, select};
use futures_util::stream::{FuturesUnordered, StreamExt};
use wire_weaver::shrink_wrap::Error as ShrinkWrapError;
use wire_weaver::{
    CallContext, ConnectionId, DisconnectReason, EventWriter, Peer, WireWeaverSharedApiBackend,
};
use ww_device::link::{Phase, Received, Transmit};
use ww_device::{
    Clock, DeviceLink, LinkConfig, LinkEvent, MessageRx, MessageTx, PeerMeta, PeerVersion,
    SendError,
};

use crate::backend::Backend;
use crate::lifecycle::{Lifecycle, Running, Stage};
use crate::outbox::{Outbox, OutboxSink};
use crate::pump::{PumpSet, Pumps};
use crate::server::{Limits, MakeBackend};

/// A connection whose link is up, as its backend's factory sees it.
#[derive(Clone, Debug)]
pub struct Connection {
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
    pub pumps: Pumps,
}

impl Connection {
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
    /// The backend has something to do on its own.
    Woke,
    /// The backend did it; the event buffer comes back.
    Polled(Vec<u8>),
}

enum Step {
    Done(Finished),
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

struct Conn<'a, Tx, Rx, C> {
    link: DeviceLink<'a>,
    tx: Tx,
    rx: Rx,
    clock: C,
    /// Link messages are built here.
    link_scratch: Vec<u8>,
    /// Requests read and not started yet, oldest first.
    pending: VecDeque<Vec<u8>>,
    outbox: &'a Outbox,
    pumps: &'a PumpSet,
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
const IDLE: Option<Pin<&mut Pending<Finished>>> = None;

impl<Tx: MessageTx, Rx: MessageRx, C: Clock> Conn<'_, Tx, Rx, C> {
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
                    Finished::Answer(done) => self.answer(done).await,
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
        let mut loopback = None;
        if let Some((kind, message)) = self.rx.message() {
            match self.link.handle_message(now, kind, message) {
                Some(Received::Data(request)) => self.pending.push_back(request.to_vec()),
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

    /// Answer a request with `Overloaded`, its handler never ran.
    async fn refuse(&mut self, request: Vec<u8>) {
        let mut scratch = self.take_buffer();
        let base = scratch.as_ptr().addr();
        // a request without a seq expects no answer
        let reply = ww_device::overloaded_reply(&request, &mut scratch)
            .map_or((0, 0), |reply| (reply.as_ptr().addr() - base, reply.len()));
        let events = self.take_buffer();
        self.answer(Done {
            request,
            scratch,
            events,
            reply: Some(reply),
        })
        .await;
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
    /// first: closing under a client that is writing would show it a transport error, not the reason.
    async fn disconnect(&mut self) {
        self.link.disconnect(DisconnectReason::Shutdown);
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
}

/// Run one request alone, through the backend's `&mut self` entry.
async fn exclusive<B: Backend>(
    backend: &mut B,
    origin: &CallContext,
    medium: B::Medium,
    outbox: &Outbox,
    mut work: Work,
) -> Finished {
    let (base, size) = (work.scratch.as_ptr().addr(), work.scratch.len());
    let mut out = EventWriter::new(OutboxSink(outbox), &mut work.events);
    let reply = backend
        .process_exclusive_bytes(origin, &mut out, medium, &work.request, &mut work.scratch)
        .await;
    let reply = reply_range(base, size, reply);
    work.done(reply)
}

/// Run one shared request through the backend's `&self` entry, beside others. `_running` counts it as running
/// until it is done.
async fn shared<B: Backend>(
    backend: &B,
    origin: &CallContext,
    medium: B::Medium,
    outbox: &Outbox,
    mut work: Work,
    _running: Running<'_>,
) -> Finished {
    let (base, size) = (work.scratch.as_ptr().addr(), work.scratch.len());
    let mut out = EventWriter::new(OutboxSink(outbox), &mut work.events);
    let reply = backend
        .process_shared_bytes(origin, &mut out, medium, &work.request, &mut work.scratch)
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
#[expect(clippy::too_many_arguments, reason = "one call site, in Server::serve")]
pub(crate) async fn run<M: MakeBackend>(
    config: LinkConfig<'_>,
    tx: impl MessageTx,
    mut rx: impl MessageRx,
    clock: impl Clock + Clone,
    id: ConnectionId,
    peer: Peer,
    make: impl Deref<Target = M>,
    limits: Limits,
    life: impl Deref<Target = Lifecycle>,
) {
    let (make, life) = (&*make, &*life);
    rx.wait_connected().await;
    let outbox = Outbox::new(limits.events_max);
    // the medium's limit: replies and events are built in buffers of this size, longer replies of a server
    // with an allocator in buffers of their own
    let buffer_len = rx.max_message_len();
    let pumps = PumpSet::default();
    let mut conn = Conn {
        link: DeviceLink::new(config, rx.max_message_len()),
        tx,
        rx,
        clock,
        link_scratch: vec![0; buffer_len],
        pending: VecDeque::new(),
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
    let info = Connection {
        id,
        peer,
        client: conn.link.peer_meta().cloned().unwrap_or_default(),
        client_version: conn.link.peer_version().cloned(),
        message_len_max: conn.link.agreed_max_message_len(),
        pumps: pumps.handle(),
    };
    let origin = info.origin();
    let mut backend = make.make(&info);
    let medium = make.medium(&info);

    loop {
        if conn
            .pending
            .front()
            .is_some_and(|request| M::Backend::is_shared(request))
        {
            if !conn.run_shared(&backend, medium, &origin).await {
                return;
            }
        } else if let Some(request) = conn.pending.pop_front() {
            let Some(_running) = life.start_request(limits.requests_max) else {
                // nothing ran: no turn for the backend either
                conn.refuse(request).await;
                continue;
            };
            let work = conn.work(request);
            let work = pin!(exclusive(&mut backend, &origin, medium, &outbox, work));
            if !conn.finish(work).await {
                return;
            }
        } else {
            match conn.idle(&mut backend).await {
                Idle::Woke => {}
                // the request first: the backend's turn comes after it
                Idle::Request => continue,
                Idle::End => return,
            }
        }
        // the backend's own turn, to its end
        let (events, now) = (conn.take_buffer(), conn.clock.now());
        let work = pin!(poll(&mut backend, medium, &outbox, now, events));
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

impl<Tx: MessageTx, Rx: MessageRx, C: Clock + Clone> Conn<'_, Tx, Rx, C> {
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
    ) -> bool {
        let (outbox, life) = (self.outbox, self.life);
        let mut running = FuturesUnordered::new();
        loop {
            while running.len() < self.limits.calls_max
                && self
                    .pending
                    .front()
                    .is_some_and(|request| B::is_shared(request))
            {
                let request = self.pending.pop_front().expect("checked above");
                let Some(counted) = life.start_request(self.limits.requests_max) else {
                    self.refuse(request).await;
                    continue;
                };
                let work = self.work(request);
                running.push(shared(backend, origin, medium, outbox, work, counted));
            }
            if running.is_empty() {
                return true;
            }
            // TODO(SRV-47): a request's own deadline drops it here and answers DeadlineExceeded; until then a
            // request that runs alone waits for every shared one before it, however long they take
            let next = pin!(next_finished(&mut running));
            if self.step(Some(next)).await == Flow::End {
                return false;
            }
        }
    }

    /// A request with the buffers it runs with.
    fn work(&mut self, request: Vec<u8>) -> Work {
        Work {
            request,
            scratch: self.take_buffer(),
            events: self.take_buffer(),
        }
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
