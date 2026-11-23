//! Mount slots (SRV-76): `mount!(hub)` and the elements of `mounts!(ext)`, levels whose API is known only at run
//! time. A [MountSlot] reads the slot's `presence` and names the path of the mounted API's root, which a view of the
//! mounted API prefixes its own paths with.

use crate::client::keyed::{as_ref, path_mut, to_owned};
use crate::client::prepared_connection::learn_mounted;
use crate::device_info::DeviceApiInfo;
use crate::{Commander, Error, PreparedRead};
use wire_weaver::prelude::UNib32;
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
/// learned from the backend, never the device's. The link is the device's: a view lives as long as the link's
/// Commander, and its clones share what the view learned.
#[derive(Clone)]
pub struct MountView {
    /// The link's Commander: reads the slot's `presence`.
    link: Commander,
    /// The view's Commander: the slot's `api` in front of every path, the mounted API's facts.
    view: Commander,
    /// Path of the slot's level as the device names it.
    slot: Vec<ResourceIndex>,
}

impl MountSlot<'_> {
    /// A view of the API mounted at this slot, with that API learned when a backend is mounted now (see
    /// [MountView::learn]). An empty slot gives a view that learns at its first [MountView::learn] after a backend
    /// came.
    pub async fn view(&self) -> Result<MountView, Error> {
        let view = self.view_unlearned()?;
        view.learn().await?;
        Ok(view)
    }

    /// Blocking variant of [Self::view].
    pub fn view_blocking(&self) -> Result<MountView, Error> {
        let view = self.view_unlearned()?;
        view.learn_blocking()?;
        Ok(view)
    }

    fn view_unlearned(&self) -> Result<MountView, Error> {
        let (slot, _) = self.cmd.to_ww_client_server_path(as_ref(&self.slot))?;
        let PathKindOwned::Absolute { path: slot } = slot else {
            return Err(Error::User(format!(
                "a mount view needs the slot's absolute path, got {slot:?}"
            )));
        };
        let mut api = slot.clone();
        api.push(ResourceIndex::new(UNib32(API)));
        Ok(MountView {
            link: self.cmd.clone(),
            view: self.cmd.mount_view(api),
            slot,
        })
    }
}

impl MountView {
    /// The view's Commander: what a generated client of the mounted API, or the dynamic client, is built on. Its
    /// paths are in the mounted API, [Commander::device_api_info] and [Commander::device_introspect] describe it.
    pub fn commander(&self) -> &Commander {
        &self.view
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

    /// Reads the slot's `presence` and, when a backend is mounted, learns its API (CLT-81 per mount): the first time,
    /// or when its hashes changed since, as a reconnect learns a device's. On another API the view's kept streams,
    /// sinks and observers whose resource doesn't hold end, `const` values and keys are forgotten. Returns the
    /// presence read.
    pub async fn learn(&self) -> Result<MountPresenceOwned, Error> {
        let presence = self.presence_read().read().await?;
        if let Some(info) = self.mounted_info(&presence) {
            learn_mounted(&self.view, info).await?;
        }
        Ok(presence)
    }

    /// Blocking variant of [Self::learn], never to be called from an async task.
    pub fn learn_blocking(&self) -> Result<MountPresenceOwned, Error> {
        let presence = self.presence_read().blocking_read()?;
        if let Some(info) = self.mounted_info(&presence) {
            futures_lite::future::block_on(learn_mounted(&self.view, info))?;
        }
        Ok(presence)
    }

    fn presence_read(&self) -> PreparedRead<MountPresenceOwned> {
        let mut path = self.slot.clone();
        path.push(ResourceIndex::new(UNib32(PRESENCE)));
        self.link
            .prepare_read(as_ref(&PathKindOwned::Absolute { path }))
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
}
