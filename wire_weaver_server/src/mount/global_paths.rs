//! Global trait paths into mount slots (SRV-77).
//!
//! A server knows nothing of what is mounted in its slots but what the backends tell it: their introspection data,
//! learned when a slot comes `Up`. A global path (`PathKind::GlobalFull`, `GlobalCompact`, API-20) to a trait the
//! server's own tree doesn't have is looked up in the mounted APIs, and the request goes on with the front's
//! absolute path into the slot, which the generated dispatch routes to the slot's handler like any other.
//!
//! Order: the server's own tree first, then the slots in path order (depth first, in id order, an element of
//! `mounts!` by its index). Mounting a backend never changes where a trait of the server's own resolves. Inside a
//! mounted API, a trait is found as the backend's own server would find it ([ApiBundleOwned::trait_path]): the first
//! one depth first, never under a resource array, in a protocol compatible version of its crate.

use std::collections::BTreeMap;
use std::sync::Arc;

use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, UNib32};
use wire_weaver::ww_version::{FullVersionOwned, VersionOwned};
use ww_client_server::mount::{RewriteError, request_at};
use ww_client_server::{PathKind, Request, ResourceIndex};
use ww_self::{ApiBundleOwned, ApiLevelOwned};

/// The traits of the APIs mounted in a server's slots, for global paths into them.
#[derive(Debug, Clone)]
pub struct MountedTraits {
    /// The server's own API, whose traits come first.
    own: Arc<ApiBundleOwned>,
    /// The mounted APIs by the path of their slot's `api`: in path order, which is depth first in id order.
    slots: BTreeMap<Vec<u32>, Arc<ApiBundleOwned>>,
}

impl MountedTraits {
    /// The traits of a server whose own API is `own`, with nothing mounted yet.
    pub fn new(own: Arc<ApiBundleOwned>) -> Self {
        Self {
            own,
            slots: BTreeMap::new(),
        }
    }

    /// A backend came up in the slot whose `api` is at `api_path` and serves `api`, its learned introspection data
    /// (with the types it left out put back): it replaces what was mounted there.
    pub fn mount(&mut self, api_path: &[ResourceIndex], api: Arc<ApiBundleOwned>) {
        self.slots.insert(ids(api_path), api);
    }

    /// The slot whose `api` is at `api_path` has nothing mounted any more.
    pub fn unmount(&mut self, api_path: &[ResourceIndex]) {
        self.slots.remove(&ids(api_path));
    }

    /// The API learned at the slot whose `api` the front's absolute path `path` goes below, with that slot's path.
    pub(crate) fn mounted(&self, path: &[u32]) -> Option<(Vec<u32>, Arc<ApiBundleOwned>)> {
        self.slots
            .iter()
            .find(|(slot, _)| path.len() > slot.len() && path.starts_with(slot))
            .map(|(slot, api)| (slot.clone(), api.clone()))
    }

    /// The front's absolute path of the global path `path`, when it reaches into a mounted API: the slot's `api`,
    /// the trait's path in the backend, then the path from the trait. `None` for an absolute path, for a trait the
    /// server's own tree has (its dispatch or the client resolves it, as without mounts), and for one no mounted
    /// API has.
    pub fn resolve(&self, path: &PathKind<'_>) -> Option<Vec<ResourceIndex>> {
        let (wanted, from_trait) = Wanted::of(path)?;
        if self
            .own
            .trait_path(|level, api| wanted.matches(level, api))
            .is_some()
        {
            return None;
        }
        let (slot, found) = self.slots.iter().find_map(|(slot, api)| {
            api.trait_path(|level, api| wanted.matches(level, api))
                .map(|found| (slot, found))
        })?;
        let mut front: Vec<ResourceIndex> = slot
            .iter()
            .chain(found.iter())
            .map(|id| ResourceIndex::new(UNib32(*id)))
            .collect();
        for id in from_trait.iter() {
            front.push(id.ok()?);
        }
        Some(front)
    }

    /// The request in `request` with the front's absolute path into a slot instead of its global path, written into
    /// `buf` ([MountedTraits::resolve]); `None` when it stays as it is.
    ///
    /// # Errors
    ///
    /// [RewriteError::Unreadable] when `request` isn't a request, [RewriteError::TooBig] when the rewritten one
    /// doesn't fit `buf`.
    pub fn request_into_mount<'b>(
        &self,
        request: &[u8],
        buf: &'b mut [u8],
    ) -> Result<Option<&'b [u8]>, RewriteError> {
        let decoded = Request::from_ww_bytes(request).map_err(RewriteError::Unreadable)?;
        match self.resolve(&decoded.path_kind) {
            Some(path) => request_at(&decoded, &path, buf).map(Some),
            None => Ok(None),
        }
    }
}

fn ids(path: &[ResourceIndex]) -> Vec<u32> {
    path.iter().map(|id| id.get().0).collect()
}

/// The trait a global path asks for.
enum Wanted<'a> {
    /// By crate, version and trait name.
    Full {
        gid: FullVersionOwned,
        trait_name: &'a str,
    },
    /// By `ww_global` id and the version of its crate.
    Compact { id: u32, version: VersionOwned },
}

impl<'a> Wanted<'a> {
    /// The trait `path` asks for and the path from it; `None` for an absolute path.
    fn of(
        path: &PathKind<'a>,
    ) -> Option<(Self, wire_weaver::shrink_wrap::RefVec<'a, ResourceIndex>)> {
        match path {
            PathKind::Absolute { .. } => None,
            PathKind::GlobalCompact {
                gid,
                path_from_trait,
            } => Some((
                Wanted::Compact {
                    id: gid.gid.id.0,
                    version: VersionOwned::new(gid.major.0, gid.minor.0, gid.patch.0),
                },
                *path_from_trait,
            )),
            PathKind::GlobalFull {
                gid,
                trait_name,
                path_from_trait,
            } => Some((
                Wanted::Full {
                    gid: gid.make_owned(),
                    trait_name,
                },
                *path_from_trait,
            )),
        }
    }

    /// `level` of `api` is the trait asked for, in a protocol compatible version of its crate.
    fn matches(&self, level: &ApiLevelOwned, api: &ApiBundleOwned) -> bool {
        let Ok(have) = api.crate_version(level.crate_idx.0) else {
            return false;
        };
        match self {
            Wanted::Full { gid, trait_name } => {
                level.trait_name == *trait_name && have.is_protocol_compatible(gid)
            }
            Wanted::Compact { id, version } => {
                level.compact_id.map(|c| c.0) == Some(*id)
                    && have.is_protocol_compatible(&FullVersionOwned::new(
                        have.crate_id.clone(),
                        version.clone(),
                    ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use wire_weaver::shrink_wrap::{RefVec, SerializeShrinkWrapOwned, UVlq32Backfill};
    use wire_weaver::ww_version::{CompactVersion, FullVersion, GlobalTypeId, Version};
    use ww_client_server::{PathKindOwned, RequestKindOwned, RequestOwned, resource_path};
    use ww_self::{ApiItemKindOwned, ApiItemOwned, Multiplicity, TypeOwned};

    use super::*;

    fn item(id: u32, ident: &str, kind: ApiItemKindOwned) -> ApiItemOwned {
        ApiItemOwned {
            id: UNib32(id),
            kind,
            multiplicity: Multiplicity::Flat,
            since: None,
            ident: ident.to_owned(),
            docs: vec![],
            optional: None,
            meta: None,
            identity: None,
        }
    }

    fn level(
        crate_idx: u32,
        name: &str,
        compact: Option<u32>,
        items: Vec<ApiItemOwned>,
    ) -> ApiLevelOwned {
        ApiLevelOwned {
            docs: vec![],
            crate_idx: UNib32(crate_idx),
            trait_name: name.to_owned(),
            items,
            compact_id: compact.map(UNib32),
            meta: None,
        }
    }

    fn trait_item(id: u32, ident: &str, trait_idx: u32) -> ApiItemOwned {
        item(
            id,
            ident,
            ApiItemKindOwned::Trait {
                trait_idx: UNib32(trait_idx),
                omitted: None,
            },
        )
    }

    fn bundle(
        root: ApiLevelOwned,
        traits: Vec<ApiLevelOwned>,
        crates: &[(&str, u32)],
    ) -> Arc<ApiBundleOwned> {
        Arc::new(ApiBundleOwned {
            magic: ww_self::MAGIC,
            ww_self_version: ww_self::VERSION,
            root,
            types: vec![],
            traits: traits
                .into_iter()
                .map(|level| ww_self::ApiLevelLocationOwned::InLine {
                    crate_idx: level.crate_idx,
                    level,
                })
                .collect(),
            ext_crates: crates
                .iter()
                .map(|(id, minor)| {
                    FullVersionOwned::new((*id).to_owned(), VersionOwned::new(0, *minor, 1))
                })
                .collect(),
            root_omitted: None,
            wire: None,
            ext_wire: None,
        })
    }

    /// `Tv`: `mount!(hub)` at 0, `mounts!(ext)` at 1, `info: Info` at 2.
    fn tv() -> Arc<ApiBundleOwned> {
        let mount = || ApiItemKindOwned::Mount {
            expects: None,
            presence_ty: TypeOwned::OutOfLine {
                type_idx: UNib32(0),
            },
        };
        bundle(
            level(
                0,
                "Tv",
                None,
                vec![
                    item(0, "hub", mount()),
                    item(1, "ext", mount()),
                    trait_item(2, "info", 0),
                ],
            ),
            vec![level(1, "Info", None, vec![])],
            &[("tv_api", 1), ("info_api", 2)],
        )
    }

    /// `Hub`: `player: Player` at 3 (compact id 77), `info: Info` at 4.
    fn hub() -> Arc<ApiBundleOwned> {
        bundle(
            level(
                0,
                "Hub",
                None,
                vec![trait_item(3, "player", 0), trait_item(4, "info", 1)],
            ),
            vec![
                level(1, "Player", Some(77), vec![]),
                level(2, "Info", None, vec![]),
            ],
            &[("hub_api", 1), ("player_api", 3), ("info_api", 2)],
        )
    }

    fn full<'a>(
        crate_id: &'a str,
        minor: u32,
        name: &'a str,
        from_trait: &'a [ResourceIndex],
    ) -> PathKind<'a> {
        PathKind::GlobalFull {
            gid: FullVersion::new(crate_id, Version::new(0, minor, 0)),
            trait_name: name,
            path_from_trait: RefVec::Slice { slice: from_trait },
        }
    }

    fn resolved(traits: &MountedTraits, path: &PathKind<'_>) -> Option<Vec<u32>> {
        traits.resolve(path).map(|p| ids(&p))
    }

    const HUB: [ResourceIndex; 2] = resource_path([0, 1]);

    #[test]
    fn global_path_reaches_into_a_mount() {
        let from = resource_path([1]);
        let player = full("player_api", 3, "Player", &from);
        let mut traits = MountedTraits::new(tv());
        assert_eq!(resolved(&traits, &player), None, "nothing mounted");
        traits.mount(&HUB, hub());
        assert_eq!(resolved(&traits, &player), Some(vec![0, 1, 3, 1]));
        // another minor before 1.0 is another protocol
        assert_eq!(
            resolved(&traits, &full("player_api", 4, "Player", &from)),
            None
        );
        assert_eq!(
            resolved(&traits, &full("player_api", 3, "Missing", &from)),
            None
        );
        let compact = PathKind::GlobalCompact {
            gid: CompactVersion::new(GlobalTypeId { id: UNib32(77) }, 0, 3, 0),
            path_from_trait: RefVec::Slice { slice: &[] },
        };
        assert_eq!(resolved(&traits, &compact), Some(vec![0, 1, 3]));
        traits.unmount(&HUB);
        assert_eq!(resolved(&traits, &player), None, "unmounted");
        assert_eq!(resolved(&traits, &PathKind::absolute(&HUB)), None);
    }

    /// The server's own trait wins over a mounted one; among slots, the first in path order.
    #[test]
    fn own_tree_first_then_slots_in_path_order() {
        let mut traits = MountedTraits::new(tv());
        traits.mount(&resource_path([1, 2, 1]), hub());
        traits.mount(&HUB, hub());
        assert_eq!(resolved(&traits, &full("info_api", 2, "Info", &[])), None);
        assert_eq!(
            resolved(&traits, &full("player_api", 3, "Player", &[])),
            Some(vec![0, 1, 3])
        );
        traits.unmount(&HUB);
        assert_eq!(
            resolved(&traits, &full("player_api", 3, "Player", &[])),
            Some(vec![1, 2, 1, 3])
        );
    }

    #[test]
    fn request_gets_the_absolute_path_into_the_slot() {
        let request = |path_kind| {
            RequestOwned {
                seq: UVlq32Backfill(9),
                priority: wire_weaver::Priority::NORMAL.to_wire(),
                context: None,
                path_kind,
                kind: RequestKindOwned::Read,
            }
            .to_ww_bytes_owned()
            .expect("a request serializes")
        };
        let mut traits = MountedTraits::new(tv());
        traits.mount(&HUB, hub());
        let global = request(PathKindOwned::GlobalFull {
            gid: FullVersionOwned::new("player_api".to_owned(), VersionOwned::new(0, 3, 0)),
            trait_name: "Player".to_owned(),
            path_from_trait: resource_path([2]).to_vec(),
        });
        let mut buf = [0u8; 64];
        let bytes = traits
            .request_into_mount(&global, &mut buf)
            .expect("rewritten")
            .expect("into the hub");
        let back = Request::from_ww_bytes(bytes).expect("a request");
        assert_eq!(back.seq.0, 9);
        let PathKind::Absolute { path } = &back.path_kind else {
            panic!("an absolute path");
        };
        assert_eq!(
            path.iter()
                .map(|id| id.unwrap().get().0)
                .collect::<Vec<_>>(),
            [0, 1, 3, 2]
        );
        let absolute = request(PathKindOwned::Absolute { path: HUB.to_vec() });
        assert_eq!(traits.request_into_mount(&absolute, &mut buf), Ok(None));
        assert!(traits.request_into_mount(&[0xff], &mut buf).is_err());
    }
}
