//! Who may do what (SRV-46): the server's side of `wire_weaver::authorize`, asked before any handler runs.

use std::sync::Arc;

use wire_weaver::CallContext;
use wire_weaver::authorize::{Authorize, Place, Resource, Step, Verdict};
use ww_client_server::target::Target;
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, Effects, keys};

/// The items `path` goes through in `api`, as the hook sees them ([Resource::items]): a keyed array's companion
/// (`index_of`, `key[]`, `keys`) is a step of its own, `read` like every one of its items. `None` when the path names
/// nothing in the model.
fn steps<'a>(api: &'a ApiBundleOwned, path: &[u32]) -> Option<Vec<Step<'a>>> {
    let mut steps = Vec::new();
    let mut level = &api.root;
    let mut rest = path;
    while let Some((id, after)) = rest.split_first() {
        let item = level.items.iter().find(|i| i.id.0 == *id)?;
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
            let (&op, after) = rest.split_first()?;
            let ident = keys::item_name(op)?;
            let (index, after) = match (op == keys::KEY, after.split_first()) {
                (true, Some((index, after))) => (Some(*index), after),
                _ => (None, after),
            };
            if !after.is_empty() {
                return None;
            }
            steps.push(Step::new(ident, op, index, Effects::READ, None));
            return Some(steps);
        }
        level = item.get_as_level(api).ok()?;
    }
    Some(steps)
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
    /// asking.
    pub(crate) fn admits(&self, call: &CallContext, request: &[u8]) -> Verdict {
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
        let items = match (&path, api) {
            (Some(path), Some(api)) => steps(api, path),
            _ => None,
        };
        self.hook.check(
            call,
            &Resource::new(
                access,
                &place,
                path.as_deref(),
                multi,
                items.as_deref(),
                api,
            ),
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
        assert!(!allows(allow_all.admits(&call, &[0xff, 0xff, 0xff])));
        assert!(!allows(allow_all.admits(&call, &[])));
        let cancel = request(absolute(&[]), RequestKindOwned::Cancel { seq: UVlq32(3) });
        assert!(!allows(allow_all.admits(&call, &cancel)));
        assert_eq!(*asked.lock().expect("not poisoned"), 0);
        assert!(allows(allow_all.admits(
            &call,
            &request(absolute(&[1]), RequestKindOwned::Read)
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
        assert!(!allows(
            auth.admits(&call, &request(global, RequestKindOwned::Introspect))
        ));
        assert!(!allows(auth.admits(
            &call,
            &request(absolute(&[4, 0]), RequestKindOwned::Read)
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
}
