//! Claims on resources of devices (SRV-34): which program has which resource to itself, and whether a request of
//! another one is refused.
//!
//! What can be claimed comes from the device's API (`ww_self::Meta`, `#[claim(..)]`, `#[shares(..)]`, `#[label]`);
//! the hub never links it, it reads the device's introspection data. A claim belongs to an [Owner]: the program's
//! client instance (the instance id of its `ClientMeta`, CLT-41) as the principal it connected as, so that a
//! connection can't share a claim by sending someone else's instance id alone.

use std::collections::BTreeSet;

use tokio::time::Instant;
use wire_weaver::shrink_wrap::UNib32;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{RefVec, SerializeShrinkWrap, UVlq32};
use wire_weaver_client::ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, ClaimMode, FieldsOwned,
    FieldsValueOwned, TypeOwned, ValueOwned,
};
use ww_client_server::{Error, ErrorKind, Event, EventKind};
use ww_hub_api::{ClaimEntryOwned, ClaimErrorOwned, ClaimKind, ClientEntryOwned, ReleaseError};

/// Who a connection acts for. Today every connection is the local OS user the hub runs as (the socket lets no one
/// else in); remote peers (SRV-33) and their rights (SRV-37) get their own principals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Principal {
    LocalUser,
}

/// Holder of a claim: a client instance of a principal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub(crate) principal: Principal,
    pub(crate) instance: Instance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Instance {
    /// The instance id of the client's `ClientMeta`: shared by all connections of one client object
    Meta(u64),
    /// A client that sent no instance id: this one connection
    Connection(u32),
}

/// What a request does to the resource at its path, as far as claims care.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Access {
    /// A method call (also opening a stream function with arguments is not one: streams stay open)
    Call,
    /// A property or sink write
    Write,
    /// A property read
    Read,
    /// Opening, closing or a sideband of a stream
    Stream,
    /// Observing properties
    Observe,
}

/// A resource by its path in a device's API, what claiming it means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Resource {
    pub(crate) kind: ClaimKind,
    pub(crate) label: String,
    pub(crate) pins: BTreeSet<String>,
    pub(crate) peripherals: BTreeSet<String>,
}

impl Resource {
    /// The device's root: always claimable, exclusively.
    fn root() -> Self {
        Resource {
            kind: ClaimKind::Exclusive,
            label: String::new(),
            pins: BTreeSet::new(),
            peripherals: BTreeSet::new(),
        }
    }

    fn shares_with(&self, other: &Resource) -> bool {
        !self.pins.is_disjoint(&other.pins) || !self.peripherals.is_disjoint(&other.peripherals)
    }
}

fn kind_of(mode: ClaimMode) -> ClaimKind {
    match mode {
        ClaimMode::Exclusive => ClaimKind::Exclusive,
        ClaimMode::Shared => ClaimKind::Shared,
        ClaimMode::Observe => ClaimKind::Observe,
    }
}

/// The resource at `path` of the API in `bundle`: the root, or a `ww_impl!` (an element of a resource array with its
/// index, or the whole array without).
pub(crate) fn resolve(
    bundle: Option<&ApiBundleOwned>,
    path: &[u32],
) -> Result<Resource, Box<ClaimErrorOwned>> {
    if path.is_empty() {
        return Ok(Resource::root());
    }
    let bundle = bundle.ok_or(ClaimErrorOwned::ApiUnknown)?;
    let mut level: &ApiLevelOwned = &bundle.root;
    let mut rest = path;
    loop {
        let (id, after) = rest.split_first().ok_or(ClaimErrorOwned::BadPath)?;
        let item = level
            .items
            .iter()
            .find(|i| i.id.0 == *id)
            .ok_or(ClaimErrorOwned::BadPath)?;
        let (index, after) = match (item.is_array(), after.split_first()) {
            (true, Some((index, after))) => (Some(*index), after),
            _ => (None, after),
        };
        if !after.is_empty() {
            level = item
                .get_as_level(bundle)
                .map_err(|_| ClaimErrorOwned::BadPath)?;
            rest = after;
            continue;
        }
        if !matches!(item.kind, ApiItemKindOwned::Trait { .. }) {
            return Err(ClaimErrorOwned::NotClaimable.into());
        }
        let mode = item
            .claim_mode(bundle)
            .ok_or(ClaimErrorOwned::NotClaimable)?;
        let mut resource = Resource {
            kind: kind_of(mode),
            label: match (item.is_array(), index) {
                (true, None) => String::new(),
                _ => item.label(index).unwrap_or_default().to_string(),
            },
            pins: BTreeSet::new(),
            peripherals: BTreeSet::new(),
        };
        for shares in item.meta.iter().flat_map(|m| &m.shares) {
            // a whole array takes every element's
            let applies = match (shares.index, index) {
                (None, _) | (_, None) => true,
                (Some(i), Some(index)) => i.0 == index,
            };
            if applies {
                resource.pins.extend(shares.pins.iter().cloned());
                resource
                    .peripherals
                    .extend(shares.peripherals.iter().cloned());
            }
        }
        return Ok(resource);
    }
}

/// Where a request goes, as far as claims can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// The resource at this path and what is under it
    At(Vec<u32>),
    /// Several resources under this path (multi requests): everything below it
    Under(Vec<u32>),
    /// Unknown (a global path the hub can't place): anywhere on the device
    Anywhere,
}

impl Target {
    /// Whether the request reaches into the claimed resource at `claimed`.
    fn reaches(&self, claimed: &[u32]) -> bool {
        match self {
            Target::At(path) => path.starts_with(claimed),
            Target::Under(path) => path.starts_with(claimed) || claimed.starts_with(path),
            Target::Anywhere => true,
        }
    }
}

/// A trait as a global path names it (API-20).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TraitId {
    Full {
        crate_id: String,
        trait_name: String,
    },
    Compact(u32),
}

/// Deepest trait nesting looked through for a global path: introspection data is not trusted.
const MAX_DEPTH: usize = 32;

/// Where the device answers a global path to `id`: the first place the trait sits in the tree (depth first, in id
/// order, not under a resource array), as a server resolves it.
pub(crate) fn trait_path(bundle: &ApiBundleOwned, id: &TraitId) -> Option<Vec<u32>> {
    fn is(level: &ApiLevelOwned, bundle: &ApiBundleOwned, id: &TraitId) -> bool {
        match id {
            TraitId::Full {
                crate_id,
                trait_name,
            } => {
                &level.trait_name == trait_name
                    && level.crate_name(bundle).ok() == Some(crate_id.as_str())
            }
            TraitId::Compact(n) => level.compact_id.map(|c| c.0) == Some(*n),
        }
    }
    fn walk(
        level: &ApiLevelOwned,
        bundle: &ApiBundleOwned,
        id: &TraitId,
        prefix: &mut Vec<u32>,
    ) -> Option<Vec<u32>> {
        if is(level, bundle, id) {
            return Some(prefix.clone());
        }
        if prefix.len() >= MAX_DEPTH {
            return None;
        }
        let mut items: Vec<_> = level
            .items
            .iter()
            .filter(|i| matches!(i.kind, ApiItemKindOwned::Trait { .. }) && !i.is_array())
            .collect();
        items.sort_by_key(|i| i.id.0);
        for item in items {
            let Ok(sub) = item.get_as_level(bundle) else {
                continue;
            };
            prefix.push(item.id.0);
            let found = walk(sub, bundle, id, prefix);
            prefix.pop();
            if found.is_some() {
                return found;
            }
        }
        None
    }
    walk(&bundle.root, bundle, id, &mut vec![])
}

/// The item at an absolute request path (indices of resource arrays included).
fn item_at<'b>(bundle: &'b ApiBundleOwned, path: &[u32]) -> Option<&'b ApiItemOwned> {
    let mut level = &bundle.root;
    let mut rest = path;
    loop {
        let (id, after) = rest.split_first()?;
        let item = level.items.iter().find(|i| i.id.0 == *id)?;
        let after = match item.is_array() {
            true => after.split_first().map_or(after, |(_, a)| a),
            false => after,
        };
        if after.is_empty() {
            return Some(item);
        }
        level = item.get_as_level(bundle).ok()?;
        rest = after;
    }
}

/// `CommonError::InUse { by }` as a value of the error type `err_ty`: itself when it is `CommonError`, in its
/// `#[common] Common(CommonError)` variant when it is an API's error enum, `None` for any other type.
fn in_use_value(err_ty: &TypeOwned, bundle: &ApiBundleOwned, by: &str) -> Option<ValueOwned> {
    let in_use = ValueOwned::Enum {
        variant: "InUse".into(),
        fields: FieldsValueOwned::Named(vec![("by".into(), ValueOwned::String(by.into()))]),
    };
    if err_ty.is_common_error(bundle) {
        return Some(in_use);
    }
    let TypeOwned::Enum(item_enum) = err_ty.get_in_line(bundle).ok()? else {
        return None;
    };
    let common = item_enum.variants.iter().find(|v| {
        matches!(&v.fields, FieldsOwned::Unnamed(f) if f.len() == 1 && f[0].ty.is_common_error(bundle))
    })?;
    Some(ValueOwned::Enum {
        variant: common.ident.clone(),
        fields: FieldsValueOwned::Unnamed(vec![in_use]),
    })
}

/// The answer to a refused request with `seq`: `Common(InUse { by })` of the method's or the property's own error
/// type where it has one (a method returning `Result<_, E>`, a property with a write error type), else an error
/// event naming the holder (`ErrorKind::UserStr`).
pub(crate) fn in_use_reply(
    bundle: Option<&ApiBundleOwned>,
    path: Option<&[u32]>,
    access: Access,
    by: &str,
    seq: u32,
) -> Vec<u8> {
    enum Typed {
        /// A method's `Err` return value
        Value(Vec<u8>),
        /// A property's write error
        WriteError(Vec<u8>),
    }
    let typed = (|| {
        let bundle = bundle?;
        let item = item_at(bundle, path?)?;
        match (access, &item.kind) {
            (
                Access::Call,
                ApiItemKindOwned::Method {
                    return_ty: Some(ret @ TypeOwned::Result { err_ty, .. }),
                    ..
                },
            ) => {
                let err = in_use_value(err_ty, bundle, by)?;
                ValueOwned::Result(Err(Box::new(err)))
                    .ser_shrink_wrap_dyn(ret, bundle)
                    .ok()
                    .map(Typed::Value)
            }
            (
                Access::Write,
                ApiItemKindOwned::Property {
                    write_err_ty: Some(err_ty),
                    ..
                },
            ) => in_use_value(err_ty, bundle, by)?
                .ser_shrink_wrap_dyn(err_ty, bundle)
                .ok()
                .map(Typed::WriteError),
            _ => None,
        }
    })();
    let text = format!("in use by {by}");
    let result = match &typed {
        Some(Typed::Value(data)) => Ok(EventKind::Value {
            data: TailBytes(data),
        }),
        Some(Typed::WriteError(data)) => Err(Error::new(
            0,
            ErrorKind::UserBytes(RefVec::Slice { slice: data }),
        )),
        None => Err(Error::new(0, ErrorKind::UserStr(&text))),
    };
    let event = Event {
        seq: UVlq32(seq),
        result,
    };
    let data_len = match &typed {
        Some(Typed::Value(d) | Typed::WriteError(d)) => d.len(),
        None => 0,
    };
    let mut buf = vec![0u8; text.len() + data_len + 32];
    event
        .to_ww_bytes(&mut buf)
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

#[derive(Debug)]
pub(crate) struct Claim {
    pub(crate) id: u32,
    pub(crate) key: String,
    pub(crate) path: Vec<u32>,
    pub(crate) resource: Resource,
    pub(crate) purpose: String,
    pub(crate) owner: Owner,
    pub(crate) holder: ClientEntryOwned,
    pub(crate) since_ms: u64,
    pub(crate) expires: Option<(Instant, u64)>,
}

impl Claim {
    pub(crate) fn entry(&self) -> ClaimEntryOwned {
        ClaimEntryOwned {
            id: self.id,
            key: self.key.clone(),
            path: self.path.iter().map(|n| UNib32(*n)).collect(),
            kind: self.resource.kind,
            label: self.resource.label.clone(),
            purpose: self.purpose.clone(),
            holder: self.holder.clone(),
            since_ms: self.since_ms,
            expires_ms: self.expires.map(|(_, ms)| ms),
        }
    }

    /// Who holds it, short enough for `CommonError::InUse { by }` (32 bytes).
    pub(crate) fn by(&self) -> String {
        let mut by = match (self.holder.program.is_empty(), self.holder.pid) {
            (true, 0) => format!("client {}", self.holder.id),
            (true, pid) => format!("pid {pid}"),
            (false, 0) => self.holder.program.clone(),
            (false, pid) => format!("{} pid {pid}", self.holder.program),
        };
        if by.len() > 32 {
            let mut end = 32;
            while !by.is_char_boundary(end) {
                end -= 1;
            }
            by.truncate(end);
        }
        by
    }
}

/// Every claim the hub holds, on every device.
#[derive(Default)]
pub(crate) struct Claims {
    next_id: u32,
    list: Vec<Claim>,
}

/// A new claim, before it is taken.
pub(crate) struct NewClaim {
    pub(crate) key: String,
    pub(crate) path: Vec<u32>,
    pub(crate) resource: Resource,
    pub(crate) purpose: String,
    pub(crate) owner: Owner,
    pub(crate) holder: ClientEntryOwned,
    pub(crate) ttl_s: Option<u32>,
}

impl Claims {
    /// Drops the claims whose ttl ran out, returns them.
    pub(crate) fn expire(&mut self, now: Instant) -> Vec<Claim> {
        let (gone, kept) = std::mem::take(&mut self.list)
            .into_iter()
            .partition(|c| c.expires.is_some_and(|(at, _)| at <= now));
        self.list = kept;
        gone
    }

    /// Takes a claim, refused while another owner holds an overlapping one: the same resource (unless both are
    /// shared), one above or below it, or one sharing a pin or peripheral. The same owner may nest claims.
    pub(crate) fn claim(
        &mut self,
        new: NewClaim,
        now: Instant,
        now_ms: u64,
    ) -> Result<&Claim, Box<ClaimErrorOwned>> {
        for c in self
            .list
            .iter()
            .filter(|c| c.key == new.key && c.owner != new.owner)
        {
            let same = c.path == new.path;
            if same
                && c.resource.kind == ClaimKind::Shared
                && new.resource.kind == ClaimKind::Shared
            {
                continue;
            }
            if same {
                return Err(ClaimErrorOwned::InUse { claim: c.entry() }.into());
            }
            if c.path.starts_with(&new.path)
                || new.path.starts_with(&c.path)
                || c.resource.shares_with(&new.resource)
            {
                return Err(ClaimErrorOwned::Overlap { claim: c.entry() }.into());
            }
        }
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let ttl = new.ttl_s.map(|s| {
            (
                now + std::time::Duration::from_secs(s.into()),
                now_ms + u64::from(s) * 1000,
            )
        });
        self.list.push(Claim {
            id: self.next_id,
            key: new.key,
            path: new.path,
            resource: new.resource,
            purpose: new.purpose,
            owner: new.owner,
            holder: new.holder,
            since_ms: now_ms,
            expires: ttl,
        });
        Ok(self.list.last().expect("just pushed"))
    }

    pub(crate) fn release(&mut self, owner: &Owner, id: u32) -> Result<Claim, ReleaseError> {
        let pos = self
            .list
            .iter()
            .position(|c| c.id == id)
            .ok_or(ReleaseError::UnknownClaim)?;
        if &self.list[pos].owner != owner {
            return Err(ReleaseError::NotHolder);
        }
        Ok(self.list.remove(pos))
    }

    /// Drops every claim of `owner` (its last connection closed), returns them.
    pub(crate) fn drop_owner(&mut self, owner: &Owner) -> Vec<Claim> {
        let (gone, kept) = std::mem::take(&mut self.list)
            .into_iter()
            .partition(|c| &c.owner == owner);
        self.list = kept;
        gone
    }

    pub(crate) fn on_device<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Claim> + 'a {
        self.list.iter().filter(move |c| c.key == key)
    }

    /// The claim that refuses `access` to `target` on device `key` by `owner`, `None` when it may go: calls and
    /// writes are refused at or under another owner's claim (a shared one lets its other holders through), reads,
    /// streams and observing only under an `observe` claim.
    pub(crate) fn refusing(
        &self,
        key: &str,
        owner: &Owner,
        target: &Target,
        access: Access,
    ) -> Option<&Claim> {
        let holds_shared = |c: &Claim| {
            self.list.iter().any(|own| {
                &own.owner == owner
                    && own.key == key
                    && own.path == c.path
                    && own.resource.kind == ClaimKind::Shared
            })
        };
        self.list.iter().find(|c| {
            c.key == key
                && &c.owner != owner
                && target.reaches(&c.path)
                && match access {
                    Access::Call | Access::Write => {
                        !(c.resource.kind == ClaimKind::Shared && holds_shared(c))
                    }
                    Access::Read | Access::Stream | Access::Observe => {
                        c.resource.kind == ClaimKind::Observe
                    }
                }
        })
    }

    pub(crate) fn any_on(&self, key: &str) -> bool {
        self.list.iter().any(|c| c.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(n: u64) -> Owner {
        Owner {
            principal: Principal::LocalUser,
            instance: Instance::Meta(n),
        }
    }

    fn res(kind: ClaimKind, pins: &[&str]) -> Resource {
        Resource {
            kind,
            label: String::new(),
            pins: pins.iter().map(|p| p.to_string()).collect(),
            peripherals: BTreeSet::new(),
        }
    }

    fn new(o: u64, path: &[u32], resource: Resource, ttl_s: Option<u32>) -> NewClaim {
        NewClaim {
            key: "dev".into(),
            path: path.to_vec(),
            resource,
            purpose: "test".into(),
            owner: owner(o),
            holder: ClientEntryOwned {
                id: o as u32,
                pid: 0,
                program: format!("p{o}"),
                kind: String::new(),
                user: String::new(),
                host: String::new(),
                instance_id: o,
                since_ms: 0,
            },
            ttl_s,
        }
    }

    fn take(c: &mut Claims, n: NewClaim) -> Result<u32, Box<ClaimErrorOwned>> {
        c.claim(n, Instant::now(), 0).map(|c| c.id)
    }

    /// Same resource, above, below, by pins; shared claims together; the same owner nests.
    #[test]
    fn overlaps() {
        let ex = || res(ClaimKind::Exclusive, &[]);
        let mut c = Claims::default();
        take(&mut c, new(1, &[3, 0], ex(), None)).unwrap();
        assert!(matches!(
            take(&mut c, new(2, &[3, 0], ex(), None)),
            Err(e) if matches!(*e, ClaimErrorOwned::InUse { .. })
        ));
        assert!(matches!(
            take(&mut c, new(2, &[3], ex(), None)),
            Err(e) if matches!(*e, ClaimErrorOwned::Overlap { .. })
        ));
        assert!(matches!(
            take(&mut c, new(2, &[3, 0, 1], ex(), None)),
            Err(e) if matches!(*e, ClaimErrorOwned::Overlap { .. })
        ));
        assert!(matches!(
            take(&mut c, new(2, &[], ex(), None)),
            Err(e) if matches!(*e, ClaimErrorOwned::Overlap { .. })
        ));
        // another element of the array is free, the same owner nests
        take(&mut c, new(2, &[3, 1], ex(), None)).unwrap();
        take(&mut c, new(1, &[3, 0, 2], ex(), None)).unwrap();

        // a UART and the bank of its pins
        take(
            &mut c,
            new(1, &[5], res(ClaimKind::Exclusive, &["PA9", "PA10"]), None),
        )
        .unwrap();
        assert!(matches!(
            take(
                &mut c,
                new(2, &[6], res(ClaimKind::Exclusive, &["PA0", "PA9"]), None)
            ),
            Err(e) if matches!(*e, ClaimErrorOwned::Overlap { .. })
        ));

        // shared: together, an exclusive one above it is refused
        let sh = || res(ClaimKind::Shared, &[]);
        take(&mut c, new(1, &[7], sh(), None)).unwrap();
        take(&mut c, new(2, &[7], sh(), None)).unwrap();
        assert!(matches!(
            take(&mut c, new(3, &[], ex(), None)),
            Err(e) if matches!(*e, ClaimErrorOwned::Overlap { .. })
        ));
    }

    /// Calls and writes at or under a claim are refused for others, reads and streams only under `observe`; a
    /// shared claim lets its holders through.
    #[test]
    fn requests_refused() {
        let mut c = Claims::default();
        take(
            &mut c,
            new(1, &[3, 0], res(ClaimKind::Exclusive, &[]), None),
        )
        .unwrap();
        take(&mut c, new(1, &[4], res(ClaimKind::Observe, &[]), None)).unwrap();
        take(&mut c, new(1, &[5], res(ClaimKind::Shared, &[]), None)).unwrap();
        take(&mut c, new(2, &[5], res(ClaimKind::Shared, &[]), None)).unwrap();
        let at = |p: &[u32]| Target::At(p.to_vec());
        let refused = |o: u64, t: &Target, a: Access| c.refusing("dev", &owner(o), t, a).is_some();

        assert!(refused(2, &at(&[3, 0, 1]), Access::Call));
        assert!(refused(2, &at(&[3, 0]), Access::Write));
        assert!(!refused(1, &at(&[3, 0, 1]), Access::Call));
        assert!(!refused(2, &at(&[3, 1, 1]), Access::Call));
        assert!(!refused(2, &at(&[3, 0, 1]), Access::Read));
        assert!(!refused(2, &at(&[3, 0, 1]), Access::Stream));
        // the method of the root isn't under the claim, a multi request over the array is
        assert!(!refused(2, &at(&[3]), Access::Call));
        assert!(refused(2, &Target::Under(vec![3]), Access::Write));
        assert!(refused(2, &Target::Anywhere, Access::Call));
        assert!(!refused(1, &Target::Anywhere, Access::Call));

        assert!(refused(2, &at(&[4, 1]), Access::Read));
        assert!(refused(2, &at(&[4, 1]), Access::Observe));

        assert!(!refused(2, &at(&[5, 0]), Access::Call));
        assert!(refused(3, &at(&[5, 0]), Access::Call));
        assert!(!refused(3, &at(&[5, 0]), Access::Read));
    }

    #[test]
    fn released_dropped_expired() {
        let mut c = Claims::default();
        let ex = || res(ClaimKind::Exclusive, &[]);
        let a = take(&mut c, new(1, &[1], ex(), None)).unwrap();
        assert_eq!(
            c.release(&owner(2), a).unwrap_err(),
            ReleaseError::NotHolder
        );
        c.release(&owner(1), a).unwrap();
        assert_eq!(
            c.release(&owner(1), a).unwrap_err(),
            ReleaseError::UnknownClaim
        );

        take(&mut c, new(1, &[1], ex(), None)).unwrap();
        take(&mut c, new(1, &[2], ex(), None)).unwrap();
        assert_eq!(c.drop_owner(&owner(1)).len(), 2);
        assert!(!c.any_on("dev"));

        let now = Instant::now();
        c.claim(new(1, &[1], ex(), Some(5)), now, 0).unwrap();
        assert!(c.expire(now).is_empty());
        assert_eq!(c.expire(now + std::time::Duration::from_secs(5)).len(), 1);
        take(&mut c, new(2, &[1], ex(), None)).unwrap();
    }

    #[test]
    fn by_fits_in_use() {
        let mut c = Claims::default();
        let mut n = new(1, &[1], res(ClaimKind::Exclusive, &[]), None);
        n.holder.program = "a program with a very long name indeed".into();
        n.holder.pid = 4711;
        let claim = c.claim(n, Instant::now(), 0).unwrap();
        assert_eq!(claim.by(), "a program with a very long name ");
        assert!(claim.by().len() <= 32);
    }
}
