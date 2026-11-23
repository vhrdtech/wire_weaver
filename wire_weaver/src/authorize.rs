//! Who may do what (SRV-46, SRV-72): one hook, [Authorize], asked before a request of a remote client runs
//! (`wire_weaver_server::Server::authorize`) and before a call in the same process reaches a local trait's
//! implementation ([Authorized]).
//!
//! The hook answers a [Verdict]: allowed, denied, or [Pending], an answer that comes later (a person approving an
//! agent's outbound or destructive call). Both paths await a pending verdict inside the call itself: a server within
//! the request's deadline and cancel, a local caller as part of the call's future, so dropping the call withdraws
//! the question. Nothing changes on the wire: a remote client sees a slower answer, then the result or `Denied`.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::string::String;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::vec::Vec;

use ww_self::{ApiBundleOwned, Effects, MetaOwned};

use crate::CallContext;

/// What a request or a call does to the resource at its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Access {
    /// A method call, one or several (`MultiCall`); `index_of` of a keyed array.
    Call,
    /// A property or sink write, one or several (`MultiWrite`), and `SinkWrite`.
    Write,
    /// A property read, one or several (`MultiRead`); an array's valid indices, a key of a keyed array.
    Read,
    /// Opening, closing or a sideband of a stream.
    Stream,
    /// Observing properties of a trait level.
    Observe,
    /// The API's model, at the root or below.
    Introspect,
}

/// A trait as a global path names it (API-20).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TraitRef {
    /// A crate's name and a trait's name in it.
    Full {
        crate_id: String,
        trait_name: String,
    },
    /// A compact id assigned to a common trait.
    Compact(u32),
}

impl TraitRef {
    /// Whether a trait level of crate `crate_id`, named `trait_name` with compact id `compact_id`, is this one.
    pub fn matches(
        &self,
        crate_id: Option<&str>,
        trait_name: &str,
        compact_id: Option<u32>,
    ) -> bool {
        match self {
            TraitRef::Full {
                crate_id: c,
                trait_name: t,
            } => t == trait_name && crate_id == Some(c.as_str()),
            TraitRef::Compact(n) => compact_id == Some(*n),
        }
    }
}

/// Where a request goes, as it names it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// The resource indices from the API's root. A call through [Authorized] is always one.
    Absolute(Vec<u32>),
    /// A global path: the trait, wherever it sits in the API's tree, then the path from it.
    Trait(TraitRef, Vec<u32>),
}

/// One item on the way to a resource: a `ww_impl!` level, a keyed array's companion, or the item itself.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Step<'a> {
    /// The item's name in its trait (`index_of`, `key` and `keys` in a keyed array's companion).
    pub ident: &'a str,
    /// The item's id, as in a path.
    pub id: u32,
    /// The element's index in a resource array; `None` for an item that is not one, and for a whole array (its
    /// valid indices).
    pub index: Option<u32>,
    /// What using it does (API-58): its own `#[effect]` or its trait's, `read` by kind for property reads, consts and
    /// stream opens, `read` for a keyed array's companion; empty for a level, which does nothing by itself.
    pub effects: Effects,
    /// Its `ww_self` metadata (SRV-34's claims, shares, labels, API-58's effects as declared); `None`: none.
    pub meta: Option<&'a MetaOwned>,
}

impl<'a> Step<'a> {
    /// The step to item `ident` with id `id`.
    pub fn new(
        ident: &'a str,
        id: u32,
        index: Option<u32>,
        effects: Effects,
        meta: Option<&'a MetaOwned>,
    ) -> Self {
        Step {
            ident,
            id,
            index,
            effects,
            meta,
        }
    }
}

/// What a request or a call does where, as [Authorize::check] sees it.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Resource<'a> {
    /// What it does.
    pub access: Access,
    /// Where it goes, as the request names it: absolute, or a trait wherever it sits.
    pub place: &'a Place,
    /// The path from the API's root, array indices included: the request's own when absolute, a global one placed
    /// through the API's model; `None` for a global path without the model, or to a trait the model doesn't have.
    pub path: Option<&'a [u32]>,
    /// A multi request: it reaches the resources under [path](Self::path), not that one alone.
    pub multi: bool,
    /// The items [path](Self::path) goes through, from the root's down to the one it names, each with its array
    /// index, its effects and its metadata. Empty for the root (introspection of the whole API); `None` without the
    /// model or for a path that names nothing in it (the dispatcher answers that one with `BadPath` if it gets
    /// through). [Authorized] always has them, from constants its code generation wrote.
    pub items: Option<&'a [Step<'a>]>,
    /// The API's model a server was given, for what [items](Self::items) leaves out (`api.item_chain(path)`: the
    /// items' kinds and types); `None` for a call through [Authorized].
    pub api: Option<&'a ApiBundleOwned>,
    /// The mount slot (SRV-76) the request goes into, when [path](Self::path) goes below a slot's `api`: the request
    /// is sent on to the slot's backend. `None` for every other request.
    pub mount: Option<Mounted<'a>>,
}

/// Where a request goes into a mount slot, see [Resource::mount].
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Mounted<'a> {
    /// The path of the slot's `api` from the API's root: [Resource::path] goes on from there in the mounted API.
    pub api_path: &'a [u32],
    /// The mounted API's model, learned from its backend when it came up (SRV-79): the [Resource::items] below the
    /// slot's `api` are its items, with the effects the backend declares. `None` while none is learned; the items
    /// are `None` then, since nothing is known of what the request does.
    pub api: Option<&'a ApiBundleOwned>,
}

impl<'a> Mounted<'a> {
    /// The slot whose `api` is at `api_path`, serving the API `api` describes.
    pub fn new(api_path: &'a [u32], api: Option<&'a ApiBundleOwned>) -> Self {
        Mounted { api_path, api }
    }
}

impl<'a> Resource<'a> {
    /// A resource as a server or [Authorized] builds it.
    pub fn new(
        access: Access,
        place: &'a Place,
        path: Option<&'a [u32]>,
        multi: bool,
        items: Option<&'a [Step<'a>]>,
        api: Option<&'a ApiBundleOwned>,
    ) -> Self {
        Resource {
            access,
            place,
            path,
            multi,
            items,
            api,
            mount: None,
        }
    }

    /// The same resource, going into the mount slot `mount`.
    #[must_use]
    pub fn with_mount(mut self, mount: Option<Mounted<'a>>) -> Self {
        self.mount = mount;
        self
    }

    /// The last item on the way: the one the request names.
    pub fn item(&self) -> Option<&Step<'a>> {
        self.items.and_then(<[Step<'a>]>::last)
    }

    /// Effects of the item the request names, [Effects::NONE] for a level or without the items.
    pub fn effects(&self) -> Effects {
        self.item().map_or(Effects::NONE, |step| step.effects)
    }
}

/// The answer of [Authorize::check] that refuses a request: it is answered `ErrorKind::Denied` and doesn't run.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Denied;

impl fmt::Display for Denied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("denied by the authorization hook")
    }
}

impl std::error::Error for Denied {}

impl From<Denied> for crate::call::Error {
    fn from(_: Denied) -> Self {
        crate::call::Error::from_kind(crate::call::ErrorKind::Denied)
    }
}

/// An answer that comes later: a future of `Ok(())` (allowed) or `Err(Denied)`. It owns what it needs, since the
/// [Resource] it was asked about lives only as long as the check: a pending approval copies the description a person
/// reads. Dropping it (the call's deadline ran out, the caller cancelled or went away) withdraws the question.
pub struct Pending(Pin<Box<dyn Future<Output = Result<(), Denied>> + Send + 'static>>);

impl Pending {
    /// The verdict `answer` gives when it completes.
    pub fn new(answer: impl Future<Output = Result<(), Denied>> + Send + 'static) -> Self {
        Pending(Box::pin(answer))
    }
}

impl Future for Pending {
    type Output = Result<(), Denied>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.as_mut().poll(cx)
    }
}

impl fmt::Debug for Pending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pending")
    }
}

/// What [Authorize::check] answers.
#[derive(Debug)]
#[must_use]
pub enum Verdict {
    /// The request runs.
    Allow,
    /// The request is answered `Denied` and doesn't run.
    Deny,
    /// The answer comes later: the request waits for it, within its deadline, then runs or is denied.
    Pending(Pending),
}

impl Verdict {
    /// The answer, waited for when it is pending.
    ///
    /// # Errors
    ///
    /// [Denied] when the hook denied, at once or later.
    pub async fn resolve(self) -> Result<(), Denied> {
        match self {
            Verdict::Allow => Ok(()),
            Verdict::Deny => Err(Denied),
            Verdict::Pending(pending) => pending.await,
        }
    }
}

impl From<Result<(), Denied>> for Verdict {
    fn from(answer: Result<(), Denied>) -> Self {
        match answer {
            Ok(()) => Verdict::Allow,
            Err(Denied) => Verdict::Deny,
        }
    }
}

impl From<Pending> for Verdict {
    fn from(pending: Pending) -> Self {
        Verdict::Pending(pending)
    }
}

/// Decides whether a request may run: asked for every request of every connection of a host server, before its
/// handler, and for every call through [Authorized], before the implementation.
///
/// `call` is who asked as the transport saw it ([Peer](crate::Peer): a unix user, a key, an address; `Local` in the
/// same process), with the request's seq, priority, trace and deadline. A denied request is answered with
/// `ErrorKind::Denied` (`err_seq::REQUEST_DENIED` on the wire, `call::ErrorKind::Denied` in the same process) and
/// nothing runs; the caller learns nothing else. A request a server can't place (malformed, or of a kind from a newer
/// version) is denied without asking.
///
/// A check is quick and doesn't block: a policy answers at once, a question to a person answers
/// [Verdict::Pending]. [from_fn] makes a hook of a closure; `&A`, `Box<A>` and `Arc<A>` of a hook are hooks, so one
/// `Arc` serves a server and local wrappers together.
///
/// ```
/// use wire_weaver::authorize::{Access, Denied, Resource, from_fn};
/// use wire_weaver::{CallContext, Peer};
///
/// let only_reads_for_others = from_fn(|call: &CallContext, resource: &Resource<'_>| match call.peer {
///     Peer::Local => Ok(()),
///     _ if resource.access == Access::Read => Ok(()),
///     _ => Err(Denied),
/// });
/// ```
pub trait Authorize {
    /// Whether `call` may do what `resource` says.
    fn check(&self, call: &CallContext, resource: &Resource<'_>) -> Verdict;
}

impl<A: Authorize + ?Sized> Authorize for &A {
    fn check(&self, call: &CallContext, resource: &Resource<'_>) -> Verdict {
        (**self).check(call, resource)
    }
}

impl<A: Authorize + ?Sized> Authorize for Box<A> {
    fn check(&self, call: &CallContext, resource: &Resource<'_>) -> Verdict {
        (**self).check(call, resource)
    }
}

impl<A: Authorize + ?Sized> Authorize for Arc<A> {
    fn check(&self, call: &CallContext, resource: &Resource<'_>) -> Verdict {
        (**self).check(call, resource)
    }
}

/// A hook of a closure, see [from_fn].
#[derive(Clone, Copy)]
pub struct FromFn<F>(F);

impl<F> fmt::Debug for FromFn<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FromFn")
    }
}

/// A hook that calls `f`, which answers a `Result<(), Denied>`, a [Pending] or a [Verdict].
pub fn from_fn<F, R>(f: F) -> FromFn<F>
where
    F: Fn(&CallContext, &Resource<'_>) -> R,
    R: Into<Verdict>,
{
    FromFn(f)
}

impl<F, R> Authorize for FromFn<F>
where
    F: Fn(&CallContext, &Resource<'_>) -> R,
    R: Into<Verdict>,
{
    fn check(&self, call: &CallContext, resource: &Resource<'_>) -> Verdict {
        (self.0)(call, resource).into()
    }
}

/// An implementation of an API's local trait (CLT-57) behind an [Authorize] hook (SRV-72): every function asks the
/// hook with the call's `CallContext`, the item's path (array indices included) and the items on the way, then
/// forwards to `T`, or answers `ErrorKind::Denied`. A call in the same process passes the check a remote one does
/// (the same hook given to `wire_weaver_server::Server::authorize` sees the same path and items).
///
/// `ww_codegen!(.., local_trait = "..")` implements every level's trait for it: a nested level's accessor gives the
/// level's implementation wrapped with the same hook (`Authorized<T::Sub<'_>, &A>`), so a level and its arrays are
/// checked too. Reaching a level asks nothing; its functions do.
pub struct Authorized<T, A> {
    inner: T,
    hook: A,
    /// The levels on the way from the root to this one.
    at: Vec<Step<'static>>,
}

impl<T: fmt::Debug, A> fmt::Debug for Authorized<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Authorized")
            .field("inner", &self.inner)
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

impl<T, A: Authorize> Authorized<T, A> {
    /// The API's root implementation `inner`, its calls asked of `hook`.
    pub fn new(inner: T, hook: A) -> Self {
        Authorized {
            inner,
            hook,
            at: Vec::new(),
        }
    }

    /// The implementation calls are forwarded to, unchecked.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// The implementation, unwrapped.
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// The hook.
    pub fn hook(&self) -> &A {
        &self.hook
    }

    /// The level `inner` reached from this one through `step`, behind the same hook. Generated code calls it.
    #[doc(hidden)]
    pub fn __level<C>(&self, inner: C, step: Step<'static>) -> Authorized<C, &A> {
        let mut at = self.at.clone();
        at.push(step);
        Authorized {
            inner,
            hook: &self.hook,
            at,
        }
    }

    /// The hook's verdict on `access` to the item `steps` lead to from this level. Generated code calls it.
    #[doc(hidden)]
    pub fn __check(&self, cx: &CallContext, access: Access, steps: &[Step<'static>]) -> Verdict {
        let items = self.at.iter().chain(steps).copied().collect::<Vec<_>>();
        let path = items
            .iter()
            .flat_map(|step| core::iter::once(step.id).chain(step.index))
            .collect::<Vec<_>>();
        let place = Place::Absolute(path);
        let Place::Absolute(path) = &place else {
            unreachable!("built absolute above")
        };
        self.hook.check(
            cx,
            &Resource::new(access, &place, Some(path), false, Some(&items), None),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn levels_add_their_steps_to_the_path() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let hook = from_fn(move |_: &CallContext, resource: &Resource<'_>| {
            record.lock().expect("not poisoned").push((
                resource.path.map(<[u32]>::to_vec),
                resource.item().map(|s| s.ident.to_owned()),
                resource.effects(),
            ));
            if resource.effects().is_read() {
                Ok(())
            } else {
                Err(Denied)
            }
        });
        let root = Authorized::new((), hook);
        let unit = root.__level((), Step::new("unit", 2, Some(7), Effects::NONE, None));
        let cx = CallContext::local();
        let read = Step::new("id", 0, None, Effects::READ, None);
        assert!(matches!(
            unit.__check(&cx, Access::Call, &[read]),
            Verdict::Allow
        ));
        let edit = Step::new("level", 1, None, Effects::EDIT, None);
        assert!(matches!(
            unit.__check(&cx, Access::Write, &[edit]),
            Verdict::Deny
        ));
        assert_eq!(
            *seen.lock().expect("not poisoned"),
            [
                (Some(vec![2, 7, 0]), Some("id".to_owned()), Effects::READ),
                (Some(vec![2, 7, 1]), Some("level".to_owned()), Effects::EDIT),
            ]
        );
    }

    #[test]
    fn a_pending_verdict_resolves_to_its_answer() {
        let block = |f: Pending| {
            // a ready future: one poll answers
            let waker = std::task::Waker::noop();
            let mut cx = Context::from_waker(waker);
            let mut f = Box::pin(Verdict::Pending(f).resolve());
            match f.as_mut().poll(&mut cx) {
                Poll::Ready(r) => r,
                Poll::Pending => panic!("a ready answer"),
            }
        };
        assert_eq!(block(Pending::new(async { Ok(()) })), Ok(()));
        assert_eq!(block(Pending::new(async { Err(Denied) })), Err(Denied));
    }
}
