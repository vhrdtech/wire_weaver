//! The fixed level of a key companion (API-67), `keyed!(flow by FlowKey)`.
//!
//! A [Keys](crate::ApiItemKind::Keys) item holds only what varies (the keyed array, the key type and the event type); its
//! operations are the same for every keyed array, so they are not stored in a bundle but built here, once for every
//! consumer (server and client codegen, the dynamic client, `ww`, Python).

use crate::{
    ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, ArgumentOwned, Multiplicity,
    NumericAnyTypeOwned, NumericBaseType, PropertyAccess, TypeOwned,
};
use shrink_wrap::UNib32;

/// Id of `fn index_of(key: K) -> Option<I>` in a key companion's level: the index of the element with that key.
pub const INDEX_OF: u32 = 0;
/// Id of `property!(ro key[I]: K)` in a key companion's level: the key of each element, all of them in one multi
/// read.
pub const KEY: u32 = 1;
/// Id of `stream!(keys: KeyEvent<I, K>)` in a key companion's level: an element appeared or went away.
pub const KEYS: u32 = 2;

/// Trait name of a key companion's level; no trait of this name exists in any crate.
pub const LEVEL_NAME: &str = "Keys";

/// Why a key companion's level can't be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeysError {
    /// The item is not a [Keys](crate::ApiItemKind::Keys) item.
    NotKeys,
    /// No item of the level has the id of `of`.
    NoSuchItem { of: u32 },
    /// The item `of` names is not an array, or is a method or another key companion.
    NotKeyable { of: String },
}

impl core::fmt::Display for KeysError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            KeysError::NotKeys => f.write_str("not a keyed! item"),
            KeysError::NoSuchItem { of } => {
                write!(f, "keyed! names item {of}, which the level doesn't have")
            }
            KeysError::NotKeyable { of } => write!(
                f,
                "keyed! names `{of}`, which is not an array of traits, properties, streams or sinks"
            ),
        }
    }
}

impl core::error::Error for KeysError {}

/// Whether a key companion may name `item`: an array of traits, properties, streams, sinks or stream functions.
pub fn is_keyable(item: &ApiItemOwned) -> bool {
    matches!(item.multiplicity, Multiplicity::Array { .. })
        && !matches!(
            item.kind,
            ApiItemKindOwned::Method { .. } | ApiItemKindOwned::Keys { .. }
        )
}

/// The index type of a keyed array: its own index type (`flow[FlowIdx]`), or `UNib32` for `flow[]`, the bytes of an
/// index in a path.
pub fn index_ty(array: &ApiItemOwned) -> TypeOwned {
    match array.multiplicity {
        Multiplicity::Array {
            index_type_idx: Some(type_idx),
        } => TypeOwned::OutOfLine { type_idx },
        _ => TypeOwned::NumericAny(NumericAnyTypeOwned::Base(NumericBaseType::UNib32)),
    }
}

/// The level of the key companion `keys` in `level`: `index_of`, `key[]` and `keys`, with the array's index type.
pub fn keys_level(level: &ApiLevelOwned, keys: &ApiItemOwned) -> Result<ApiLevelOwned, KeysError> {
    let ApiItemKindOwned::Keys { of, key, event_ty } = &keys.kind else {
        return Err(KeysError::NotKeys);
    };
    let array = level
        .items
        .iter()
        .find(|item| item.id == *of)
        .ok_or(KeysError::NoSuchItem { of: of.0 })?;
    let (true, &Multiplicity::Array { index_type_idx }) = (is_keyable(array), &array.multiplicity)
    else {
        return Err(KeysError::NotKeyable {
            of: array.ident.clone(),
        });
    };
    let index = index_ty(array);
    let item = |id: u32, ident: &str, kind, multiplicity| ApiItemOwned {
        id: UNib32(id),
        kind,
        multiplicity,
        since: None,
        ident: ident.to_owned(),
        docs: vec![],
        optional: None,
        meta: None,
        identity: None,
    };
    let items = vec![
        item(
            INDEX_OF,
            "index_of",
            ApiItemKindOwned::Method {
                args: vec![ArgumentOwned {
                    ident: "key".to_owned(),
                    ty: key.clone(),
                    default: None,
                    flatten: false,
                }],
                return_ty: Some(TypeOwned::Option {
                    some_ty: Box::new(index),
                }),
                errors: None,
            },
            Multiplicity::Flat,
        ),
        item(
            KEY,
            "key",
            ApiItemKindOwned::Property {
                ty: key.clone(),
                access: PropertyAccess::ReadOnly { observe: false },
                write_err_ty: None,
                errors: None,
                batched: None,
            },
            Multiplicity::Array { index_type_idx },
        ),
        item(
            KEYS,
            "keys",
            ApiItemKindOwned::Stream {
                ty: event_ty.clone(),
                is_up: true,
            },
            Multiplicity::Flat,
        ),
    ];
    Ok(ApiLevelOwned {
        docs: vec![],
        crate_idx: level.crate_idx,
        trait_name: LEVEL_NAME.to_owned(),
        items,
        compact_id: None,
        meta: None,
    })
}
