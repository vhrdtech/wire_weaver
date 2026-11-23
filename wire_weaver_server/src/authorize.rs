//! Who may do what (SRV-46): the server's side of `wire_weaver::authorize`, asked before any handler runs.

use std::sync::Arc;

use wire_weaver::CallContext;
use wire_weaver::authorize::{Authorize, Mounted, Place, Resource, Step, Verdict};
use ww_client_server::target::Target;
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, Effects, keys, mount};

use crate::mount::MountRoutes;

/// What a walk of a request's path through the model finds.
struct Walk<'a> {
    /// The items on the way ([Resource::items]); `None` when the path names nothing in the model, or goes below a
    /// mount slot whose API isn't learned.
    items: Option<Vec<Step<'a>>>,
    /// The length of the path to the `api` of the mount slot it goes below.
    mount_at: Option<usize>,
    /// The mounted API the items below the slot are from.
    mounted: Option<&'a ApiBundleOwned>,
}

/// The items `path` goes through in `api`, as the hook sees them ([Resource::items]): a keyed array's companion
/// (`index_of`, `key[]`, `keys`) is a step of its own, `read` like every one of its items, and so is a mount slot's
/// `presence` or `api`. Below a slot's `api` the walk goes on in `mounted`, the API learned from the backend at the
/// slot whose `api` is at that path, with the effects it declares (SRV-79); without it the items are `None`.
fn walk<'a>(
    api: &'a ApiBundleOwned,
    path: &[u32],
    mounted: Option<(&[u32], &'a ApiBundleOwned)>,
) -> Walk<'a> {
    let mut walk = Walk {
        items: None,
        mount_at: None,
        mounted: None,
    };
    let mut steps = Vec::new();
    let mut bundle = api;
    let mut level = &api.root;
    let mut rest = path;
    while let Some((id, after)) = rest.split_first() {
        let Some(item) = level.items.iter().find(|i| i.id.0 == *id) else {
            return walk;
        };
        let (index, after) = match (item.is_array(), after.split_first()) {
            (true, Some((index, after))) => (Some(*index), after),
            _ => (None, after),
        };
        steps.push(Step::new(
            &item.ident,
            item.id.0,
            index,
            item.effects(),
            item.meta.as_ref(),
        ));
        rest = after;
        if rest.is_empty() {
            break;
        }
        if let ApiItemKindOwned::Keys { .. } = item.kind {
            walk.items = keys_step(rest).map(|step| {
                steps.push(step);
                steps
            });
            return walk;
        }
        if let ApiItemKindOwned::Mount { .. } = item.kind {
            let Some((&op, after)) = rest.split_first() else {
                return walk;
            };
            let Some(ident) = mount::item_name(op) else {
                return walk;
            };
            if op == mount::PRESENCE {
                if after.is_empty() {
                    steps.push(Step::new(ident, op, None, Effects::READ, None));
                    walk.items = Some(steps);
                }
                return walk;
            }
            // the slot's `api`: the root of the mounted API, a level that does nothing by itself
            steps.push(Step::new(ident, op, None, Effects::NONE, None));
            rest = after;
            if rest.is_empty() {
                break;
            }
            if walk.mount_at.is_some() {
                // a slot of the mounted API: what is below it is only its own backend's to know
                return walk;
            }
            let at = path.len() - rest.len();
            walk.mount_at = Some(at);
            match mounted {
                Some((api_path, mounted)) if api_path == &path[..at] => {
                    walk.mounted = Some(mounted);
                    bundle = mounted;
                    level = &mounted.root;
                    continue;
                }
                _ => return walk,
            }
        }
        let Ok(next) = item.get_as_level(bundle) else {
            return walk;
        };
        level = next;
    }
    walk.items = Some(steps);
    walk
}

/// The step of a keyed array's companion at `rest`, the path below the array.
fn keys_step<'a>(rest: &[u32]) -> Option<Step<'a>> {
    let (&op, after) = rest.split_first()?;
    let ident = keys::item_name(op)?;
    let (index, after) = match (op == keys::KEY, after.split_first()) {
        (true, Some((index, after))) => (Some(*index), after),
        _ => (None, after),
    };
    after
        .is_empty()
        .then(|| Step::new(ident, op, index, Effects::READ, None))
}

/// The hook and the API's model, as a server keeps them.
pub(crate) struct Authorization {
    hook: Box<dyn Authorize + Send + Sync>,
    api: Option<Arc<ApiBundleOwned>>,
}

impl Authorization {
    pub(crate) fn new(
        hook: impl Authorize + Send + Sync + 'static,
        api: Option<Arc<ApiBundleOwned>>,
    ) -> Self {
        Authorization {
            hook: Box::new(hook),
            api,
        }
    }

    /// Whether the request in `request` may run for `call`: a request that can't be placed is denied without
    /// asking. `routes` has the APIs learned from the backends of the server's mount slots.
    pub(crate) fn admits(
        &self,
        call: &CallContext,
        request: &[u8],
        routes: Option<&MountRoutes>,
    ) -> Verdict {
        let Ok(Target {
            access,
            place,
            multi,
        }) = ww_client_server::target::target(request)
        else {
            return Verdict::Deny;
        };
        let api = self.api.as_deref();
        let path = match (&place, api) {
            (Place::Absolute(path), _) => Some(path.clone()),
            (Place::Trait(trait_ref, from_trait), Some(api)) => api
                .trait_path(|level: &ApiLevelOwned, bundle| {
                    trait_ref.matches(
                        level.crate_name(bundle).ok(),
                        &level.trait_name,
                        level.compact_id.map(|c| c.0),
                    )
                })
                .map(|mut path| {
                    path.extend_from_slice(from_trait);
                    path
                }),
            (Place::Trait(..), None) => None,
        };
        // the API learned at the slot the path goes into: held here, for the steps that borrow from it
        let learned = match (&path, routes) {
            (Some(path), Some(routes)) => routes.mounted(path),
            _ => None,
        };
        let walked = match (&path, api) {
            (Some(path), Some(api)) => Some(walk(
                api,
                path,
                learned
                    .as_ref()
                    .map(|(api_path, api)| (api_path.as_slice(), &**api)),
            )),
            _ => None,
        };
        let mount = match (&path, &walked) {
            (Some(path), Some(walked)) => walked
                .mount_at
                .map(|at| Mounted::new(&path[..at], walked.mounted)),
            _ => None,
        };
        let items = walked.as_ref().and_then(|walked| walked.items.as_deref());
        self.hook.check(
            call,
            &Resource::new(access, &place, path.as_deref(), multi, items, api).with_mount(mount),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use wire_weaver::shrink_wrap::{SerializeShrinkWrapOwned, UNib32, UVlq32, UVlq32Backfill};
    use ww_client_server::target::TraitRef;
    use ww_client_server::{PathKindOwned, RequestKindOwned, RequestOwned, ResourceIndex};

    use wire_weaver::authorize::{Access, Denied, from_fn};

    use super::*;

    /// Whether a verdict that isn't pending allows.
    fn allows(verdict: Verdict) -> bool {
        match verdict {
            Verdict::Allow => true,
            Verdict::Deny => false,
            Verdict::Pending(_) => panic!("a policy answers at once"),
        }
    }

    fn request(path_kind: PathKindOwned, kind: RequestKindOwned) -> Vec<u8> {
        RequestOwned {
            seq: UVlq32Backfill(1),
            priority: wire_weaver::Priority::NORMAL.to_wire(),
            context: None,
            path_kind,
            kind,
        }
        .to_ww_bytes_owned()
        .expect("a request serializes")
    }

    fn absolute(path: &[u32]) -> PathKindOwned {
        PathKindOwned::Absolute {
            path: path
                .iter()
                .map(|n| ResourceIndex::new(UNib32(*n)))
                .collect(),
        }
    }

    #[test]
    fn requests_that_cant_be_placed_are_denied_without_asking() {
        let asked = Arc::new(Mutex::new(0));
        let counter = asked.clone();
        let allow_all = Authorization::new(
            from_fn(move |_: &CallContext, _: &Resource<'_>| {
                *counter.lock().expect("not poisoned") += 1;
                Ok(())
            }),
            None,
        );
        let call = CallContext::local();
        assert!(!allows(allow_all.admits(&call, &[0xff, 0xff, 0xff], None)));
        assert!(!allows(allow_all.admits(&call, &[], None)));
        let cancel = request(absolute(&[]), RequestKindOwned::Cancel { seq: UVlq32(3) });
        assert!(!allows(allow_all.admits(&call, &cancel, None)));
        assert_eq!(*asked.lock().expect("not poisoned"), 0);
        assert!(allows(allow_all.admits(
            &call,
            &request(absolute(&[1]), RequestKindOwned::Read),
            None
        )));
        assert_eq!(*asked.lock().expect("not poisoned"), 1);
    }

    #[test]
    fn without_the_model_a_global_path_has_no_path_and_no_items() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let auth = Authorization::new(
            from_fn(move |_: &CallContext, resource: &Resource<'_>| {
                record.lock().expect("not poisoned").push((
                    resource.access,
                    resource.place.clone(),
                    resource.path.map(<[u32]>::to_vec),
                    resource.items.is_some(),
                ));
                Err(Denied)
            }),
            None,
        );
        let global = PathKindOwned::GlobalCompact {
            gid: ww_client_server::COMPACT_VERSION,
            path_from_trait: vec![ResourceIndex::new(UNib32(2))],
        };
        let compact = ww_client_server::COMPACT_VERSION.gid.id.0;
        let call = CallContext::local();
        assert!(!allows(auth.admits(
            &call,
            &request(global, RequestKindOwned::Introspect),
            None
        )));
        assert!(!allows(auth.admits(
            &call,
            &request(absolute(&[4, 0]), RequestKindOwned::Read),
            None
        )));
        assert_eq!(
            *seen.lock().expect("not poisoned"),
            [
                (
                    Access::Introspect,
                    Place::Trait(TraitRef::Compact(compact), vec![2]),
                    None,
                    false
                ),
                (
                    Access::Read,
                    Place::Absolute(vec![4, 0]),
                    Some(vec![4, 0]),
                    false
                ),
            ]
        );
    }

    fn item(
        id: u32,
        ident: &str,
        kind: ApiItemKindOwned,
        meta: Option<ww_self::MetaOwned>,
    ) -> ww_self::ApiItemOwned {
        ww_self::ApiItemOwned {
            id: UNib32(id),
            kind,
            multiplicity: ww_self::Multiplicity::Flat,
            since: None,
            ident: ident.to_owned(),
            docs: vec![],
            optional: None,
            meta,
            identity: None,
        }
    }

    fn slot() -> ApiItemKindOwned {
        ApiItemKindOwned::Mount {
            expects: None,
            presence_ty: ww_self::TypeOwned::OutOfLine {
                type_idx: UNib32(0),
            },
        }
    }

    fn bundle(trait_name: &str, items: Vec<ww_self::ApiItemOwned>) -> ApiBundleOwned {
        ApiBundleOwned {
            magic: ww_self::MAGIC,
            ww_self_version: ww_self::VERSION,
            root: ApiLevelOwned {
                docs: vec![],
                crate_idx: UNib32(0),
                trait_name: trait_name.to_owned(),
                items,
                compact_id: None,
                meta: None,
            },
            types: vec![],
            traits: vec![],
            ext_crates: vec![],
            root_omitted: None,
            wire: None,
            ext_wire: None,
        }
    }

    /// A bundle whose root has the slot `hub` (id 0).
    fn tv() -> ApiBundleOwned {
        bundle("Tv", vec![item(0, "hub", slot(), None)])
    }

    /// The API a hub backend serves: `add` (id 0) edits, `inner` (id 1) is a slot of its own.
    fn hub() -> ApiBundleOwned {
        let add = ApiItemKindOwned::Method {
            args: vec![],
            return_ty: None,
            errors: None,
        };
        let edits = ww_self::MetaOwned {
            effects: Effects::EDIT,
            ..Default::default()
        };
        bundle(
            "Hub",
            vec![
                item(0, "add", add, Some(edits)),
                item(1, "inner", slot(), None),
            ],
        )
    }

    fn steps<'a>(api: &'a ApiBundleOwned, path: &[u32]) -> Option<Vec<Step<'a>>> {
        walk(api, path, None).items
    }

    fn idents_and_effects(steps: &[Step<'_>]) -> Vec<(String, Effects)> {
        steps
            .iter()
            .map(|s| (s.ident.to_owned(), s.effects))
            .collect()
    }

    /// A slot's `presence` is a read, its `api` a step with no effects; a path below `presence` or to an id the slot
    /// has not names nothing.
    #[test]
    fn mount_slot_steps() {
        let api = tv();
        let presence = steps(&api, &[0, mount::PRESENCE]).expect("presence is in the model");
        assert_eq!(
            idents_and_effects(&presence)[1..],
            [("presence".to_owned(), Effects::READ)]
        );
        let below = steps(&api, &[0, mount::API]).expect("api is a step");
        assert_eq!(
            idents_and_effects(&below)[1..],
            [("api".to_owned(), Effects::NONE)]
        );
        assert!(steps(&api, &[0, mount::PRESENCE, 1]).is_none());
        assert!(steps(&api, &[0, 2]).is_none());
    }

    /// Below a slot's `api` the steps are the mounted API's, with the effects its backend declares (SRV-79); without
    /// a learned API nothing is known of them, and the request still says it goes into the slot.
    #[test]
    fn steps_go_on_in_the_mounted_api() {
        let (tv, hub) = (tv(), hub());
        let slot = [0, mount::API];
        let walked = walk(&tv, &[0, mount::API, 0], Some((&slot, &hub)));
        assert_eq!(walked.mount_at, Some(2));
        assert!(walked.mounted.is_some());
        assert_eq!(
            idents_and_effects(&walked.items.expect("in the hub's model")),
            [
                ("hub".to_owned(), Effects::NONE),
                ("api".to_owned(), Effects::NONE),
                ("add".to_owned(), Effects::EDIT),
            ]
        );
        let unknown = walk(&tv, &[0, mount::API, 0], None);
        assert_eq!(unknown.mount_at, Some(2));
        assert!(unknown.items.is_none() && unknown.mounted.is_none());
        // the API learned at another slot is not this one's
        let other = [3, mount::API];
        assert!(
            walk(&tv, &[0, mount::API, 0], Some((&other, &hub)))
                .items
                .is_none()
        );
        // an id the hub hasn't names nothing
        assert!(
            walk(&tv, &[0, mount::API, 7], Some((&slot, &hub)))
                .items
                .is_none()
        );
        // a slot of the mounted API: its own `api` is a step, below it is its own backend's
        let inner = walk(&tv, &[0, mount::API, 1, mount::API], Some((&slot, &hub)));
        assert_eq!(inner.items.expect("the inner slot's api").len(), 4);
        let below_inner = walk(&tv, &[0, mount::API, 1, mount::API, 0], Some((&slot, &hub)));
        assert!(below_inner.items.is_none());
        assert_eq!(below_inner.mount_at, Some(2));
    }

    /// The hook sees a mounted call's effects and the slot it goes into, from the API the routes learned.
    #[test]
    fn the_hook_sees_into_a_mount() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let auth = Authorization::new(
            from_fn(move |_: &CallContext, resource: &Resource<'_>| {
                let mount = resource
                    .mount
                    .map(|m| (m.api_path.to_vec(), m.api.is_some()));
                record.lock().expect("not poisoned").push((
                    resource.effects(),
                    resource.items.is_some(),
                    mount,
                ));
                if resource.effects().contains(Effects::EDIT) {
                    Err(Denied)
                } else {
                    Ok(())
                }
            }),
            Some(Arc::new(tv())),
        );
        let routes = MountRoutes::new(Arc::new(tv()));
        let call = CallContext::local();
        let add = request(absolute(&[0, mount::API, 0]), RequestKindOwned::Read);
        // nothing learned yet: the hook sees the slot, and no items
        assert!(allows(auth.admits(&call, &add, Some(&routes))));
        routes.learned(&[0, mount::API], Arc::new(hub()));
        assert!(!allows(auth.admits(&call, &add, Some(&routes))));
        assert_eq!(
            *seen.lock().expect("not poisoned"),
            [
                (Effects::NONE, false, Some((vec![0, mount::API], false))),
                (Effects::EDIT, true, Some((vec![0, mount::API], true))),
            ]
        );
    }
}
