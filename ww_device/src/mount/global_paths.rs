//! Global trait paths into modules served in process (SRV-82): the `no_std` counterpart of
//! `wire_weaver_server::mount::MountedTraits`, with no heap and no owned model.
//!
//! A global path (`PathKind::GlobalFull`, `GlobalCompact`, API-20) asks for a trait by its crate and name, or by its
//! `ww_global` id. The firmware's own tree answers first (the generated `has_global_trait`), then the modules loaded in
//! its slots, in path order. A slot walks its module's model once, at load, into a [TraitTable]: reaching an item of a
//! shrink_wrap model decodes everything before it, so a walk per request would cost a model decode each time. The
//! request goes on with the absolute path into the slot, which the generated dispatch routes to the slot's handler
//! like any other.

use core::ops::ControlFlow;

use shrink_wrap::UNib32;
use shrink_wrap::prelude::DeserializeShrinkWrap;
use ww_client_server::dispatch::version_compatible;
use ww_client_server::mount::{PathArray, PathStore, request_at};
use ww_client_server::{PathKind, Request, ResourceIndex};
use ww_self::{ApiBundle, TraitAt, TraitWalk};

use super::{LoadError, Module, ModuleSlot, PATH_IDS_MAX};

/// Traits of a module a global path can reach, each crate and name once: more refuse the module's load.
pub const MODULE_TRAITS_MAX: usize = 16;
/// Ids of the paths of a module's traits, all together: more refuse the module's load.
pub const MODULE_TRAIT_IDS_MAX: usize = 48;
/// Levels below a module's root its traits may sit at: the slot's `api` takes at least two ids of [PATH_IDS_MAX].
pub const MODULE_TRAIT_DEPTH_MAX: usize = PATH_IDS_MAX - 2;

/// A trait of a module, as a global path finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModuleTrait {
    /// `ww_global::trait_hash` of its crate and name.
    hash: u32,
    /// The version of its crate.
    major: u32,
    minor: u32,
    /// Its `ww_global` id.
    compact_id: Option<u32>,
    /// Where its path lies in [TraitTable::ids].
    path_start: u8,
    path_len: u8,
}

impl ModuleTrait {
    const EMPTY: ModuleTrait = ModuleTrait {
        hash: 0,
        major: 0,
        minor: 0,
        compact_id: None,
        path_start: 0,
        path_len: 0,
    };
}

/// The traits of a loaded module a global path can reach, read from its model at load: what the generated
/// `WW_GLOBAL_TRAITS` table is for a firmware's own traits. No names, a hash of each crate and trait name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraitTable {
    traits: [ModuleTrait; MODULE_TRAITS_MAX],
    len: u8,
    ids: [ResourceIndex; MODULE_TRAIT_IDS_MAX],
    ids_len: u8,
}

const _: () = assert!(
    MODULE_TRAITS_MAX <= u8::MAX as usize,
    "the table's length is a u8"
);
const _: () = assert!(
    MODULE_TRAIT_IDS_MAX <= u8::MAX as usize,
    "offsets into the ids are u8"
);

impl TraitTable {
    pub(crate) const EMPTY: TraitTable = TraitTable {
        traits: [ModuleTrait::EMPTY; MODULE_TRAITS_MAX],
        len: 0,
        ids: [ResourceIndex::new(UNib32(0)); MODULE_TRAIT_IDS_MAX],
        ids_len: 0,
    };

    /// The traits of `model`, depth first: a trait's first place is the one its global paths reach.
    ///
    /// # Errors
    ///
    /// [LoadError::Model] when the model doesn't decode, [LoadError::TooManyTraits] past [MODULE_TRAITS_MAX] traits or
    /// [MODULE_TRAIT_IDS_MAX] ids, [LoadError::TraitsTooDeep] past `ww_self::TRAIT_WALK_ITEMS_MAX` trait items or
    /// [MODULE_TRAIT_DEPTH_MAX] levels.
    pub(crate) fn of(model: &ApiBundle<'_>) -> Result<TraitTable, LoadError> {
        let mut table = TraitTable::EMPTY;
        let mut error = None;
        let mut path = [0u32; MODULE_TRAIT_DEPTH_MAX];
        let walk = model.visit_traits(&mut path, |at| match table.push(at) {
            Ok(()) => ControlFlow::Continue(()),
            Err(e) => {
                error = Some(e);
                ControlFlow::Break(())
            }
        });
        match (walk, error) {
            (_, Some(e)) => Err(e),
            (TraitWalk::Done, None) => Ok(table),
            (TraitWalk::Limit, None) => Err(LoadError::TraitsTooDeep),
            (TraitWalk::Malformed | TraitWalk::Stopped, None) => Err(LoadError::Model),
        }
    }

    fn push(&mut self, at: &TraitAt<'_, '_>) -> Result<(), LoadError> {
        let hash = ww_global::trait_hash(at.crate_version.crate_id, at.trait_name);
        if self.traits().iter().any(|t| t.hash == hash) {
            return Ok(());
        }
        let start = usize::from(self.ids_len);
        let slot = self
            .traits
            .get_mut(usize::from(self.len))
            .ok_or(LoadError::TooManyTraits)?;
        let ids = self
            .ids
            .get_mut(start..start + at.path.len())
            .ok_or(LoadError::TooManyTraits)?;
        for (to, id) in ids.iter_mut().zip(at.path) {
            *to = ResourceIndex::new(UNib32(*id));
        }
        let path_len = u8::try_from(at.path.len()).expect("bounded by MODULE_TRAIT_IDS_MAX");
        let version = &at.crate_version.version;
        *slot = ModuleTrait {
            hash,
            major: version.major.0,
            minor: version.minor.0,
            compact_id: at.compact_id,
            path_start: self.ids_len,
            path_len,
        };
        self.len += 1;
        self.ids_len += path_len;
        Ok(())
    }

    fn traits(&self) -> &[ModuleTrait] {
        &self.traits[..usize::from(self.len)]
    }

    /// The path in the module of the trait `path` names; `None` for an absolute path and a trait it doesn't have.
    fn find(&self, path: &PathKind<'_>) -> Option<&[ResourceIndex]> {
        let found = match path {
            PathKind::Absolute { .. } => return None,
            PathKind::GlobalFull {
                gid, trait_name, ..
            } => {
                let hash = ww_global::trait_hash(gid.crate_id, trait_name);
                let (major, minor) = (gid.version.major.0, gid.version.minor.0);
                self.traits()
                    .iter()
                    .find(|t| t.hash == hash && version_compatible(t.major, t.minor, major, minor))
            }
            PathKind::GlobalCompact { gid, .. } => self.traits().iter().find(|t| {
                t.compact_id == Some(gid.gid.id.0)
                    && version_compatible(t.major, t.minor, gid.major.0, gid.minor.0)
            }),
        }?;
        let start = usize::from(found.path_start);
        self.ids.get(start..start + usize::from(found.path_len))
    }
}

/// The modules loaded in a firmware's slots, for global trait paths into them: up to `N` slots, each the path of its
/// `api` and its module's traits, kept in path order. Built for one request from the server's slots, in the backend's
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
    slots: [(&'a [ResourceIndex], &'a TraitTable); N],
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
            slots: [(&[], &TraitTable::EMPTY); N],
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
        if !slot.is_loaded() {
            return self;
        }
        assert!(self.len < N, "ModuleTraits holds at most N slots");
        // path order: depth first, in id order
        let at = self.slots[..self.len]
            .iter()
            .position(|(other, _)| path_ids(other).gt(path_ids(api_path)))
            .unwrap_or(self.len);
        self.slots.copy_within(at..self.len, at + 1);
        self.slots[at] = (api_path, slot.traits());
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
        let from_trait = match path {
            PathKind::Absolute { .. } => return None,
            PathKind::GlobalFull {
                path_from_trait, ..
            }
            | PathKind::GlobalCompact {
                path_from_trait, ..
            } => path_from_trait,
        };
        if own(path) {
            return None;
        }
        let (api_path, in_module) = self.slots[..self.len]
            .iter()
            .find_map(|(api_path, traits)| Some((api_path, traits.find(path)?)))?;
        let mut front = PathArray::new();
        for id in api_path.iter().chain(in_module) {
            front.push(*id).ok()?;
        }
        for id in from_trait.iter() {
            front.push(id.ok()?).ok()?;
        }
        Some(front)
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
        // most requests have an absolute path: a peek, not a decode
        if self.len == 0 || !Request::peek_is_global(request).ok()? {
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
