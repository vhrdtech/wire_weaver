//! Global trait paths into modules served in process (SRV-82): the `no_std` counterpart of
//! `wire_weaver_server::mount::MountedTraits`, with no heap and no owned model.
//!
//! A global path (`PathKind::GlobalFull`, `GlobalCompact`, API-20) asks for a trait by its crate and name, or by its
//! `ww_global` id. The firmware's own tree answers first (the generated `has_global_trait`), then the modules loaded in
//! its slots, in path order: each module's model, the bytes its slot validated at load, is walked in place
//! ([ww_self::ApiBundle::find_trait]). The request goes on with the absolute path into the slot, which the generated
//! dispatch routes to the slot's handler like any other.

use shrink_wrap::UNib32;
use shrink_wrap::prelude::DeserializeShrinkWrap;
use wire_weaver::ww_version::Version;
use ww_client_server::mount::{PathArray, PathStore, request_at};
use ww_client_server::{PathKind, Request, ResourceIndex};
use ww_self::{ApiBundle, TraitWalk, WantedTrait};

use super::{Module, ModuleSlot, PATH_IDS_MAX};

/// The modules loaded in a firmware's slots, for global trait paths into them: up to `N` slots, each the path of its
/// `api` and its module's model, kept in path order. Built for one request from the server's slots, in the backend's
/// [global_into_mount](wire_weaver::WireWeaverAsyncApiBackend::global_into_mount):
///
/// ```ignore
/// fn global_into_mount<'b>(&self, request: &[u8], buf: &'b mut [u8]) -> Option<&'b [u8]> {
///     ModuleTraits::<1>::new()
///         .slot(&api_impl::HUB_API_PATH, &self.hub)
///         .request_into_mount(request, api_impl::has_global_trait, buf)
/// }
/// ```
pub struct ModuleTraits<'a, const N: usize> {
    slots: [(&'a [ResourceIndex], &'a [u8]); N],
    len: usize,
}

impl<const N: usize> Default for ModuleTraits<'_, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, const N: usize> ModuleTraits<'a, N> {
    /// No slots.
    pub const fn new() -> Self {
        ModuleTraits {
            slots: [(&[], &[]); N],
            len: 0,
        }
    }

    /// The slot whose `api` is at `api_path` (the generated `HUB_API_PATH`, `ext_api_path(index_chain)`), with the
    /// module loaded in it; nothing while it is empty.
    ///
    /// # Panics
    ///
    /// When `N` slots with a module are in already.
    pub fn slot<M: Module, const MSG: usize, const OUT: usize>(
        &mut self,
        api_path: &'a [ResourceIndex],
        slot: &'a ModuleSlot<M, MSG, OUT>,
    ) -> &mut Self {
        match slot.model() {
            Some(model) => self.model(api_path, model),
            None => self,
        }
    }

    /// A module's `model` (see [Module::model]) mounted at the slot whose `api` is at `api_path`.
    ///
    /// # Panics
    ///
    /// When `N` slots are in already.
    pub fn model(&mut self, api_path: &'a [ResourceIndex], model: &'a [u8]) -> &mut Self {
        assert!(self.len < N, "ModuleTraits holds at most N slots");
        // path order: depth first, in id order
        let at = self.slots[..self.len]
            .iter()
            .position(|(other, _)| path_ids(other).gt(path_ids(api_path)))
            .unwrap_or(self.len);
        self.slots.copy_within(at..self.len, at + 1);
        self.slots[at] = (api_path, model);
        self.len += 1;
        self
    }

    /// The front's absolute path of the global path `path` when it reaches into a module: the slot's `api`, the
    /// trait's path in the module, then the path from the trait. `None` for an absolute path, for a trait `own`
    /// says the server's own tree has (dispatch answers it as without modules), for one no module has, and for a path
    /// past [PATH_IDS_MAX] ids.
    pub fn resolve(
        &self,
        path: &PathKind<'_>,
        own: impl Fn(&PathKind<'_>) -> bool,
    ) -> Option<PathArray<PATH_IDS_MAX>> {
        let (wanted, from_trait) = match path {
            PathKind::Absolute { .. } => return None,
            PathKind::GlobalFull {
                gid,
                trait_name,
                path_from_trait,
            } => (
                WantedTrait::Full {
                    crate_version: *gid,
                    trait_name,
                },
                path_from_trait,
            ),
            PathKind::GlobalCompact {
                gid,
                path_from_trait,
            } => (
                WantedTrait::Compact {
                    id: gid.gid.id.0,
                    version: Version::new(gid.major.0, gid.minor.0, gid.patch.0),
                },
                path_from_trait,
            ),
        };
        if own(path) {
            return None;
        }
        self.slots[..self.len].iter().find_map(|(api_path, model)| {
            let bundle = ApiBundle::from_ww_bytes(model).ok()?;
            let mut ids = [0u32; PATH_IDS_MAX];
            let room = PATH_IDS_MAX.checked_sub(api_path.len())?;
            let TraitWalk::Found(n) = bundle.find_trait(&wanted, &mut ids[..room]) else {
                return None;
            };
            let mut front = PathArray::new();
            for id in api_path.iter().copied() {
                front.push(id).ok()?;
            }
            for id in &ids[..n] {
                front.push(ResourceIndex::new(UNib32(*id))).ok()?;
            }
            for id in from_trait.iter() {
                front.push(id.ok()?).ok()?;
            }
            Some(front)
        })
    }

    /// The request in `request` with the front's absolute path into a slot in place of its global path, written into
    /// `buf` ([resolve](Self::resolve)); `None` when it stays as it is: not a request, not into a module, or longer
    /// than `buf`.
    pub fn request_into_mount<'b>(
        &self,
        request: &[u8],
        own: impl Fn(&PathKind<'_>) -> bool,
        buf: &'b mut [u8],
    ) -> Option<&'b [u8]> {
        if self.len == 0 {
            return None;
        }
        let decoded = Request::from_ww_bytes(request).ok()?;
        let path = self.resolve(&decoded.path_kind, own)?;
        request_at(&decoded, PathStore::as_slice(&path), buf).ok()
    }
}

fn path_ids(path: &[ResourceIndex]) -> impl Iterator<Item = u32> + '_ {
    path.iter().map(|id| id.get().0)
}
