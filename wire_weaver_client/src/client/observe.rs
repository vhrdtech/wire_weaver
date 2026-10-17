//! Observing properties on the host (CLT-28): the client side of `RequestKind::Observe` and
//! `EventKind::PropertyUpdate` (API-38, device side SRV-18).
//!
//! A trait level's `+observe` properties are observed as one unit: the generated `observe(cfg)` of a level client
//! returns an [Observer] of the level's `<Trait>Update` (one `Option` per property, the ones sent present), and
//! `observe_<name>(mode)` an [Observer] of one property's values, fanned out from the same level. Every observer
//! of a level asks for its own settings, the host sends the device one subscription per level that covers them all
//! ([ObserveConfig::merge]), and sends it again when an observer comes, changes its settings ([Observer::configure])
//! or goes; when the last one goes the level is turned `Off`.
//!
//! The level keeps the latest value of each property with its receive time ([Observer::with_latest]).
//! Observers live on through `disconnect().keep_streams()`: [Commander::resume_streams](crate::Commander::resume_streams)
//! subscribes again on the next connection. A device whose API model predates observing (`ww_client_server`
//! before 0.2) gives [Error::Unsupported].

use crate::Error;
use crate::StreamError;
use crate::client::batch::unwrap_t;
use crate::event_loop::command::Command;
use crate::event_loop::commander::TransportCommander;
use crate::event_loop::rx_dispatcher::ResponseReceiver;
use std::any::Any;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::pin::pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::{debug, warn};
use wire_weaver::shrink_wrap::{
    DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned, UNib32, UVlq32Backfill,
};
use ww_client_server::observe::{ObserveEntry, ObserveGrantedOwned, ObserveRequestOwned};
pub use ww_client_server::observe::{ObserveMode, ObserveRefusal, ObserveRefused};
use ww_client_server::{PathKindOwned, RequestKindOwned, RequestOwned};
pub use ww_si::Second;

/// `ObserveMode::OnChange`: sent when the value changes, at most once every `min`.
pub fn on_change(min: Duration) -> ObserveMode {
    ObserveMode::OnChange {
        min_us: micros(min),
    }
}

/// `ObserveMode::Periodic`: sent every `every`, changed or not.
pub fn periodic(every: Duration) -> ObserveMode {
    ObserveMode::Periodic {
        every_us: micros(every),
    }
}

/// `ObserveMode::OnChangeOrBeat`: sent when the value changes, at most once every `min`, and at least once every
/// `max`.
pub fn on_change_or_beat(min: Duration, max: Duration) -> ObserveMode {
    ObserveMode::OnChangeOrBeat {
        min_us: micros(min),
        max_us: micros(max),
    }
}

fn micros(d: Duration) -> UNib32 {
    UNib32(d.as_micros().min(u32::MAX as u128) as u32)
}

/// Data of a level's `PropertyUpdate`: the generated `<Trait>Update`, with its latest-value cache and its
/// settings type.
pub trait LevelUpdate: DeserializeShrinkWrapOwned + Debug + Send + 'static {
    /// `<Trait>Latest`: the last value of each property, with when it arrived.
    type Latest: Default + Debug + Send + 'static;
    /// `<Trait>Observe`: how each property is observed.
    type Config: LevelConfig;

    /// Move the properties present in this update into `latest`.
    fn update_latest(self, latest: &mut Self::Latest, received: Instant, t: Option<Second>);
}

/// Settings of a level: the generated `<Trait>Observe`, one `Option<ObserveMode>` per property.
pub trait LevelConfig: Default + Clone + Debug + PartialEq + Send + Sync + 'static {
    /// Modes by resource index
    fn to_entries(&self) -> BTreeMap<u32, ObserveMode>;
    fn from_entries(entries: &BTreeMap<u32, ObserveMode>) -> Self;
}

/// Settings of a level by resource index, what [LevelConfig] converts to and from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObserveConfig {
    /// Mode per property, by resource index; absent and `Off` entries are not observed.
    pub entries: BTreeMap<u32, ObserveMode>,
    /// Bytes per second the level's updates may take, `None` leaves it to the device.
    pub budget: Option<u32>,
}

impl ObserveConfig {
    /// One setting that gives every observer at least what it asked for: per property, the shortest on-change
    /// interval and the shortest period of all, so `OnChange` and `Periodic` give `OnChangeOrBeat`; the biggest
    /// budget.
    pub fn merge<'a>(configs: impl IntoIterator<Item = &'a ObserveConfig>) -> ObserveConfig {
        let mut merged = ObserveConfig::default();
        for cfg in configs {
            for (&index, &mode) in &cfg.entries {
                let m = merged.entries.entry(index).or_insert(ObserveMode::Off);
                *m = merge_modes(*m, mode);
            }
            merged.budget = match (merged.budget, cfg.budget) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
        }
        merged.entries.retain(|_, m| !matches!(m, ObserveMode::Off));
        merged
    }
}

/// On-change interval and period of a mode
fn mode_parts(mode: ObserveMode) -> (Option<u32>, Option<u32>) {
    match mode {
        ObserveMode::OnChange { min_us } => (Some(min_us.0), None),
        ObserveMode::Periodic { every_us } => (None, Some(every_us.0)),
        ObserveMode::OnChangeOrBeat { min_us, max_us } => (Some(min_us.0), Some(max_us.0)),
        ObserveMode::Off | ObserveMode::Unknown(_) => (None, None),
    }
}

fn merge_modes(a: ObserveMode, b: ObserveMode) -> ObserveMode {
    if matches!(a, ObserveMode::Off) {
        return b;
    }
    if matches!(b, ObserveMode::Off | ObserveMode::Unknown(_)) {
        return a;
    }
    if matches!(a, ObserveMode::Unknown(_)) {
        return b;
    }
    let min = |x: Option<u32>, y: Option<u32>| match (x, y) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, y) => x.or(y),
    };
    let (ca, pa) = mode_parts(a);
    let (cb, pb) = mode_parts(b);
    match (min(ca, cb), min(pa, pb)) {
        (Some(c), None) => ObserveMode::OnChange { min_us: UNib32(c) },
        (None, Some(p)) => ObserveMode::Periodic {
            every_us: UNib32(p),
        },
        // sent at least every p anyway
        (Some(c), Some(p)) if c >= p => ObserveMode::Periodic {
            every_us: UNib32(p),
        },
        (Some(c), Some(p)) => ObserveMode::OnChangeOrBeat {
            min_us: UNib32(c),
            max_us: UNib32(p),
        },
        (None, None) => ObserveMode::Off,
    }
}

/// What the device applies to a level now: the settings of all its observers together, clamped by the device.
#[derive(Clone, Debug, PartialEq)]
pub struct Granted<C> {
    /// Every property observed now with its mode.
    pub modes: C,
    /// Bytes per second the level's updates may take.
    pub budget: u32,
    /// Entries of the last request the device did not apply, and why.
    pub refused: Vec<ObserveRefused>,
}

/// One update as an observer gets it.
#[derive(Clone, Debug, PartialEq)]
pub struct Observed<T> {
    /// The level's `<Trait>Update`, or one property's value.
    pub value: T,
    /// When the device took the values, on its clock, if it sends timestamps. Microseconds the device sends as a
    /// wrapping `u32` come unwrapped, as a `u64` counting on across wraps.
    pub t: Option<Second>,
    /// Sends of this level that were due but did not fit the budget since the previous update.
    pub missed: u32,
    /// When the update arrived.
    pub received: Instant,
}

/// The latest value of one property, in a level's `<Trait>Latest`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample<T> {
    pub value: T,
    /// When the value arrived.
    pub received: Instant,
    /// When the device took it, on its clock, if it sends timestamps.
    pub t: Option<Second>,
}

/// Anything an [Observer] can receive.
#[derive(Clone, Debug, PartialEq)]
pub enum ObserveEvent<T> {
    Update(Observed<T>),
    /// The link went down; after `disconnect().keep_streams()` and a reconnect the observer goes on with
    /// [ObserveEvent::Resumed].
    Disconnected,
    /// Subscribed again on a new connection.
    Resumed,
    /// `events` updates were dropped before this one, the observer's queue on the host was full (the program
    /// didn't read for a while).
    Dropped {
        events: u64,
    },
}

/// A `PropertyUpdate` as received, decoded by each observer
#[derive(Debug)]
struct RawUpdate {
    data: Vec<u8>,
    t: Option<Second>,
    missed: u32,
    received: Instant,
}

enum QueueItem {
    Update(Arc<RawUpdate>),
    Disconnected,
    Resumed,
    Dropped { events: u64 },
}

/// Queue of one observer, bounded in bytes of update data like a stream's (CLT-23): data that doesn't fit is
/// dropped and reported once.
struct Queue {
    inner: Mutex<QueueInner>,
    notify: tokio::sync::Notify,
    cond: Condvar,
}

struct QueueInner {
    items: VecDeque<QueueItem>,
    bytes: usize,
    limit: usize,
    /// Dropped since the last item that got in
    unreported: u64,
    dropped_total: u64,
    /// Nothing more will come
    closed: bool,
}

impl Queue {
    fn new(limit: usize) -> Arc<Queue> {
        Arc::new(Queue {
            inner: Mutex::new(QueueInner {
                items: VecDeque::new(),
                bytes: 0,
                limit,
                unreported: 0,
                dropped_total: 0,
                closed: false,
            }),
            notify: tokio::sync::Notify::new(),
            cond: Condvar::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, QueueInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn push(&self, item: QueueItem) {
        let mut inner = self.lock();
        let len = match &item {
            QueueItem::Update(u) => u.data.len(),
            _ => 0,
        };
        if len > 0 && inner.bytes + len > inner.limit {
            inner.unreported += 1;
            inner.dropped_total += 1;
            return;
        }
        if inner.unreported > 0 {
            let events = std::mem::take(&mut inner.unreported);
            warn!("observer queue full, dropped {events} updates");
            inner.items.push_back(QueueItem::Dropped { events });
        }
        inner.bytes += len;
        inner.items.push_back(item);
        drop(inner);
        self.wake();
    }

    fn close(&self) {
        self.lock().closed = true;
        self.wake();
    }

    fn wake(&self) {
        self.notify.notify_waiters();
        self.cond.notify_all();
    }

    fn pop(inner: &mut QueueInner) -> Option<QueueItem> {
        let item = inner.items.pop_front()?;
        if let QueueItem::Update(u) = &item {
            inner.bytes -= u.data.len();
        }
        Some(item)
    }

    fn try_pop(&self) -> Result<Option<QueueItem>, StreamError> {
        let mut inner = self.lock();
        match Self::pop(&mut inner) {
            Some(item) => Ok(Some(item)),
            None if inner.closed => Err(StreamError::Closed),
            None => Ok(None),
        }
    }

    async fn pop_async(&self) -> Result<QueueItem, StreamError> {
        loop {
            let mut notified = pin!(self.notify.notified());
            notified.as_mut().enable();
            if let Some(item) = self.try_pop()? {
                return Ok(item);
            }
            notified.await;
        }
    }

    fn pop_blocking(&self) -> Result<QueueItem, StreamError> {
        let mut inner = self.lock();
        loop {
            if let Some(item) = Self::pop(&mut inner) {
                return Ok(item);
            }
            if inner.closed {
                return Err(StreamError::Closed);
            }
            inner = self.cond.wait(inner).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// A level as the event loop sees it: where its updates go, type-erased.
pub(crate) trait LevelRoute: Send + Sync + 'static {
    fn on_update(&self, received: Instant, t: Option<Second>, missed: u32, data: &[u8]);
    /// The link went down: `keep` after `disconnect().keep_streams()`, the observers wait for a resume.
    fn on_disconnected(&self, keep: bool);
    /// The event loop is gone.
    fn on_detached(&self);
    fn as_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
    /// Commands that subscribe again on a new connection, None without observers.
    fn resume(self: Arc<Self>, timeout: Duration) -> Result<Option<Pending>, Error>;
    /// The answer to a request [Self::resume] built.
    fn resumed(&self, generation: u64, answer: Result<Vec<u8>, Error>) -> Result<(), Error>;
}

/// Where a level's `PropertyUpdate` events go, see [Command::ObserveRoute].
#[doc(hidden)]
pub struct ObserveRouteHook {
    pub(crate) path: Vec<UNib32>,
    pub(crate) route: Arc<dyn LevelRoute>,
}

impl Debug for ObserveRouteHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObserveRouteHook")
            .field("path", &self.path)
            .finish()
    }
}

/// Levels observed through a [Commander](crate::Commander) and its clones, by path.
pub(crate) type ObserveRegistry = Arc<Mutex<HashMap<Vec<UNib32>, Arc<dyn LevelRoute>>>>;

pub(crate) struct Level<U: LevelUpdate> {
    path: Vec<UNib32>,
    state: Mutex<LevelState<U>>,
}

struct LevelState<U: LevelUpdate> {
    observers: BTreeMap<u64, (ObserveConfig, Arc<Queue>)>,
    next_id: u64,
    latest: U::Latest,
    granted: Option<Granted<U::Config>>,
    /// Settings last sent to the device on this connection, None: nothing sent yet (or the last request failed)
    sent: Option<ObserveConfig>,
    /// Requests sent, and the newest one whose answer was applied
    sent_gen: u64,
    granted_gen: u64,
    /// The route is registered with the current event loop
    routed: bool,
    /// The next answer is to a resume
    resuming: bool,
    /// Device time of the last update, µs, unwrapped (see `batch::unwrap_t`)
    last_t: Option<u64>,
}

impl<U: LevelUpdate> Level<U> {
    fn lock(&self) -> MutexGuard<'_, LevelState<U>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl<U: LevelUpdate> LevelRoute for Level<U> {
    fn on_update(&self, received: Instant, t: Option<Second>, missed: u32, data: &[u8]) {
        let mut state = self.lock();
        let t = t.map(|t| unwrap_t(&mut state.last_t, t));
        let raw = Arc::new(RawUpdate {
            data: data.to_vec(),
            t,
            missed,
            received,
        });
        match U::from_ww_bytes_owned(data) {
            Ok(update) => update.update_latest(&mut state.latest, received, t),
            Err(e) => warn!("property update of {:?} not decoded: {e:?}", self.path),
        }
        for (_, queue) in state.observers.values() {
            queue.push(QueueItem::Update(raw.clone()));
        }
    }

    fn on_disconnected(&self, keep: bool) {
        let mut state = self.lock();
        state.routed = false;
        state.sent = None;
        // the device may have restarted its clock
        state.last_t = None;
        for (_, queue) in state.observers.values() {
            queue.push(QueueItem::Disconnected);
            if !keep {
                queue.close();
            }
        }
    }

    fn on_detached(&self) {
        let state = self.lock();
        if state.routed {
            for (_, queue) in state.observers.values() {
                queue.close();
            }
        }
    }

    fn as_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }

    fn resume(self: Arc<Self>, timeout: Duration) -> Result<Option<Pending>, Error> {
        let mut state = self.lock();
        if state.observers.is_empty() {
            return Ok(None);
        }
        state.resuming = true;
        self.pending(&mut state, timeout, true).map(Some)
    }

    fn resumed(&self, generation: u64, answer: Result<Vec<u8>, Error>) -> Result<(), Error> {
        self.answered(generation, answer).map(|_| ())
    }
}

/// The answer to a request sent for a level, and the request's generation
pub(crate) type Wait = Option<(ResponseReceiver, u64)>;

/// A request to send for a level, built under its lock
pub(crate) struct Pending {
    route: Option<Command>,
    request: Option<(Command, ResponseReceiver, u64)>,
}

impl<U: LevelUpdate> Level<U> {
    /// Commands that bring the device in line with the observers: the route if not registered on this connection,
    /// and the merged settings if they changed (or `force`).
    fn pending(
        self: &Arc<Self>,
        state: &mut LevelState<U>,
        timeout: Duration,
        force: bool,
    ) -> Result<Pending, Error> {
        let route = if state.routed {
            None
        } else {
            state.routed = true;
            Some(Command::ObserveRoute {
                hook: Box::new(ObserveRouteHook {
                    path: self.path.clone(),
                    route: self.clone(),
                }),
            })
        };
        let merged = ObserveConfig::merge(state.observers.values().map(|(cfg, _)| cfg));
        if !force && state.sent.as_ref() == Some(&merged) {
            return Ok(Pending {
                route,
                request: None,
            });
        }
        let bytes = request_bytes(&self.path, &merged)?;
        let (done_tx, done_rx) = oneshot::channel();
        state.sent = Some(merged);
        state.sent_gen += 1;
        Ok(Pending {
            route,
            request: Some((
                Command::SendMessage {
                    bytes,
                    done_tx: Some((done_tx, timeout)),
                },
                done_rx,
                state.sent_gen,
            )),
        })
    }

    /// The answer to request `generation`: applied unless a newer one was already.
    fn answered(
        &self,
        generation: u64,
        answer: Result<Vec<u8>, Error>,
    ) -> Result<Granted<U::Config>, Error> {
        let mut state = self.lock();
        let granted = answer.and_then(|bytes| parse_granted::<U::Config>(&bytes));
        let granted = match granted {
            Ok(granted) => granted,
            Err(e) => {
                if generation == state.sent_gen {
                    // sent again on the next change
                    state.sent = None;
                }
                return Err(e);
            }
        };
        if generation >= state.granted_gen {
            state.granted_gen = generation;
            state.granted = Some(granted.clone());
            if std::mem::take(&mut state.resuming) {
                for (_, queue) in state.observers.values() {
                    queue.push(QueueItem::Resumed);
                }
            }
        }
        Ok(granted)
    }
}

fn request_bytes(path: &[UNib32], cfg: &ObserveConfig) -> Result<Vec<u8>, Error> {
    let entries = cfg
        .entries
        .iter()
        .map(|(&index, &mode)| ObserveEntry {
            index: UNib32(index),
            mode,
        })
        .collect();
    let req = RequestOwned {
        seq: UVlq32Backfill(0),
        path_kind: PathKindOwned::Absolute {
            path: path.to_vec(),
        },
        kind: RequestKindOwned::Observe {
            cfg: ObserveRequestOwned {
                entries,
                patch: false,
                budget: cfg.budget.map(UNib32),
            },
        },
    };
    Ok(req.to_ww_bytes_owned()?)
}

fn parse_granted<C: LevelConfig>(bytes: &[u8]) -> Result<Granted<C>, Error> {
    let granted = ObserveGrantedOwned::from_ww_bytes_owned(bytes)?;
    let entries = granted
        .entries
        .iter()
        .map(|e| (e.index.0, e.mode))
        .collect();
    Ok(Granted {
        modes: C::from_entries(&entries),
        budget: granted.budget.0,
        refused: granted.refused,
    })
}

/// Send what [Level::pending] built, in order: the caller reserved the places in the command queue before it took
/// the lock, so that requests of different observers reach the device in the order they were built.
pub(crate) fn send_pending(
    permits: tokio::sync::mpsc::PermitIterator<'_, Command>,
    pending: Pending,
) -> Wait {
    let mut permits = permits;
    if let Some(route) = pending.route {
        permits.next().expect("2 reserved").send(route);
    }
    let (cmd, done_rx, generation) = pending.request?;
    permits.next().expect("2 reserved").send(cmd);
    Some((done_rx, generation))
}

/// The level at `path`, a new one if it isn't observed yet.
fn level_of<U: LevelUpdate>(
    levels: &mut HashMap<Vec<UNib32>, Arc<dyn LevelRoute>>,
    path: &[UNib32],
) -> Result<Arc<Level<U>>, Error> {
    if let Some(level) = levels.get(path) {
        return level.clone().as_any().downcast::<Level<U>>().map_err(|_| {
            Error::User(format!(
                "level {path:?} is observed with another update type"
            ))
        });
    }
    let level = Arc::new(Level::<U> {
        path: path.to_vec(),
        state: Mutex::new(LevelState {
            observers: BTreeMap::new(),
            next_id: 0,
            latest: Default::default(),
            granted: None,
            sent: None,
            sent_gen: 0,
            granted_gen: 0,
            routed: false,
            resuming: false,
            last_t: None,
        }),
    });
    levels.insert(path.to_vec(), level.clone());
    Ok(level)
}

/// Observes a trait level's properties: every update of the level (`T` = `<Trait>Update`), or the values of one
/// property (`T` its type, updates without it skipped). Dropping it stops observing what only it asked for.
pub struct Observer<U: LevelUpdate, T = U> {
    level: Arc<Level<U>>,
    id: u64,
    queue: Arc<Queue>,
    transport: TransportCommander,
    extract: fn(U) -> Option<T>,
    _phantom: PhantomData<fn() -> T>,
}

/// A subscription being set up, see [crate::Commander::prepare_observe].
#[doc(hidden)]
pub struct PreparedObserve<U: LevelUpdate, T> {
    pub(crate) postpone_err: Result<(), Error>,
    pub(crate) transport: TransportCommander,
    pub(crate) path: Vec<UNib32>,
    pub(crate) cfg: ObserveConfig,
    pub(crate) queue_bytes: usize,
    pub(crate) extract: fn(U) -> Option<T>,
}

impl<U: LevelUpdate, T> PreparedObserve<U, T> {
    /// Bytes per second the level's updates may take on the link, by default the device decides. Observers of a
    /// level asking for different budgets get the biggest one.
    pub fn budget(mut self, bytes_per_s: u32) -> Self {
        self.cfg.budget = Some(bytes_per_s);
        self
    }

    /// Bytes of updates queued for this observer on the host, see
    /// [ClientConfig::stream_queue_bytes](crate::ClientConfig::stream_queue_bytes).
    pub fn queue_bytes(mut self, bytes: usize) -> Self {
        self.queue_bytes = bytes;
        self
    }

    /// Register the observer and build the commands, the permits reserved.
    fn start(
        &self,
        permits: tokio::sync::mpsc::PermitIterator<'_, Command>,
    ) -> Result<(Observer<U, T>, Wait), Error> {
        // held until the observer is in, so that the last observer going doesn't remove the level meanwhile
        let mut levels = self
            .transport
            .observes
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let level = level_of::<U>(&mut levels, &self.path)?;
        let mut state = level.lock();
        let id = state.next_id;
        state.next_id += 1;
        let queue = Queue::new(self.queue_bytes);
        state
            .observers
            .insert(id, (self.cfg.clone(), queue.clone()));
        let pending = level.pending(&mut state, self.transport.default_timeout(), false);
        drop(state);
        let observer = Observer {
            level: level.clone(),
            id,
            queue,
            transport: self.transport.clone(),
            extract: self.extract,
            _phantom: PhantomData,
        };
        Ok((observer, send_pending(permits, pending?)))
    }

    /// Subscribe: returns once the device answered with what it applies.
    pub async fn subscribe(mut self) -> Result<Observer<U, T>, Error> {
        std::mem::replace(&mut self.postpone_err, Ok(()))?;
        let permits = self.transport.reserve(2).await?;
        let (observer, wait) = self.start(permits)?;
        if let Some((done_rx, generation)) = wait {
            let answer = done_rx.await.unwrap_or(Err(Error::EventLoopNotRunning));
            observer.level.answered(generation, answer)?;
        }
        Ok(observer)
    }

    /// Blocking variant of [Self::subscribe].
    pub fn subscribe_blocking(mut self) -> Result<Observer<U, T>, Error> {
        std::mem::replace(&mut self.postpone_err, Ok(()))?;
        let permits = futures_lite::future::block_on(self.transport.reserve(2))?;
        let (observer, wait) = self.start(permits)?;
        if let Some((done_rx, generation)) = wait {
            let answer = done_rx
                .blocking_recv()
                .unwrap_or(Err(Error::EventLoopNotRunning));
            observer.level.answered(generation, answer)?;
        }
        Ok(observer)
    }
}

impl<U: LevelUpdate, T> Observer<U, T> {
    fn decode(&self, item: QueueItem) -> Result<Option<ObserveEvent<T>>, StreamError> {
        Ok(Some(match item {
            QueueItem::Update(raw) => {
                let update = U::from_ww_bytes_owned(&raw.data)?;
                let Some(value) = (self.extract)(update) else {
                    return Ok(None);
                };
                ObserveEvent::Update(Observed {
                    value,
                    t: raw.t,
                    missed: raw.missed,
                    received: raw.received,
                })
            }
            QueueItem::Disconnected => ObserveEvent::Disconnected,
            QueueItem::Resumed => ObserveEvent::Resumed,
            QueueItem::Dropped { events } => ObserveEvent::Dropped { events },
        }))
    }

    /// Next update, [ObserveEvent::Disconnected], [ObserveEvent::Resumed] or [ObserveEvent::Dropped].
    /// [StreamError::Closed] once nothing more can come (the link went down without `keep_streams`).
    pub async fn recv_event(&mut self) -> Result<ObserveEvent<T>, StreamError> {
        loop {
            let item = self.queue.pop_async().await?;
            if let Some(ev) = self.decode(item)? {
                return Ok(ev);
            }
        }
    }

    /// Blocking variant of [Self::recv_event].
    pub fn recv_event_blocking(&mut self) -> Result<ObserveEvent<T>, StreamError> {
        loop {
            let item = self.queue.pop_blocking()?;
            if let Some(ev) = self.decode(item)? {
                return Ok(ev);
            }
        }
    }

    /// Next update, skipping the other events.
    pub async fn recv(&mut self) -> Result<Observed<T>, StreamError> {
        loop {
            if let ObserveEvent::Update(u) = self.recv_event().await? {
                return Ok(u);
            }
        }
    }

    /// Blocking variant of [Self::recv].
    pub fn recv_blocking(&mut self) -> Result<Observed<T>, StreamError> {
        loop {
            if let ObserveEvent::Update(u) = self.recv_event_blocking()? {
                return Ok(u);
            }
        }
    }

    /// An update if one is queued, skipping the other events.
    pub fn try_recv(&mut self) -> Result<Option<Observed<T>>, StreamError> {
        while let Some(item) = self.queue.try_pop()? {
            if let Some(ObserveEvent::Update(u)) = self.decode(item)? {
                return Ok(Some(u));
            }
        }
        Ok(None)
    }

    /// Updates dropped because this observer's queue was full.
    pub fn dropped(&self) -> u64 {
        self.queue.lock().dropped_total
    }

    /// What the device applies to the level (for all its observers), None before it answered: as of the last
    /// answer the host waited for (a subscribe, `configure` or resume); the request sent when an observer is dropped
    /// isn't waited for.
    pub fn granted(&self) -> Option<Granted<U::Config>> {
        self.level.lock().granted.clone()
    }

    /// Look at the latest value of each of the level's properties, with when it arrived.
    pub fn with_latest<R>(&self, f: impl FnOnce(&U::Latest) -> R) -> R {
        f(&self.level.lock().latest)
    }

    /// The latest value of each of the level's properties, with when it arrived.
    pub fn latest(&self) -> U::Latest
    where
        U::Latest: Clone,
    {
        self.with_latest(|l| l.clone())
    }

    fn reconfigure(
        &self,
        permits: tokio::sync::mpsc::PermitIterator<'_, Command>,
        cfg: &U::Config,
        budget: Option<Option<u32>>,
    ) -> Result<Wait, Error> {
        let mut state = self.level.lock();
        if let Some((own, _)) = state.observers.get_mut(&self.id) {
            own.entries = cfg.to_entries();
            if let Some(budget) = budget {
                own.budget = budget;
            }
        }
        let pending = self
            .level
            .pending(&mut state, self.transport.default_timeout(), false)?;
        Ok(send_pending(permits, pending))
    }

    fn current(&self) -> Result<Granted<U::Config>, Error> {
        self.granted()
            .ok_or_else(|| Error::User("the device didn't answer an observe request yet".into()))
    }

    /// Change what this observer asks for, its budget stays; returns what the device applies to the level now.
    pub async fn configure(&self, cfg: U::Config) -> Result<Granted<U::Config>, Error> {
        self.configure_inner(cfg, None).await
    }

    /// Change what this observer asks for and its budget (`None` leaves it to the device).
    pub async fn configure_with_budget(
        &self,
        cfg: U::Config,
        budget: Option<u32>,
    ) -> Result<Granted<U::Config>, Error> {
        self.configure_inner(cfg, Some(budget)).await
    }

    async fn configure_inner(
        &self,
        cfg: U::Config,
        budget: Option<Option<u32>>,
    ) -> Result<Granted<U::Config>, Error> {
        let permits = self.transport.reserve(2).await?;
        match self.reconfigure(permits, &cfg, budget)? {
            Some((done_rx, generation)) => {
                let answer = done_rx.await.unwrap_or(Err(Error::EventLoopNotRunning));
                self.level.answered(generation, answer)
            }
            None => self.current(),
        }
    }

    /// Blocking variant of [Self::configure].
    pub fn configure_blocking(&self, cfg: U::Config) -> Result<Granted<U::Config>, Error> {
        let permits = futures_lite::future::block_on(self.transport.reserve(2))?;
        match self.reconfigure(permits, &cfg, None)? {
            Some((done_rx, generation)) => {
                let answer = done_rx
                    .blocking_recv()
                    .unwrap_or(Err(Error::EventLoopNotRunning));
                self.level.answered(generation, answer)
            }
            None => self.current(),
        }
    }
}

impl<U: LevelUpdate, T> Drop for Observer<U, T> {
    fn drop(&mut self) {
        let registry = &self.transport.observes;
        let mut levels = registry.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.level.lock();
        state.observers.remove(&self.id);
        if state.observers.is_empty() {
            levels.remove(&self.level.path);
        }
        if !state.routed {
            // not connected: nothing to tell the device
            return;
        }
        let Ok(Some(permits)) = self.transport.try_reserve_many(2) else {
            warn!(
                "command queue full, observe settings of {:?} not updated",
                self.level.path
            );
            return;
        };
        match self
            .level
            .pending(&mut state, self.transport.default_timeout(), false)
        {
            // nobody waits for the answer
            Ok(pending) => _ = send_pending(permits, pending),
            Err(e) => debug!("observe request not built: {e:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(entries: &[(u32, ObserveMode)], budget: Option<u32>) -> ObserveConfig {
        ObserveConfig {
            entries: entries.iter().copied().collect(),
            budget,
        }
    }

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn merge_gives_everyone_at_least_what_it_asked_for() {
        let a = cfg(&[(0, on_change(10 * MS)), (1, periodic(5 * MS))], Some(100));
        let b = cfg(
            &[
                (0, periodic(50 * MS)),
                (1, periodic(2 * MS)),
                (2, ObserveMode::Off),
            ],
            None,
        );
        let c = cfg(&[(0, on_change(4 * MS)), (3, on_change(MS))], Some(300));
        assert_eq!(
            ObserveConfig::merge([&a, &b, &c]),
            cfg(
                &[
                    (0, on_change_or_beat(4 * MS, 50 * MS)),
                    (1, periodic(2 * MS)),
                    (3, on_change(MS)),
                ],
                Some(300)
            )
        );
        // a beat shorter than the on-change interval is just periodic
        let d = cfg(&[(0, on_change(10 * MS))], None);
        let e = cfg(&[(0, periodic(MS))], None);
        assert_eq!(
            ObserveConfig::merge([&d, &e]),
            cfg(&[(0, periodic(MS))], None)
        );
        assert_eq!(ObserveConfig::merge([]), ObserveConfig::default());
    }
}
