//! What a request does and where, read from its envelope alone (SRV-34, SRV-46): the access, the path as the
//! request names it, and whether it reaches several resources. Claims in the hub and a host server's authorization
//! decide on it without the user API's types.

use std::string::{String, ToString};
use std::vec::Vec;

use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, RefVec};

use crate::{PathKind, Request, RequestKind, ResourceIndex};

/// What a request does to the resource at its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Access {
    /// A method call, one or several (`MultiCall`).
    Call,
    /// A property or sink write, one or several (`MultiWrite`), and `SinkWrite`.
    Write,
    /// A property read, one or several (`MultiRead`).
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
    /// The resource indices from the API's root.
    Absolute(Vec<u32>),
    /// A global path: the trait, wherever it sits in the API's tree, then the path from it.
    Trait(TraitRef, Vec<u32>),
}

/// What a request does where.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    pub access: Access,
    pub place: Place,
    /// A multi request: it reaches resources under its path, by the indices in its body.
    pub multi: bool,
}

/// Why a request has no [Target].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unplaced {
    /// The request doesn't decode with this build: malformed, or of a newer envelope.
    Unreadable,
    /// A kind that names no resource of the API: `Cancel` (of another request), or one from a newer version.
    NoResource,
}

/// What the request in `request` does where.
///
/// # Errors
///
/// [Unplaced] for a request that can't be placed: who decides on it must not let it through as if it were
/// harmless.
pub fn target(request: &[u8]) -> Result<Target, Unplaced> {
    let req = Request::from_ww_bytes(request).map_err(|_| Unplaced::Unreadable)?;
    let (access, multi) = match &req.kind {
        RequestKind::Call { .. } => (Access::Call, false),
        RequestKind::MultiCall { .. } => (Access::Call, true),
        RequestKind::Write { .. }
        | RequestKind::SinkWrite { .. }
        | RequestKind::SinkWriteUnits { .. } => (Access::Write, false),
        RequestKind::MultiWrite { .. } => (Access::Write, true),
        RequestKind::Read => (Access::Read, false),
        RequestKind::MultiRead { .. } => (Access::Read, true),
        RequestKind::StreamOpen { .. } | RequestKind::StreamSideband { .. } => {
            (Access::Stream, false)
        }
        RequestKind::Observe { .. } => (Access::Observe, false),
        RequestKind::Introspect => (Access::Introspect, false),
        RequestKind::Cancel { .. } | RequestKind::Unknown(_) => return Err(Unplaced::NoResource),
    };
    let ids = |p: &RefVec<'_, ResourceIndex>| {
        p.iter()
            .map(|n| n.map(|n| n.get().0))
            .collect::<Result<Vec<u32>, _>>()
            .map_err(|_| Unplaced::Unreadable)
    };
    let place = match &req.path_kind {
        PathKind::Absolute { path } => Place::Absolute(ids(path)?),
        PathKind::GlobalCompact {
            gid,
            path_from_trait,
        } => Place::Trait(TraitRef::Compact(gid.gid.id.0), ids(path_from_trait)?),
        PathKind::GlobalFull {
            gid,
            trait_name,
            path_from_trait,
        } => Place::Trait(
            TraitRef::Full {
                crate_id: gid.crate_id.to_string(),
                trait_name: trait_name.to_string(),
            },
            ids(path_from_trait)?,
        ),
    };
    Ok(Target {
        access,
        place,
        multi,
    })
}

#[cfg(test)]
mod tests {
    use wire_weaver::shrink_wrap::{SerializeShrinkWrapOwned, UNib32, UVlq32, UVlq32Backfill};

    use super::*;
    use crate::{PathKindOwned, RequestKindOwned, RequestOwned};

    fn request(path: &[u32], kind: RequestKindOwned) -> std::vec::Vec<u8> {
        RequestOwned {
            seq: UVlq32Backfill(5),
            priority: wire_weaver::Priority::NORMAL.to_wire(),
            context: None,
            path_kind: PathKindOwned::Absolute {
                path: path
                    .iter()
                    .map(|n| ResourceIndex::new(UNib32(*n)))
                    .collect(),
            },
            kind,
        }
        .to_ww_bytes_owned()
        .unwrap()
    }

    #[test]
    fn kinds_have_their_access() {
        let at = |kind, access| {
            assert_eq!(
                target(&request(&[2, 1], kind)),
                Ok(Target {
                    access,
                    place: Place::Absolute(vec![2, 1]),
                    multi: false
                })
            );
        };
        at(RequestKindOwned::Read, Access::Read);
        at(RequestKindOwned::Introspect, Access::Introspect);
        at(
            RequestKindOwned::Call {
                args: wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned(std::vec![]),
            },
            Access::Call,
        );
        at(
            RequestKindOwned::Write {
                data: wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned(std::vec![]),
            },
            Access::Write,
        );
    }

    #[test]
    fn requests_without_a_resource_are_not_placed() {
        assert_eq!(
            target(&request(&[], RequestKindOwned::Cancel { seq: UVlq32(3) })),
            Err(Unplaced::NoResource)
        );
        assert_eq!(target(&[0xff; 3]), Err(Unplaced::Unreadable));
        assert_eq!(target(&[]), Err(Unplaced::Unreadable));
    }

    #[test]
    fn trait_refs_match_by_name_or_compact_id() {
        let full = TraitRef::Full {
            crate_id: "ww_gpio".into(),
            trait_name: "Bank".into(),
        };
        assert!(full.matches(Some("ww_gpio"), "Bank", Some(4)));
        assert!(!full.matches(Some("ww_gpio"), "Pin", None));
        assert!(!full.matches(None, "Bank", None));
        assert!(TraitRef::Compact(4).matches(None, "Bank", Some(4)));
        assert!(!TraitRef::Compact(4).matches(Some("ww_gpio"), "Bank", None));
    }
}
