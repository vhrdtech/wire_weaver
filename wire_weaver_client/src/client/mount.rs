//! Mount slots (SRV-76): `mount!(hub)` and the elements of `mounts!(ext)`, levels whose API is known only at run
//! time. A [MountSlot] reads the slot's `presence` and names the path of the mounted API's root, which a view of the
//! mounted API prefixes its own paths with.

use crate::client::keyed::{as_ref, path_mut, to_owned};
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
