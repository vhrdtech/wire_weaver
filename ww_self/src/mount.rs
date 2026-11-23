//! The fixed level of a mount slot (SRV-76), `mount!(hub)` or an element of `mounts!(ext)`.
//!
//! A [Mount](crate::ApiItemKind::Mount) item holds only what varies (the expected API crate and the presence type);
//! the slot's operations are the same for every slot, so they are not stored in a bundle but built here, once for
//! every consumer (server and client codegen, the dynamic client, `ww`, Python).
//!
//! Paths: `presence` is the slot's path plus [PRESENCE]; the mounted API's root is the slot's path plus [API]. Its
//! items are not in this level, nor in the bundle: what is mounted is known only at run time, from the slot's own
//! introspection (an `Introspect` request at the slot's path plus [API]).

use crate::{
    ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, Multiplicity, PropertyAccess, TypeOwned,
};
use shrink_wrap::UNib32;

/// Id of `property!(ro presence: MountPresence +observe)` in a slot's level: whether a backend is mounted, and which
/// API it serves.
pub const PRESENCE: u32 = 0;
/// Id of the mounted API's root in a slot's level: a request under it reaches the backend with the slot's path, this
/// id included, taken off.
pub const API: u32 = 1;

/// Name of the item `id` of a slot's level: `presence` or `api`; None for any other id.
pub const fn item_name(id: u32) -> Option<&'static str> {
    match id {
        PRESENCE => Some("presence"),
        API => Some("api"),
        _ => None,
    }
}

/// Trait name of a slot's level; no trait of this name exists in any crate.
pub const LEVEL_NAME: &str = "MountSlot";

/// Name of the keyed companion of `mounts!(of)`: `ext_keys`, as [keyed!](crate::keys) names its own.
pub fn keys_ident(of: &str) -> String {
    format!("{of}_keys")
}

/// The level of the mount slot `mount`: its `presence` property. The mounted API's root ([API]) has no item here.
///
/// # Errors
///
/// `mount` is not a [Mount](crate::ApiItemKind::Mount) item.
pub fn slot_level(level: &ApiLevelOwned, mount: &ApiItemOwned) -> Result<ApiLevelOwned, NotMount> {
    let ApiItemKindOwned::Mount { presence_ty, .. } = &mount.kind else {
        return Err(NotMount);
    };
    Ok(ApiLevelOwned {
        docs: vec![],
        crate_idx: level.crate_idx,
        trait_name: LEVEL_NAME.to_owned(),
        items: vec![presence(presence_ty.clone())],
        compact_id: None,
        meta: None,
    })
}

fn presence(ty: TypeOwned) -> ApiItemOwned {
    ApiItemOwned {
        id: UNib32(PRESENCE),
        kind: ApiItemKindOwned::Property {
            ty,
            access: PropertyAccess::ReadOnly { observe: true },
            write_err_ty: None,
            errors: None,
            batched: None,
        },
        multiplicity: Multiplicity::Flat,
        since: None,
        ident: item_name(PRESENCE)
            .expect("the level's own ids have names")
            .to_owned(),
        docs: vec![],
        optional: None,
        meta: None,
        identity: None,
    }
}

/// The item is not a [Mount](crate::ApiItemKind::Mount) item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotMount;

impl core::fmt::Display for NotMount {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("not a mount! or mounts! item")
    }
}

impl core::error::Error for NotMount {}

#[cfg(test)]
mod tests {
    use super::*;

    fn mount() -> ApiItemOwned {
        ApiItemOwned {
            id: UNib32(3),
            kind: ApiItemKindOwned::Mount {
                expects: Some("hub_api".to_owned()),
                presence_ty: TypeOwned::OutOfLine {
                    type_idx: UNib32(7),
                },
            },
            multiplicity: Multiplicity::Flat,
            since: None,
            ident: "hub".to_owned(),
            docs: vec![],
            optional: None,
            meta: None,
            identity: None,
        }
    }

    fn level() -> ApiLevelOwned {
        ApiLevelOwned {
            docs: vec![],
            crate_idx: UNib32(0),
            trait_name: "Tv".to_owned(),
            items: vec![mount()],
            compact_id: None,
            meta: None,
        }
    }

    #[test]
    fn slot_level_has_presence_only() {
        let slot = slot_level(&level(), &mount()).unwrap();
        assert_eq!(slot.trait_name, LEVEL_NAME);
        assert_eq!(slot.items.len(), 1);
        let presence = &slot.items[0];
        assert_eq!(
            (presence.id.0, presence.ident.as_str()),
            (PRESENCE, "presence")
        );
        let ApiItemKindOwned::Property { ty, access, .. } = &presence.kind else {
            panic!("presence is a property");
        };
        assert!(matches!(access, PropertyAccess::ReadOnly { observe: true }));
        assert_eq!(
            ty,
            &TypeOwned::OutOfLine {
                type_idx: UNib32(7)
            }
        );
        assert_eq!(item_name(API), Some("api"));
        assert_eq!(item_name(2), None);
    }

    #[test]
    fn slot_level_refuses_other_items() {
        let mut other = mount();
        other.kind = ApiItemKindOwned::Trait {
            trait_idx: UNib32(0),
            omitted: None,
        };
        assert_eq!(slot_level(&level(), &other).unwrap_err(), NotMount);
    }
}
