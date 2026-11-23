//! A front connection's side of its mount slots (SRV-78): a link of its own to each mounted backend it uses, the
//! requests under a slot sent on, the backend's events sent back with the slot's path in front.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::future::{pending, poll_fn};

use futures_util::future::{select, select_all};
use futures_util::pin_mut;
use wire_weaver::shrink_wrap::{
    DeserializeShrinkWrap, SerializeShrinkWrap, UVlq32, UVlq32Backfill,
};
use wire_weaver::ww_version::WireVersion;
use wire_weaver::{EventOut, EventWriter, MessageSink, Priority};
use ww_client_server::mount::{MountAnswer, MountPrefix, MountRequest, request_at};
use ww_client_server::{
    Error, ErrorKind, Event, EventKind, PathKind, Request, RequestKind, ResourceIndex,
    StreamSideband, err_seq,
};
use ww_device::{Clock, Instant};

use super::connect::Connect;
use super::link::{BackendLink, LinkEvent, LinkParams};
use super::table::{MountEvent, Mounts, ids};
use crate::Connection;
use crate::fanout::Subscription;

/// Requests of one front connection that may wait for a backend's link to come up; more are refused `Overloaded`.
const WAITING_MAX: usize = 64;
/// Requests of one front connection sent on to one backend and not answered yet; more are refused `Overloaded`.
const IN_FLIGHT_MAX: usize = 256;
/// Streams opened and trait levels observed under one slot that a front connection tracks, to close them when the
/// backend goes.
const CLOSABLE_MAX: usize = 256;

/// What a front connection does with its mount slots: hold it in the connection's backend, send each request of a
/// slot's handler on with [forward](Self::forward), and give it the backend's turns ([wait](Self::wait),
/// [next_wake](Self::next_wake), [poll](Self::poll), [cancel](Self::cancel), [end](Self::end)).
///
/// ```ignore
/// fn hub_request(&mut self, _cx: &mut Context<'_, impl EventOut>, request: MountRequest<'_, '_>) -> MountAnswer {
///     self.mounts.forward(&request)
/// }
///
/// impl Backend for Tv {
///     async fn wait(&mut self) { self.mounts.wait().await }
///     fn next_wake(&mut self, _now: Instant) -> Option<Instant> { self.mounts.next_wake() }
///     async fn poll(&mut self, out: &mut EventWriter<'_, impl MessageSink>, _medium: (), _now: Instant) {
///         for event in self.mounts.poll(out).await {
///             if let MountEvent::Presence { .. } = event { self.mark_changed_hub_presence(); }
///         }
///     }
///     fn cancel(&mut self, seq: u32) { self.mounts.cancel(seq) }
///     async fn end(&mut self) { self.mounts.end().await }
/// }
/// ```
///
/// Each slot gets a link of its own, opened by the first request under it and kept while the backend is up: seqs
/// pass through unchanged. While it comes up, requests wait (at most 64). When the backend goes, every
/// request it didn't answer is answered `Unavailable`, and every stream opened and every trait level observed under
/// the slot gets a `Close` sideband at its path, which ends the client's stream or observers.
pub struct Forwarder<C: Connect, K: Clock> {
    mounts: Mounts<C>,
    clock: K,
    events: Subscription<MountEvent>,
    /// Table changes received in [wait](Self::wait), for the next [poll](Self::poll).
    changes: Vec<MountEvent>,
    /// The table changed more than the queue held: every link is checked.
    lagged: bool,
    /// The front connection's limit and model.
    message_len_max: usize,
    model_wire: Option<WireVersion>,
    links: BTreeMap<Vec<u32>, FrontLink<C>>,
    /// Errors this side answers with itself, sent in the next [poll](Self::poll).
    answers: VecDeque<(u32, Priority, u32, ErrorKind<'static>)>,
    /// Requests and events are rewritten here.
    buf: Vec<u8>,
}

/// One front connection's link to one slot's backend.
struct FrontLink<C: Connect> {
    link: BackendLink<C>,
    /// The mount this link belongs to: another one at the same slot makes it stale.
    epoch: u64,
    prefix: MountPrefix,
    /// The backend reads request contexts and cancels.
    reads_context: bool,
    /// The longest request the backend takes.
    message_len_max: usize,
    /// Requests sent on and not answered yet, with their priority.
    in_flight: BTreeMap<u32, Priority>,
    /// Paths (the front's) of the streams open and the trait levels observed under the slot: each is sent a
    /// `Close` sideband when the backend goes.
    closable: BTreeSet<Vec<u32>>,
}

impl<C: Connect, K: Clock> Forwarder<C, K> {
    /// The forwarder of the front connection `connection`: links to the backends of `mounts` take its message limit
    /// and the client's model, so that what they answer fits it and reads for it.
    pub fn new<P>(mounts: &Mounts<C>, connection: &Connection<P>, clock: K) -> Self {
        Forwarder {
            events: mounts.subscribe(),
            mounts: mounts.clone(),
            clock,
            changes: Vec::new(),
            lagged: false,
            message_len_max: connection.message_len_max,
            model_wire: connection
                .client_version
                .as_ref()
                .and_then(|version| version.model_wire),
            links: BTreeMap::new(),
            answers: VecDeque::new(),
            buf: vec![0; connection.message_len_max],
        }
    }

    /// Send `request` on to the backend mounted at its slot; for a slot's `<slot>_request` handler. `Unavailable`
    /// when nothing is up there. Errors found on the way (too long, too many waiting) are answered in the next
    /// [poll](Self::poll).
    pub fn forward(&mut self, request: &MountRequest<'_, '_>) -> MountAnswer {
        let Some(prefix) = request.prefix() else {
            return MountAnswer::Unavailable;
        };
        let key = ids(&prefix);
        let Some((connector, options, epoch, facts)) = self.mounts.backend(&prefix) else {
            self.drop_link(&key);
            return MountAnswer::Unavailable;
        };
        let now = self.clock.now();
        if self
            .links
            .get(&key)
            .is_some_and(|front| front.epoch != epoch || front.link.is_down())
        {
            self.drop_link(&key);
        }
        let front = self.links.entry(key).or_insert_with(|| FrontLink {
            link: BackendLink::open(
                &*connector,
                LinkParams {
                    target: options.target,
                    message_len_max: self.message_len_max,
                    model_wire: self.model_wire,
                },
                now,
            ),
            epoch,
            prefix: MountPrefix::new(prefix),
            reads_context: facts.request_context,
            message_len_max: usize::try_from(facts.dev_max_message_len).unwrap_or(usize::MAX),
            in_flight: BTreeMap::new(),
            closable: BTreeSet::new(),
        });
        let seq = request.request.seq.0;
        let priority = Priority::from(request.request.priority);
        let refuse = |answers: &mut VecDeque<_>, site, kind| {
            if seq != 0 {
                answers.push_back((seq, priority, site, kind));
            }
            MountAnswer::Forwarded
        };
        if front.link.waiting() >= WAITING_MAX || front.in_flight.len() >= IN_FLIGHT_MAX {
            return refuse(
                &mut self.answers,
                err_seq::MOUNT_QUEUE_FULL,
                ErrorKind::Overloaded,
            );
        }
        let mut sent = request.request.clone();
        if !front.reads_context {
            // an older reader takes the context's flag for a reserved bit
            sent.context = None;
        }
        let bytes = match request_at(&sent, request.path, &mut self.buf) {
            Ok(bytes) if bytes.len() <= front.message_len_max => bytes,
            _ => {
                return refuse(
                    &mut self.answers,
                    err_seq::MOUNT_TOO_LONG,
                    ErrorKind::ResponseSerFailed,
                );
            }
        };
        if !front.link.send(bytes, now) {
            return MountAnswer::Unavailable;
        }
        if seq != 0 {
            front.in_flight.insert(seq, priority);
        }
        let path = front.prefix.join(request.path);
        match &request.request.kind {
            RequestKind::StreamOpen { .. }
            | RequestKind::StreamSideband {
                sideband: StreamSideband::Open,
            } if front.closable.len() < CLOSABLE_MAX => {
                front.closable.insert(ids(&path));
            }
            RequestKind::StreamSideband {
                sideband: StreamSideband::Close,
            } => {
                front.closable.remove(&ids(&path));
            }
            RequestKind::Observe { cfg } => {
                // `entries: [], patch: false` stops observing the level; one that doesn't decode may start it
                let stops = cfg
                    .read()
                    .is_ok_and(|cfg| cfg.entries.is_empty() && !cfg.patch);
                if stops {
                    front.closable.remove(&ids(&path));
                } else if front.closable.len() < CLOSABLE_MAX {
                    front.closable.insert(ids(&path));
                }
            }
            _ => {}
        }
        MountAnswer::Forwarded
    }

    /// The client cancelled its request `seq`, which the connection doesn't hold ([Backend::cancel](
    /// crate::Backend::cancel)): sent on to the backend that has it, when it reads cancels.
    pub fn cancel(&mut self, seq: u32) {
        let now = self.clock.now();
        let Some(front) = self
            .links
            .values_mut()
            .find(|front| front.in_flight.contains_key(&seq))
        else {
            return;
        };
        if !front.reads_context {
            return;
        }
        let cancel = Request {
            seq: UVlq32Backfill(0),
            priority: Priority::default().to_wire(),
            context: None,
            path_kind: PathKind::absolute(&[]),
            kind: RequestKind::Cancel { seq: UVlq32(seq) },
        };
        if let Ok(bytes) = cancel.to_ww_bytes(&mut self.buf) {
            front.link.send(bytes, now);
        }
    }

    /// Completes when there is something to send: a backend's event or link change, a change of the mount table,
    /// an answer of this side's own. Cancel-safe.
    pub async fn wait(&mut self) {
        if !self.answers.is_empty() || !self.changes.is_empty() || self.lagged {
            return;
        }
        let ready: Vec<_> = self
            .links
            .values_mut()
            .map(|front| Box::pin(front.link.ready()))
            .collect();
        let links = async move {
            if ready.is_empty() {
                pending::<()>().await;
            } else {
                select_all(ready).await;
            }
        };
        let (events, changes, lagged) = (&mut self.events, &mut self.changes, &mut self.lagged);
        let table = poll_fn(|cx| match events.poll_recv(cx) {
            std::task::Poll::Ready(Some(Ok(event))) => {
                changes.push(event);
                std::task::Poll::Ready(())
            }
            std::task::Poll::Ready(Some(Err(_))) => {
                *lagged = true;
                std::task::Poll::Ready(())
            }
            // the table is gone: nothing more changes
            std::task::Poll::Ready(None) | std::task::Poll::Pending => std::task::Poll::Pending,
        });
        pin_mut!(links, table);
        select(links, table).await;
    }

    /// When [poll](Self::poll) wants to run without [wait](Self::wait) completing: a link's ping or timeout.
    pub fn next_wake(&self) -> Option<Instant> {
        self.links
            .values()
            .filter_map(|front| front.link.next_wake())
            .min()
    }

    /// Send what came from the backends to the client, and this side's own answers; returns what changed in the
    /// mount table since the last call, for the slots' `presence` (`mark_changed_<slot>_presence`) and the keys of
    /// `mounts!` (`<slots>_keys_send`).
    pub async fn poll(&mut self, out: &mut EventWriter<'_, impl MessageSink>) -> Vec<MountEvent> {
        let now = self.clock.now();
        let changes = std::mem::take(&mut self.changes);
        let lagged = std::mem::take(&mut self.lagged);
        // a link whose mount is no longer up is gone with its backend
        let stale: Vec<Vec<u32>> = self
            .links
            .iter()
            .filter(|(_, front)| {
                (lagged || changes.iter().any(|change| touches(change, front)))
                    && self.mounts.epoch_up(front.prefix.path()) != Some(front.epoch)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in stale {
            if let Some(front) = self.links.remove(&key) {
                self.gone(front, out).await;
            }
        }
        let keys: Vec<Vec<u32>> = self.links.keys().cloned().collect();
        for key in keys {
            let front = self.links.get_mut(&key).expect("keys of the map");
            let down = pass_on(front, now, out, &mut self.buf, &mut self.answers).await;
            if down {
                let front = self.links.remove(&key).expect("keys of the map");
                self.gone(front, out).await;
            } else {
                front.link.flush().await;
            }
        }
        while let Some((seq, priority, site, kind)) = self.answers.pop_front() {
            let error = Error::new(site, kind.for_client(self.model_wire));
            let _ = out
                .send_reply_with(priority, |scratch| {
                    ww_client_server::util::ser_err_event(scratch, seq, error).map(<[u8]>::len)
                })
                .await;
        }
        changes
    }

    /// The connection is over: each backend is told, and its link dropped.
    pub async fn end(&mut self) {
        for front in self.links.values_mut() {
            front.link.close().await;
        }
        self.links.clear();
    }

    fn drop_link(&mut self, key: &[u32]) {
        if let Some(front) = self.links.remove(key) {
            // answered in the next poll: the requests it had are gone with it
            for (seq, priority) in front.in_flight {
                self.answers.push_back((
                    seq,
                    priority,
                    err_seq::MOUNT_BACKEND_GONE,
                    ErrorKind::Unavailable,
                ));
            }
        }
    }

    /// `front`'s backend is gone: what it didn't answer is answered `Unavailable`, its streams and observed levels
    /// are closed.
    async fn gone(&mut self, front: FrontLink<C>, out: &mut EventWriter<'_, impl MessageSink>) {
        for (seq, priority) in front.in_flight {
            self.answers.push_back((
                seq,
                priority,
                err_seq::MOUNT_BACKEND_GONE,
                ErrorKind::Unavailable,
            ));
        }
        for path in front.closable {
            let path: Vec<ResourceIndex> = path
                .iter()
                .map(|id| ResourceIndex::new(wire_weaver::shrink_wrap::UNib32(*id)))
                .collect();
            let _ = out
                .send_with(|scratch| {
                    ww_client_server::util::ser_stream_sideband_event(
                        scratch,
                        0,
                        &path,
                        StreamSideband::Close,
                    )
                    .map(<[u8]>::len)
                })
                .await;
        }
    }
}

/// Whether `change` is about the slot of `front`.
fn touches<C: Connect>(change: &MountEvent, front: &FrontLink<C>) -> bool {
    let (MountEvent::Mounted { api_path }
    | MountEvent::Presence { api_path }
    | MountEvent::Unmounted { api_path }) = change;
    api_path.as_slice() == front.prefix.path()
}

/// Send what `front`'s backend sent on to the client, rewritten with the slot's prefix. True when its link is down.
async fn pass_on<C: Connect>(
    front: &mut FrontLink<C>,
    now: Instant,
    out: &mut EventWriter<'_, impl MessageSink>,
    buf: &mut [u8],
    answers: &mut VecDeque<(u32, Priority, u32, ErrorKind<'static>)>,
) -> bool {
    while let Some(event) = front.link.next_event(now) {
        let bytes = match event {
            LinkEvent::Up => continue,
            LinkEvent::Down(_) => return true,
            LinkEvent::Data(bytes) => bytes,
        };
        let Ok(Event { seq, result }) = Event::from_ww_bytes(bytes) else {
            // not an event of this build: dropped, as a client drops it
            continue;
        };
        let seq = seq.0;
        // stream data under a request's seq (introspection) comes before its answer
        let last = !matches!(
            result,
            Ok(EventKind::StreamData { .. }
                | EventKind::StreamItem { .. }
                | EventKind::PropertyUpdate { .. })
        );
        if seq == 0
            && let Ok(EventKind::StreamSideband {
                path,
                sideband: StreamSideband::Close,
            }) = &result
        {
            let below: Vec<u32> = path
                .iter()
                .filter_map(Result::ok)
                .map(|i| i.get().0)
                .collect();
            let mut full = ids(front.prefix.path());
            full.extend(below);
            front.closable.remove(&full);
        }
        // a backend answers only what it was sent: a reply under another seq could pose as the answer to a request
        // of the same client to the front itself or to another slot
        let priority = match seq {
            0 => None,
            seq if last => match front.in_flight.remove(&seq) {
                Some(priority) => Some(priority),
                None => continue,
            },
            seq => match front.in_flight.get(&seq) {
                Some(priority) => Some(*priority),
                None => continue,
            },
        };
        let Ok(rewritten) = front.prefix.event_out(bytes, buf) else {
            if seq != 0 && last {
                answers.push_back((
                    seq,
                    priority.unwrap_or_default(),
                    err_seq::MOUNT_TOO_LONG,
                    ErrorKind::ResponseSerFailed,
                ));
            }
            continue;
        };
        let sent = match priority {
            Some(priority) => out.send_reply_bytes(priority, rewritten).await,
            None => {
                out.send_with(|scratch| {
                    let to = scratch
                        .get_mut(..rewritten.len())
                        .ok_or(wire_weaver::shrink_wrap::Error::OutOfBoundsWriteRawSlice)?;
                    to.copy_from_slice(rewritten);
                    Ok(rewritten.len())
                })
                .await
            }
        };
        if sent.is_err() && seq != 0 && last {
            answers.push_back((
                seq,
                priority.unwrap_or_default(),
                err_seq::MOUNT_TOO_LONG,
                ErrorKind::ResponseSerFailed,
            ));
        }
    }
    false
}
