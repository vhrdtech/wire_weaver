//! The mount table of a server (SRV-78): what is mounted where, whether each backend is up, and the traits of the
//! mounted APIs. Shared by every front connection's [Forwarder](super::Forwarder) and driven by [Mounts::run].

use std::collections::BTreeMap;
use std::fmt;
use std::future::{pending, poll_fn};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::time::Duration;

use futures_util::future::{select, select_all};
use futures_util::pin_mut;
use wire_weaver::shrink_wrap::{
    DeserializeShrinkWrap, DeserializeShrinkWrapOwned, SerializeShrinkWrap, UNib32, UVlq32Backfill,
};
use wire_weaver::ww_api::mount::MountPresenceOwned;
use ww_byte_stream::Target;
use ww_client_server::{
    Event, EventKind, PathKind, Priority, Request, RequestKind, ResourceIndex, StreamSideband,
};
use ww_device::{Clock, Instant};
use ww_link::DeviceInfoOwned;
use ww_self::ApiBundleOwned;

use super::MountedTraits;
use super::connect::Connect;
use super::link::{BackendLink, LinkDown, LinkEvent, LinkParams};
use crate::fanout::{Fanout, Subscription};
use crate::sync::locked;

/// The longest message a control link takes: introspection comes in small chunks.
const CONTROL_MESSAGE_LEN_MAX: usize = 64 * 1024;
/// The longest introspection data learned from a backend; a longer one is not learned.
const INTROSPECTION_LEN_MAX: usize = 4 * 1024 * 1024;
/// Events a front connection may fall behind on before it is told it lagged.
const EVENTS_QUEUED_MAX: usize = 64;
/// The seq of a control link's introspection request: the only request it sends.
const INTROSPECT_SEQ: u32 = 1;

/// How a slot reaches its backend, beside its [Connect].
#[derive(Clone, Debug)]
pub struct MountOptions {
    /// What the preamble asks for: `Target::Device("")`, the default, is the only API of a server's listener;
    /// `Target::Service(name)` one of several on one listener.
    pub target: Target,
    /// The slot's control link tries again this long after its backend went away or could not be reached: how soon
    /// a restarted backend is up again. 1 s by default.
    pub retry_after: Duration,
}

impl Default for MountOptions {
    fn default() -> Self {
        MountOptions {
            target: Target::Device(String::new()),
            retry_after: Duration::from_secs(1),
        }
    }
}

/// What changed in the mount table, as [Mounts::subscribe] tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountEvent {
    /// A backend was mounted at the slot whose `api` is at `api_path`; its presence is `Absent` until it is up.
    Mounted { api_path: Vec<ResourceIndex> },
    /// The slot's presence changed: its backend came up, or went away. A front connection's own link to the
    /// backend went with it.
    Presence { api_path: Vec<ResourceIndex> },
    /// Nothing is mounted at the slot any more.
    Unmounted { api_path: Vec<ResourceIndex> },
}

/// Turns a backend's introspection data (the bytes of its introspection stream, compressed or not) into its API,
/// with what it left out put back: `wire_weaver_client::decode_introspection` with a resolver does that. `None`:
/// nothing is learned, global paths don't reach into the slot.
pub type Learn = dyn Fn(&[u8]) -> Option<ApiBundleOwned> + Send + Sync;

/// The global trait paths into the mounted APIs, for [Server::mounts](crate::Server::mounts): every request whose
/// global path reaches into a slot goes on with the front's absolute path into it.
#[derive(Clone, Debug)]
pub struct MountRoutes(Arc<Mutex<MountedTraits>>);

impl MountRoutes {
    /// [MountedTraits::request_into_mount] of the current table; `None` when the request stays as it is, also when
    /// it doesn't decode (the dispatcher answers it).
    pub fn request_into_mount<'b>(&self, request: &[u8], buf: &'b mut [u8]) -> Option<&'b [u8]> {
        locked(&self.0)
            .request_into_mount(request, buf)
            .ok()
            .flatten()
    }
}

/// The mount table: slots by the path of their `api`, each with how to reach its backend and whether it is up.
/// Clones share it.
pub struct Mounts<C: Connect> {
    table: Arc<Mutex<Table<C>>>,
    events: Fanout<MountEvent>,
    routes: MountRoutes,
    learn: Arc<Learn>,
}

impl<C: Connect> Clone for Mounts<C> {
    fn clone(&self) -> Self {
        Mounts {
            table: self.table.clone(),
            events: self.events.clone(),
            routes: self.routes.clone(),
            learn: self.learn.clone(),
        }
    }
}

impl<C: Connect> fmt::Debug for Mounts<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let table = locked(&self.table);
        f.debug_struct("Mounts")
            .field("slots", &table.slots.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

struct Table<C> {
    slots: BTreeMap<Vec<u32>, Slot<C>>,
    /// Bumped on every mount and unmount, so that [Mounts::run] sees the change.
    version: u64,
    /// [Mounts::run] waits on this for a change.
    waker: Option<Waker>,
}

/// One mounted slot.
pub(crate) struct Slot<C> {
    pub connector: Arc<C>,
    pub options: MountOptions,
    /// Tells this mount from an earlier one at the same slot.
    pub epoch: u64,
    pub presence: MountPresenceOwned,
    /// The backend's facts while it is up.
    pub info: Option<DeviceInfoOwned>,
    /// Why its control link last went down or failed to come up.
    pub last_down: Option<String>,
}

impl<C: Connect> Mounts<C> {
    /// An empty table of a server whose own API is `own`: its traits come first for global paths.
    pub fn new(own: Arc<ApiBundleOwned>) -> Self {
        Mounts {
            table: Arc::new(Mutex::new(Table {
                slots: BTreeMap::new(),
                version: 0,
                waker: None,
            })),
            events: Fanout::new(EVENTS_QUEUED_MAX),
            routes: MountRoutes(Arc::new(Mutex::new(MountedTraits::new(own)))),
            learn: Arc::new(decode_as_sent),
        }
    }

    /// Learn each backend's API with `learn` instead of decoding its introspection data as sent. It runs inside
    /// [run](Self::run): one that reads files or fetches holds up the table's other slots meanwhile.
    #[must_use]
    pub fn learn_with(mut self, learn: impl Fn(&[u8]) -> Option<ApiBundleOwned> + Send + Sync + 'static) -> Self {
        self.learn = Arc::new(learn);
        self
    }

    /// Mount the backend `connector` reaches at the slot whose `api` is at `api_path` (`[hub, 1]` for `mount!(hub)`,
    /// `[ext, index, 1]` for an element of `mounts!(ext)`), replacing what was mounted there. Its presence is `Absent`
    /// until [run](Self::run) has set its link up.
    pub fn mount(&self, api_path: &[ResourceIndex], connector: C, options: MountOptions) {
        let key = ids(api_path);
        {
            let mut table = locked(&self.table);
            table.version += 1;
            let epoch = table.version;
            table.slots.insert(
                key,
                Slot {
                    connector: Arc::new(connector),
                    options,
                    epoch,
                    presence: MountPresenceOwned::Absent,
                    info: None,
                    last_down: None,
                },
            );
            if let Some(waker) = table.waker.take() {
                waker.wake();
            }
        }
        locked(&self.routes.0).unmount(api_path);
        self.events.send(MountEvent::Mounted {
            api_path: api_path.to_vec(),
        });
    }

    /// Nothing is mounted at the slot whose `api` is at `api_path` any more; false when nothing was.
    pub fn unmount(&self, api_path: &[ResourceIndex]) -> bool {
        {
            let mut table = locked(&self.table);
            if table.slots.remove(&ids(api_path)).is_none() {
                return false;
            }
            table.version += 1;
            if let Some(waker) = table.waker.take() {
                waker.wake();
            }
        }
        locked(&self.routes.0).unmount(api_path);
        self.events.send(MountEvent::Unmounted {
            api_path: api_path.to_vec(),
        });
        true
    }

    /// Presence of the slot whose `api` is at `api_path`: `Absent` when nothing is mounted or its backend is not up.
    pub fn presence(&self, api_path: &[ResourceIndex]) -> MountPresenceOwned {
        locked(&self.table)
            .slots
            .get(&ids(api_path))
            .map_or(MountPresenceOwned::Absent, |slot| slot.presence.clone())
    }

    /// Why the slot's backend last went away or could not be reached: for logs and status lines.
    pub fn last_down(&self, api_path: &[ResourceIndex]) -> Option<String> {
        locked(&self.table).slots.get(&ids(api_path))?.last_down.clone()
    }

    /// The `api` paths of the mounted slots, in path order.
    pub fn slots(&self) -> Vec<Vec<ResourceIndex>> {
        locked(&self.table)
            .slots
            .keys()
            .map(|key| key.iter().map(|id| ResourceIndex::new(UNib32(*id))).collect())
            .collect()
    }

    /// The global trait paths into the mounted APIs, for [Server::mounts](crate::Server::mounts).
    pub fn routes(&self) -> MountRoutes {
        self.routes.clone()
    }

    /// What changes from now on. A front connection's [Forwarder](super::Forwarder) subscribes itself.
    pub fn subscribe(&self) -> Subscription<MountEvent> {
        self.events.subscribe()
    }

    /// The slot's backend, its options, its epoch and its facts, while it is up.
    pub(crate) fn backend(
        &self,
        api_path: &[ResourceIndex],
    ) -> Option<(Arc<C>, MountOptions, u64, BackendFacts)> {
        let table = locked(&self.table);
        let slot = table.slots.get(&ids(api_path))?;
        slot.info.as_ref().map(|info| {
            let facts = BackendFacts {
                request_context: info.request_context,
                dev_max_message_len: info.dev_max_message_len,
            };
            (slot.connector.clone(), slot.options.clone(), slot.epoch, facts)
        })
    }

    /// The epoch of the slot's mount while its backend is up: a front link of another epoch is stale.
    pub(crate) fn epoch_up(&self, api_path: &[ResourceIndex]) -> Option<u64> {
        let table = locked(&self.table);
        let slot = table.slots.get(&ids(api_path))?;
        slot.info.as_ref().map(|_| slot.epoch)
    }

    /// Keeps a control link to each mounted backend: its presence follows the link, its API is learned for global
    /// paths when it comes up. Never ends; drop it to stop. Run it beside the server, on the same executor.
    pub async fn run(&self, clock: impl Clock) {
        let mut controls: BTreeMap<Vec<u32>, Control<C>> = BTreeMap::new();
        let mut seen_version = None;
        loop {
            let now = clock.now();
            let version = locked(&self.table).version;
            if seen_version != Some(version) {
                seen_version = Some(version);
                self.sync(&mut controls, now);
            }
            for (key, control) in &mut controls {
                self.step(key, control, now);
            }
            for control in controls.values_mut() {
                if let Some(link) = &mut control.link {
                    link.flush().await;
                }
            }
            let wake = controls.values().filter_map(Control::next_wake).min();
            self.wait(&mut controls, &clock, wake, version).await;
        }
    }

    /// Controls for exactly the mounted slots, a new one for a slot mounted again.
    fn sync(&self, controls: &mut BTreeMap<Vec<u32>, Control<C>>, now: Instant) {
        let table = locked(&self.table);
        controls.retain(|key, control| {
            table
                .slots
                .get(key)
                .is_some_and(|slot| slot.epoch == control.epoch)
        });
        for (key, slot) in &table.slots {
            controls.entry(key.clone()).or_insert_with(|| Control {
                epoch: slot.epoch,
                connector: slot.connector.clone(),
                target: slot.options.target.clone(),
                retry_after: slot.options.retry_after,
                link: None,
                retry_at: Some(now),
                introspection: Vec::new(),
                too_big: false,
            });
        }
    }

    /// Handle what a slot's control link has to say at `now`.
    fn step(&self, key: &[u32], control: &mut Control<C>, now: Instant) {
        if control.link.is_none() && control.retry_at.is_some_and(|at| at <= now) {
            control.retry_at = None;
            control.introspection.clear();
            control.too_big = false;
            let params = LinkParams {
                target: control.target.clone(),
                message_len_max: CONTROL_MESSAGE_LEN_MAX,
                model_wire: Some(ww_client_server::WIRE_VERSION),
            };
            control.link = Some(BackendLink::open(&*control.connector, params, now));
        }
        let Some(link) = &mut control.link else {
            return;
        };
        let mut learned = None;
        let mut down = None;
        while let Some(event) = link.next_event(now) {
            match event {
                LinkEvent::Up => {
                    let mut buf = [0u8; 32];
                    let bytes = introspect_request(&mut buf);
                    link.send(bytes, now);
                }
                LinkEvent::Data(bytes) => match introspection_part(bytes) {
                    Part::Chunk(chunk) => {
                        if control.introspection.len() + chunk.len() <= INTROSPECTION_LEN_MAX {
                            control.introspection.extend_from_slice(chunk);
                        } else {
                            control.too_big = true;
                        }
                    }
                    Part::End => {
                        let bytes = std::mem::take(&mut control.introspection);
                        learned = Some((!std::mem::take(&mut control.too_big)).then_some(bytes));
                    }
                    Part::Other => {}
                },
                LinkEvent::Down(why) => {
                    down = Some(why);
                    break;
                }
            }
        }
        let api_path = paths(key);
        if let Some(bytes) = learned {
            let info = link.info().cloned().expect("introspection comes after link setup");
            // empty: a backend with introspection off; `None`: too long to learn
            let api = bytes.filter(|b| !b.is_empty()).and_then(|b| (self.learn)(&b));
            self.up(key, &api_path, control.epoch, info, api);
        }
        if let Some(why) = down {
            control.link = None;
            control.retry_at = Some(now + control.retry_after);
            self.absent(key, &api_path, control.epoch, &why);
        }
    }

    /// The slot's backend came up with `info` and serves `api`.
    fn up(
        &self,
        key: &[u32],
        api_path: &[ResourceIndex],
        epoch: u64,
        info: DeviceInfoOwned,
        api: Option<ApiBundleOwned>,
    ) {
        {
            let mut table = locked(&self.table);
            let Some(slot) = table.slots.get_mut(key).filter(|slot| slot.epoch == epoch) else {
                return;
            };
            slot.presence = MountPresenceOwned::Up {
                api: info.user_api_version.clone(),
                hash: info.hash.clone(),
                wire: info.user_api_wire,
            };
            slot.info = Some(info);
        }
        {
            let mut routes = locked(&self.routes.0);
            match api {
                Some(api) => routes.mount(api_path, Arc::new(api)),
                None => routes.unmount(api_path),
            }
        }
        self.events.send(MountEvent::Presence {
            api_path: api_path.to_vec(),
        });
    }

    /// The slot's backend went away, or never came up.
    fn absent(&self, key: &[u32], api_path: &[ResourceIndex], epoch: u64, why: &LinkDown) {
        {
            let mut table = locked(&self.table);
            let Some(slot) = table.slots.get_mut(key).filter(|slot| slot.epoch == epoch) else {
                return;
            };
            slot.last_down = Some(why.to_string());
            if slot.info.take().is_none() {
                // was not up: nothing changes for the clients
                return;
            }
            slot.presence = MountPresenceOwned::Absent;
        }
        locked(&self.routes.0).unmount(api_path);
        self.events.send(MountEvent::Presence {
            api_path: api_path.to_vec(),
        });
    }

    /// Wait for a control link's progress, its next timer or a change of the table at `version`.
    async fn wait(
        &self,
        controls: &mut BTreeMap<Vec<u32>, Control<C>>,
        clock: &impl Clock,
        wake: Option<Instant>,
        version: u64,
    ) {
        let ready: Vec<_> = controls
            .values_mut()
            .filter_map(|control| control.link.as_mut())
            .map(|link| Box::pin(link.ready()))
            .collect();
        let links = async move {
            if ready.is_empty() {
                pending::<()>().await;
            } else {
                select_all(ready).await;
            }
        };
        let timer = async move {
            match wake {
                Some(at) => clock.wait_until(at).await,
                None => pending().await,
            }
        };
        let table = &self.table;
        let changed = poll_fn(move |cx| {
            let mut table = locked(table);
            if table.version == version {
                table.waker = Some(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        });
        pin_mut!(links, timer, changed);
        select(select(links, timer), changed).await;
    }
}

/// What a front link needs of its backend, from the control link's `DeviceInfo`.
#[derive(Copy, Clone, Debug)]
pub(crate) struct BackendFacts {
    /// The backend reads request contexts and cancels.
    pub request_context: bool,
    /// The longest request it takes.
    pub dev_max_message_len: u32,
}

/// A slot's control link and what it learns.
struct Control<C: Connect> {
    epoch: u64,
    connector: Arc<C>,
    target: Target,
    retry_after: Duration,
    link: Option<BackendLink<C>>,
    /// When to open the link again, while there is none.
    retry_at: Option<Instant>,
    /// Introspection data received so far.
    introspection: Vec<u8>,
    /// The introspection data is longer than [INTROSPECTION_LEN_MAX]: not learned.
    too_big: bool,
}

impl<C: Connect> Control<C> {
    fn next_wake(&self) -> Option<Instant> {
        match &self.link {
            Some(link) => link.next_wake(),
            None => self.retry_at,
        }
    }
}

/// What an event of a control link is to the introspection it asked for.
enum Part<'a> {
    Chunk(&'a [u8]),
    End,
    Other,
}

fn introspection_part(bytes: &[u8]) -> Part<'_> {
    let Ok(Event { seq, result }) = Event::from_ww_bytes(bytes) else {
        return Part::Other;
    };
    if seq.0 != INTROSPECT_SEQ {
        return Part::Other;
    }
    match result {
        Ok(EventKind::StreamData { data, .. }) => Part::Chunk(data.0),
        Ok(EventKind::StreamSideband {
            sideband: StreamSideband::Close,
            ..
        })
        // a backend without introspection answers with an error: nothing to learn
        | Err(_) => Part::End,
        Ok(_) => Part::Other,
    }
}

/// The control link's only request: the backend's introspection data, at its root.
fn introspect_request(buf: &mut [u8]) -> &[u8] {
    let request = Request {
        seq: UVlq32Backfill(INTROSPECT_SEQ),
        priority: Priority::NORMAL.to_wire(),
        context: None,
        path_kind: PathKind::absolute(&[]),
        kind: RequestKind::Introspect,
    };
    let len = request
        .to_ww_bytes(buf)
        .expect("an introspection request fits 32 bytes")
        .len();
    Request::set_seq(&mut buf[..len], INTROSPECT_SEQ).expect("the request was just written")
}

/// Introspection data as sent, decoded; what it left out stays out.
fn decode_as_sent(bytes: &[u8]) -> Option<ApiBundleOwned> {
    let bytes = ww_self::compressed::bundle_bytes(bytes).ok()?;
    ApiBundleOwned::from_ww_bytes_owned(&bytes).ok()
}

pub(crate) fn ids(path: &[ResourceIndex]) -> Vec<u32> {
    path.iter().map(|id| id.get().0).collect()
}

fn paths(key: &[u32]) -> Vec<ResourceIndex> {
    key.iter().map(|id| ResourceIndex::new(UNib32(*id))).collect()
}
