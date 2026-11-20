//! One connection: link setup, a backend of its own, requests in order, until the link goes down.

use std::collections::VecDeque;
use std::future::{Future, Pending, pending, poll_fn};
use std::pin::{Pin, pin};
use std::task::Poll;

use futures_util::future::select;
use wire_weaver::{ConnectionId, DisconnectReason, EventWriter, Peer};
use ww_device::link::{Phase, Received, Transmit};
use ww_device::{
    Clock, DeviceLink, LinkConfig, LinkEvent, MessageRx, MessageTx, PeerMeta, PeerVersion,
    SendError,
};

use crate::backend::Backend;
use crate::lifecycle::{Lifecycle, Stage};
use crate::outbox::{Outbox, OutboxSink};
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
    /// Replies are built here; `None` while a request has it.
    reply_buffer: Option<Vec<u8>>,
    /// Events are built here; `None` while a request or the backend's own work has it.
    event_buffer: Option<Vec<u8>>,
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
        let (outbox, life, known) = (self.outbox, self.life, self.stage);
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
            if !outbox.is_empty() {
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
                        self.event_buffer = Some(events);
                    }
                }
                flow = Flow::Finished;
            }
            Step::Events => self.send_events().await,
            Step::Message(false) => self.link.on_transport_down(),
            Step::Message(true) => {
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
            if self.send(&event.bytes).await.is_ok() && event.urgent {
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
        self.reply_buffer = Some(done.scratch);
        self.event_buffer = Some(done.events);
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

    /// Tell the client that the server goes away, and send it now.
    async fn disconnect(&mut self) {
        // TODO(SRV-12): a `Shutdown` reason of its own, appended with the `Overloaded` error kind
        self.link.disconnect(DisconnectReason::RequestByUser);
        self.drain_transmit().await;
        let _ = self.tx.flush().await;
    }
}

/// Run one request through the backend.
async fn call<B: Backend>(
    backend: &mut B,
    medium: B::Medium,
    outbox: &Outbox,
    request: Vec<u8>,
    mut scratch: Vec<u8>,
    mut events: Vec<u8>,
) -> Finished {
    let base = scratch.as_ptr().addr();
    let size = scratch.len();
    let mut out = EventWriter::new(OutboxSink(outbox), &mut events);
    let reply = match backend
        .process_bytes(&mut out, medium, &request, &mut scratch)
        .await
    {
        Ok([]) => Some((0, 0)),
        Ok(reply) => {
            // the reply is a part of `scratch`: kept as a range, so the buffer can move with it
            let start = reply.as_ptr().addr().wrapping_sub(base);
            let len = reply.len();
            assert!(
                start.checked_add(len).is_some_and(|end| end <= size),
                "the dispatcher's reply is not inside its scratch buffer"
            );
            Some((start, len))
        }
        Err(_) => None,
    };
    Finished::Answer(Done {
        request,
        scratch,
        events,
        reply,
    })
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
    make: &M,
    limits: Limits,
    life: &Lifecycle,
) {
    rx.wait_connected().await;
    let outbox = Outbox::new(limits.events_max);
    // the medium's limit: replies and events are built in buffers of this size, longer replies of a server
    // with an allocator in buffers of their own
    let buffer_len = rx.max_message_len();
    let mut conn = Conn {
        link: DeviceLink::new(config, rx.max_message_len()),
        tx,
        rx,
        clock,
        link_scratch: vec![0; buffer_len],
        pending: VecDeque::new(),
        outbox: &outbox,
        reply_buffer: Some(vec![0; buffer_len]),
        event_buffer: Some(vec![0; buffer_len]),
        limits,
        life,
        stage: Stage::Run,
    };
    conn.rx.reset();
    conn.tx.reset();
    conn.link.on_transport_up(conn.clock.now());
    conn.drain_transmit().await;

    // link setup; a server that shuts down meanwhile drops the connection
    loop {
        match conn.step(IDLE).await {
            Flow::Up => break,
            Flow::End => return,
            Flow::Continue | Flow::Finished => {
                if conn.stage != Stage::Run {
                    return;
                }
            }
        }
    }
    outbox.set_message_len_max(conn.link.agreed_max_message_len());
    let info = Connection {
        id,
        peer,
        client: conn.link.peer_meta().cloned().unwrap_or_default(),
        client_version: conn.link.peer_version().cloned(),
        message_len_max: conn.link.agreed_max_message_len(),
    };
    let mut backend = make.make(&info);
    let medium = make.medium(&info);

    loop {
        if let Some(request) = conn.pending.pop_front() {
            let scratch = conn
                .reply_buffer
                .take()
                .expect("the buffer comes back with every answer");
            let events = conn
                .event_buffer
                .take()
                .expect("the buffer comes back with every answer");
            let mut work = pin!(call(
                &mut backend,
                medium,
                &outbox,
                request,
                scratch,
                events
            ));
            loop {
                match conn.step(Some(work.as_mut())).await {
                    Flow::Finished => break,
                    Flow::End => return,
                    Flow::Continue | Flow::Up => {}
                }
            }
        } else {
            if conn.stage != Stage::Run {
                // shutting down and nothing left to answer
                conn.disconnect().await;
                return;
            }
            // nothing to answer: until a request comes, or the backend has work of its own
            let wake = backend.next_wake(conn.clock.now());
            let clock = conn.clock.clone();
            let mut idle = pin!(async {
                let timer = async {
                    match wake {
                        Some(at) => clock.wait_until(at).await,
                        None => pending().await,
                    }
                };
                select(pin!(backend.wait()), pin!(timer)).await;
                Finished::Woke
            });
            let woke = loop {
                match conn.step(Some(idle.as_mut())).await {
                    Flow::Finished => break true,
                    Flow::End => return,
                    Flow::Continue | Flow::Up => {
                        if !conn.pending.is_empty() || conn.stage != Stage::Run {
                            break false;
                        }
                    }
                }
            };
            if !woke {
                // the request first: the backend's turn comes after it
                continue;
            }
        }
        // the backend's own turn, to its end
        let events = conn
            .event_buffer
            .take()
            .expect("the buffer comes back with every answer");
        let now = conn.clock.now();
        let mut work = pin!(poll(&mut backend, medium, &outbox, now, events));
        loop {
            match conn.step(Some(work.as_mut())).await {
                Flow::Finished => break,
                Flow::End => return,
                Flow::Continue | Flow::Up => {}
            }
        }
    }
}
