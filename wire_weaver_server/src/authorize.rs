//! Who may do what (SRV-46): a hook the server asks before any handler runs.

use std::fmt;
use std::sync::Arc;

use wire_weaver::CallContext;
use ww_client_server::target::{Access, Place, Target, Unplaced};
use ww_self::{ApiBundleOwned, ApiItemOwned, ApiLevelOwned};

/// Decides whether a request may run: asked by the server for every request of every connection, before its
/// handler, whichever entry (shared or alone) serves it.
///
/// `call` is who asked as the transport saw it ([Peer](wire_weaver::Peer): a unix user, a key, an address), with the
/// request's seq, priority, trace and deadline. A request the hook denies is answered with `ErrorKind::Denied`
/// (`err_seq::REQUEST_DENIED`) and no handler runs; the client learns nothing else. A request the server can't place
/// (malformed, or of a kind from a newer version) is denied without asking.
///
/// A closure `Fn(&CallContext, &Resource) -> Result<(), Denied>` is one.
///
/// ```ignore
/// let only_reads_for_others = |call: &CallContext, resource: &Resource<'_>| match call.peer {
///     Peer::Unix { uid, .. } if uid == me => Ok(()),
///     _ if resource.access == Access::Read => Ok(()),
///     _ => Err(Denied),
/// };
/// let server = Server::new(clock).authorize(only_reads_for_others, Some(api));
/// ```
pub trait Authorize {
    /// Whether `call` may do what `resource` says.
    ///
    /// # Errors
    ///
    /// [Denied]: the request is answered `ErrorKind::Denied` and doesn't run.
    fn check(&self, call: &CallContext, resource: &Resource<'_>) -> Result<(), Denied>;
}

impl<F> Authorize for F
where
    F: Fn(&CallContext, &Resource<'_>) -> Result<(), Denied>,
{
    fn check(&self, call: &CallContext, resource: &Resource<'_>) -> Result<(), Denied> {
        self(call, resource)
    }
}

/// The answer of [Authorize::check] that refuses a request.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Denied;

impl fmt::Display for Denied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("denied by the server's policy")
    }
}

impl std::error::Error for Denied {}

/// What a request does where, as [Authorize::check] sees it.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Resource<'a> {
    /// What the request does.
    pub access: Access,
    /// Where it goes, as the request names it: absolute, or a trait wherever it sits.
    pub place: &'a Place,
    /// The path from the API's root: the request's own when absolute, a global one placed through the API's
    /// model; `None` for a global path without the model, or to a trait the model doesn't have.
    pub path: Option<&'a [u32]>,
    /// A multi request: it reaches the resources under [path](Self::path), not that one alone.
    pub multi: bool,
    /// The items [path](Self::path) goes through, from the root's down to the one it names, each with its array
    /// index: their `ww_self` metadata (`meta`: SRV-34's claims, labels, API-58's effects, which
    /// `ApiItemOwned::effects` answers for the last one, a read by kind included). Empty for the root (introspection
    /// of the whole API); `None` without the model or for a path that names nothing in it (the dispatcher answers
    /// that one with `BadPath` if it gets through).
    pub items: Option<&'a [(&'a ApiItemOwned, Option<u32>)]>,
    /// The API's model the server was given.
    pub api: Option<&'a ApiBundleOwned>,
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

    /// Whether the request in `request` may run for `call`.
    pub(crate) fn admits(&self, call: &CallContext, request: &[u8]) -> Result<(), Denied> {
        let Target {
            access,
            place,
            multi,
        } = ww_client_server::target::target(request).map_err(|_: Unplaced| Denied)?;
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
            (Some(path), Some(api)) => api.item_chain(path),
            _ => None,
        };
        self.hook.check(
            call,
            &Resource {
                access,
                place: &place,
                path: path.as_deref(),
                multi,
                items: items.as_deref(),
                api,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use wire_weaver::shrink_wrap::{SerializeShrinkWrapOwned, UNib32, UVlq32, UVlq32Backfill};
    use ww_client_server::target::TraitRef;
    use ww_client_server::{PathKindOwned, RequestKindOwned, RequestOwned, ResourceIndex};

    use super::*;

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
            move |_: &CallContext, _: &Resource<'_>| {
                *counter.lock().expect("not poisoned") += 1;
                Ok(())
            },
            None,
        );
        let call = CallContext::local();
        assert_eq!(allow_all.admits(&call, &[0xff, 0xff, 0xff]), Err(Denied));
        assert_eq!(allow_all.admits(&call, &[]), Err(Denied));
        let cancel = request(absolute(&[]), RequestKindOwned::Cancel { seq: UVlq32(3) });
        assert_eq!(allow_all.admits(&call, &cancel), Err(Denied));
        assert_eq!(*asked.lock().expect("not poisoned"), 0);
        assert_eq!(
            allow_all.admits(&call, &request(absolute(&[1]), RequestKindOwned::Read)),
            Ok(())
        );
        assert_eq!(*asked.lock().expect("not poisoned"), 1);
    }

    #[test]
    fn without_the_model_a_global_path_has_no_path_and_no_items() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let auth = Authorization::new(
            move |_: &CallContext, resource: &Resource<'_>| {
                record.lock().expect("not poisoned").push((
                    resource.access,
                    resource.place.clone(),
                    resource.path.map(<[u32]>::to_vec),
                    resource.items.is_some(),
                ));
                Err(Denied)
            },
            None,
        );
        let global = PathKindOwned::GlobalCompact {
            gid: ww_client_server::COMPACT_VERSION,
            path_from_trait: vec![ResourceIndex::new(UNib32(2))],
        };
        let compact = ww_client_server::COMPACT_VERSION.gid.id.0;
        let call = CallContext::local();
        assert_eq!(
            auth.admits(&call, &request(global, RequestKindOwned::Introspect)),
            Err(Denied)
        );
        assert_eq!(
            auth.admits(&call, &request(absolute(&[4, 0]), RequestKindOwned::Read)),
            Err(Denied)
        );
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
