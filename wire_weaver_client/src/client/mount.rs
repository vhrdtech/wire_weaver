//! Mount slots (SRV-76): `mount!(hub)` and the elements of `mounts!(ext)`, levels whose API is known only at run
//! time. A [MountSlot] reads the slot's `presence` and names the path of the mounted API's root, which a view of the
//! mounted API prefixes its own paths with.

use crate::client::keyed::{as_ref, path_mut, to_owned};
use crate::client::observe::{
    LevelUpdate, ObserveEvent, Observer, PreparedObserve, Second, on_change,
};
use crate::client::prepared_connection::learn_mounted;
use crate::config::IntrospectBundle;
use crate::device_info::DeviceApiInfo;
use crate::event_loop::commander::WeakCommander;
use crate::{ClientLine, Commander, DynConfig, Error, PreparedRead, WwClient};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tracing::{debug, warn};
use wire_weaver::prelude::UNib32;
use wire_weaver::shrink_wrap::option_any_vec::OptionAnyVec;
use wire_weaver::shrink_wrap::{
    BufReader, DeserializeShrinkWrapOwned, ElementSize, Error as ShrinkWrapError,
};
use wire_weaver::ww_api::mount::MountPresenceOwned;
use ww_client_server::{PathKind, PathKindOwned, ResourceIndex};
use ww_self::mount::{API, PRESENCE};

/// A mount slot of the connected device: generated `<slot>()` and `<slots>_by(&name)` accessors hand one out.
pub struct MountSlot<'c> {
    cmd: &'c Commander,
    /// Path of the slot's level as the client names it, before the Commander resolves it.
    slot: PathKindOwned,
}

impl Commander {
    /// The mount slot whose level is at `slot`.
    pub fn mount_slot(&self, slot: PathKind<'_>) -> Result<MountSlot<'_>, Error> {
        Ok(MountSlot {
            cmd: self,
            slot: to_owned(slot)?,
        })
    }
}

impl MountSlot<'_> {
    /// Path of the slot's level.
    pub fn path(&self) -> &PathKindOwned {
        &self.slot
    }

    /// Path of the mounted API's root: a request to this path plus the mounted API's own path reaches the backend
    /// with the slot's part taken off, an `Introspect` at it asks for the backend's introspection data.
    pub fn api_path(&self) -> PathKindOwned {
        self.sub(API)
    }

    /// Reads whether a backend is mounted, and which API it serves (its crate, version, hashes and wire version).
    pub fn presence(&self) -> PreparedRead<MountPresenceOwned> {
        self.cmd.prepare_read(as_ref(&self.sub(PRESENCE)))
    }

    fn sub(&self, id: u32) -> PathKindOwned {
        let mut path = self.slot.clone();
        path_mut(&mut path).push(ResourceIndex::new(UNib32(id)));
        path
    }
}

/// A view of the API mounted at a slot (CLT-84): requests through [Self::commander] go to the slot's `api` with their
/// path in the mounted API, checked against that API's own facts (introspection data, hash, global trait paths),
/// learned from the backend, never the device's. The view follows the slot's `presence` while one of its clones
/// lives: it learns the API when a backend comes `Up`, again when its hashes change (CLT-81 per mount), and resumes
/// its streams, sinks and observers on the first `Up` after the link came back. [Self::status] tells the program,
/// apart from the link's state.
#[derive(Clone)]
pub struct MountView {
    /// The link's Commander: reads the slot's `presence`.
    link: Commander,
    /// The view's Commander: the slot's `api` in front of every path, the mounted API's facts.
    view: Commander,
    /// Path of the slot's level as the device names it.
    slot: Vec<ResourceIndex>,
    /// The slot's presence as the view last handled it: `Up` once its API is learned.
    status: watch::Receiver<MountPresenceOwned>,
}

/// The parts of a view before it follows its slot.
struct Parts {
    link: Commander,
    view: Commander,
    slot: Vec<ResourceIndex>,
}

impl MountSlot<'_> {
    /// A view of the API mounted at this slot, its API learned when a backend is mounted now, following the slot
    /// from then on (see [MountView]). Needs a tokio runtime, which runs the follower.
    /// The view knows no client API: the dynamic client's, or a tool's that walks the mounted API's introspection data.
    pub async fn view(&self) -> Result<MountView, Error> {
        self.parts(None)?.start().await
    }

    /// Blocking variant of [Self::view], from outside an async task with a tokio runtime entered.
    pub fn view_blocking(&self) -> Result<MountView, Error> {
        self.parts(None)?.start_blocking()
    }

    /// A view of the API mounted at this slot for a generated client of `line` (its `client_line()`): calls are
    /// checked against the mounted API as a connect checks them against a device's, and the client is made with its
    /// generated `on_mount(&view)`. Otherwise as [Self::view].
    pub async fn view_for(&self, line: &ClientLine) -> Result<MountView, Error> {
        self.parts(Some(line))?.start().await
    }

    /// Blocking variant of [Self::view_for].
    pub fn view_for_blocking(&self, line: &ClientLine) -> Result<MountView, Error> {
        self.parts(Some(line))?.start_blocking()
    }

    fn parts(&self, line: Option<&ClientLine>) -> Result<Parts, Error> {
        let (slot, _) = self.cmd.to_ww_client_server_path(as_ref(&self.slot))?;
        let PathKindOwned::Absolute { path: slot } = slot else {
            return Err(Error::User(format!(
                "a mount view needs the slot's absolute path, got {slot:?}"
            )));
        };
        let mut api = slot.clone();
        api.push(ResourceIndex::new(UNib32(API)));
        let mut view = self.cmd.mount_view(api);
        if let Some(line) = line {
            let bundle = IntrospectBundle::from_client(
                line.ww_self_bytes,
                line.api_hash.clone(),
                view.resolver,
            )
            .map_err(|e| Error::Other(format!("{e:#}")))?;
            view.set_client_introspect(bundle);
        }
        Ok(Parts {
            link: self.cmd.clone(),
            view,
            slot,
        })
    }
}

impl Parts {
    async fn start(self) -> Result<MountView, Error> {
        let observer = self.observe().subscribe().await?;
        let presence = self.learn().await?;
        self.follow(observer, presence)
    }

    fn start_blocking(self) -> Result<MountView, Error> {
        let observer = self.observe().subscribe_blocking()?;
        let presence = self.learn_blocking()?;
        self.follow(observer, presence)
    }

    /// Observing the slot's `presence`, on the link.
    fn observe(&self) -> PreparedObserve<PresenceUpdate, MountPresenceOwned> {
        let modes = DynConfig(BTreeMap::from([(PRESENCE, on_change(Duration::ZERO))]));
        self.link.prepare_observe(
            as_ref(&PathKindOwned::Absolute {
                path: self.slot.clone(),
            }),
            modes,
            |update: PresenceUpdate| update.0,
        )
    }

    fn presence_read(&self) -> PreparedRead<MountPresenceOwned> {
        let mut path = self.slot.clone();
        path.push(ResourceIndex::new(UNib32(PRESENCE)));
        self.link
            .prepare_read(as_ref(&PathKindOwned::Absolute { path }))
    }

    /// Reads the slot's `presence` and learns the API mounted there, see [learn_mounted].
    async fn learn(&self) -> Result<MountPresenceOwned, Error> {
        let presence = self.presence_read().read().await?;
        if let Some(info) = self.mounted_info(&presence) {
            learn_mounted(&self.view, info).await?;
        }
        Ok(presence)
    }

    fn learn_blocking(&self) -> Result<MountPresenceOwned, Error> {
        let presence = self.presence_read().blocking_read()?;
        if let Some(info) = self.mounted_info(&presence) {
            futures_lite::future::block_on(learn_mounted(&self.view, info))?;
        }
        Ok(presence)
    }

    /// The device facts of the mounted API `presence` names, over the link's: `None` while the slot is empty.
    fn mounted_info(&self, presence: &MountPresenceOwned) -> Option<DeviceApiInfo> {
        let MountPresenceOwned::Up { api, hash, wire } = presence else {
            return None;
        };
        let mut info = self.link.device_api_info();
        info.user_api_version = api.clone();
        info.user_api_hash = hash.clone();
        info.user_api_wire = *wire;
        Some(info)
    }

    /// The view, with a task that follows the slot's `presence` until the view's last clone is dropped.
    fn follow(
        self,
        observer: Observer<PresenceUpdate, MountPresenceOwned>,
        presence: MountPresenceOwned,
    ) -> Result<MountView, Error> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|e| Error::Unsupported {
            what: "a mount view",
            reason: format!("it follows its slot on a tokio runtime: {e}"),
        })?;
        let (tx, status) = watch::channel(presence);
        let follower = Follower {
            link: self.link.downgrade(),
            view: self.view.downgrade(),
            slot: self.slot.clone(),
        };
        runtime.spawn(follower.run(observer, tx));
        Ok(MountView {
            link: self.link,
            view: self.view,
            slot: self.slot,
            status,
        })
    }
}

/// What follows a view's slot: handles of the link and the view that don't keep the connection open.
struct Follower {
    link: WeakCommander,
    view: WeakCommander,
    slot: Vec<ResourceIndex>,
}

impl Follower {
    async fn run(
        self,
        mut observer: Observer<PresenceUpdate, MountPresenceOwned>,
        tx: watch::Sender<MountPresenceOwned>,
    ) {
        // the link went down since the view last handled an `Up`: its streams wait for a resume
        let mut gap = false;
        loop {
            let event = tokio::select! {
                () = tx.closed() => return,
                event = observer.recv_event() => event,
            };
            let fresh = match event {
                Ok(ObserveEvent::Update(observed)) => Some(observed.value),
                Ok(ObserveEvent::Disconnected) => {
                    gap = true;
                    continue;
                }
                // subscribed again, or updates were lost: what is there now
                Ok(ObserveEvent::Resumed | ObserveEvent::Dropped { .. }) => None,
                Ok(ObserveEvent::Ended) | Err(_) => return,
            };
            let Some(parts) = self.parts() else {
                return;
            };
            let presence = match fresh {
                Some(presence) => presence,
                None => match parts.presence_read().read().await {
                    Ok(presence) => presence,
                    Err(e) => {
                        warn!("mount slot {:?}: presence not read: {e}", self.slot);
                        continue;
                    }
                },
            };
            let was_up = !gap && matches!(*tx.borrow(), MountPresenceOwned::Up { .. });
            if let Some(info) = parts.mounted_info(&presence) {
                if let Err(e) = learn_mounted(&parts.view, info).await {
                    warn!(
                        "mount slot {:?}: the mounted API is not usable: {e}",
                        self.slot
                    );
                    continue;
                }
                if !was_up {
                    match parts.view.resume_streams().await {
                        Ok(n) if n > 0 => debug!("mount slot {:?}: {n} resumed", self.slot),
                        Ok(_) => {}
                        Err(e) => warn!("mount slot {:?}: streams not resumed: {e}", self.slot),
                    }
                }
                gap = false;
            }
            tx.send_replace(presence);
        }
    }

    fn parts(&self) -> Option<Parts> {
        Some(Parts {
            link: self.link.upgrade()?,
            view: self.view.upgrade()?,
            slot: self.slot.clone(),
        })
    }
}

impl MountView {
    /// The view's Commander: what a generated client of the mounted API, or the dynamic client, is built on. Its
    /// paths are in the mounted API, [Commander::device_api_info] and [Commander::device_introspect] describe it.
    pub fn commander(&self) -> &Commander {
        &self.view
    }

    /// A generated client of `line` on this view, what its `on_mount(&view)` returns: refused unless the view was made
    /// for that client ([MountSlot::view_for]), so that its calls are checked against the mounted API.
    pub fn client<T: WwClient>(&self, line: &ClientLine) -> Result<T, Error> {
        match self.view.client_introspect() {
            Some(client) if client.api_hash.no_docs == line.api_hash.no_docs => {
                T::try_from_cmd(self.view.clone())
            }
            _ => Err(Error::User(format!(
                "a client of {:?} needs a view made for it: MountSlot::view_for(&Client::client_line())",
                line.version
            ))),
        }
    }

    /// Path of the slot's level, as the device names it.
    pub fn slot_path(&self) -> &[ResourceIndex] {
        &self.slot
    }

    /// Path of the mounted API's root, as the device names it: the prefix of every request of the view.
    pub fn api_path(&self) -> &[ResourceIndex] {
        self.view.prefix()
    }

    /// Whether the view learned a mounted API, now or before the backend went.
    pub fn has_learned(&self) -> bool {
        self.view.has_learned()
    }

    /// The slot's presence as the view handled it last: `Up` once the API mounted there is learned, `Absent` while
    /// the slot is empty. It doesn't change while the link is down: [Commander::connection_state] tells that.
    pub fn status(&self) -> watch::Receiver<MountPresenceOwned> {
        self.status.clone()
    }

    /// Reads the slot's `presence` and, when a backend is mounted, learns its API (CLT-81 per mount): the first time,
    /// or when its hashes changed since, as a reconnect learns a device's. On another API the view's kept streams,
    /// sinks and observers whose resource doesn't hold end, `const` values and keys are forgotten. The view does
    /// this itself whenever the slot's presence changes; returns the presence read.
    pub async fn learn(&self) -> Result<MountPresenceOwned, Error> {
        self.parts().learn().await
    }

    /// Blocking variant of [Self::learn], never to be called from an async task.
    pub fn learn_blocking(&self) -> Result<MountPresenceOwned, Error> {
        self.parts().learn_blocking()
    }

    fn parts(&self) -> Parts {
        Parts {
            link: self.link.clone(),
            view: self.view.clone(),
            slot: self.slot.clone(),
        }
    }
}

/// The `PropertyUpdate` of a slot's level: its `presence` when the update carries it.
#[derive(Debug, Default)]
pub(crate) struct PresenceUpdate(Option<MountPresenceOwned>);

impl DeserializeShrinkWrapOwned for PresenceUpdate {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, ShrinkWrapError> {
        let data = rd.read_raw_slice(rd.bytes_left())?;
        let mut slots = OptionAnyVec::new(data)?.iter();
        Ok(PresenceUpdate(slots.read_slot_owned(PRESENCE)?))
    }
}

impl LevelUpdate for PresenceUpdate {
    /// No values kept: the view keeps its status itself
    type Latest = ();
    type Config = DynConfig;

    fn update_latest(
        self,
        _latest: &mut (),
        _received: Instant,
        _t: Option<Second>,
        _sample_times: Option<&[Second]>,
    ) {
    }
}
