//! The name path of a track's resource, from the API bundle (STD-125): what [track_key](crate::track_key) hashes.
//!
//! A [TrackDef::path](crate::TrackDef::path) is numeric (item ids and array indices); the bundle gives each id its
//! identity name, and [TrackDef::keys](crate::TrackDef::keys) the key that stands for the index into a keyed array
//! (API-67). Producers and readers build the name path here, so they agree on the key.

use crate::{Segment, api_root_id, track_key};
use ww_self::keys::{KeysError, item_name, keys_level};
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, Multiplicity};

/// Why a numeric path has no name path in a bundle.
#[derive(Debug)]
pub enum NamePathError {
    /// No item of id `id` in the level reached at path position `at`.
    NoItem { at: usize, id: u32 },
    /// The path ends at an array, before its index.
    MissingIndex { at: usize },
    /// The index at path position `at` is into a keyed array, but no key is given for it.
    MissingKey { at: usize },
    /// A key is given for path position `at`, which is not an index into a keyed array.
    StrayKey { at: usize },
    /// The path goes on past a resource that has no items, at position `at`.
    TooLong { at: usize },
    /// The level of a trait item can't be read from the bundle (not in it, or skipped and not inlined).
    Trait(anyhow::Error),
    /// The crate of the bundle's root can't be read from the bundle.
    Crate(anyhow::Error),
    /// A key companion's level can't be built.
    Keys(KeysError),
}

impl core::fmt::Display for NamePathError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NamePathError::NoItem { at, id } => write!(f, "no item {id} at path position {at}"),
            NamePathError::MissingIndex { at } => {
                write!(f, "the path ends at an array, index {at} missing")
            }
            NamePathError::MissingKey { at } => {
                write!(f, "index {at} is into a keyed array, its key is missing")
            }
            NamePathError::StrayKey { at } => {
                write!(f, "a key for path position {at}, which is no keyed index")
            }
            NamePathError::TooLong { at } => {
                write!(f, "the path goes on at {at} past a resource without items")
            }
            NamePathError::Trait(e) => write!(f, "trait level: {e}"),
            NamePathError::Crate(e) => write!(f, "root crate: {e}"),
            NamePathError::Keys(e) => write!(f, "key companion: {e}"),
        }
    }
}

impl core::error::Error for NamePathError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            NamePathError::Trait(e) | NamePathError::Crate(e) => Some(e.as_ref()),
            NamePathError::Keys(e) => Some(e),
            _ => None,
        }
    }
}

/// A level on the path: one of the bundle, or the fixed level of a key companion, whose names are constants.
enum Level<'a> {
    Bundle(&'a ApiLevelOwned),
    Keys(ApiLevelOwned),
}

impl Level<'_> {
    fn level(&self) -> &ApiLevelOwned {
        match self {
            Level::Bundle(level) => level,
            Level::Keys(level) => level,
        }
    }
}

/// The name path of the resource at the numeric `path` in `bundle`: each item's identity name, each array index,
/// and for an index into a keyed array its key from `keys` (path position and the key's shrink_wrap bytes, as in
/// [TrackDef::keys](crate::TrackDef::keys)).
pub fn name_path<'a>(
    bundle: &'a ApiBundleOwned,
    path: &[u32],
    keys: &[(u32, &'a [u8])],
) -> Result<Vec<Segment<'a>>, NamePathError> {
    let mut level = Level::Bundle(&bundle.root);
    let mut segments = Vec::with_capacity(path.len());
    let mut keyed_at = Vec::new();
    let mut at = 0;
    while let Some(&id) = path.get(at) {
        let find = |level: &ApiLevelOwned| {
            level
                .items
                .iter()
                .position(|item| item.id.0 == id)
                .ok_or(NamePathError::NoItem { at, id })
        };
        let name = match &level {
            Level::Bundle(lvl) => {
                let lvl: &'a ApiLevelOwned = lvl;
                lvl.items[find(lvl)?].identity_name()
            }
            Level::Keys(lvl) => {
                find(lvl)?;
                item_name(id).ok_or(NamePathError::NoItem { at, id })?
            }
        };
        segments.push(Segment::Name(name));
        let lvl = level.level();
        let item = &lvl.items[find(lvl)?];
        at += 1;
        if let Multiplicity::Array { .. } = item.multiplicity {
            let index = *path.get(at).ok_or(NamePathError::MissingIndex { at })?;
            let keyed = lvl.items.iter().any(
                |other| matches!(&other.kind, ApiItemKindOwned::Keys { of, .. } if of.0 == id),
            );
            let key = keys.iter().find(|(k, _)| *k as usize == at);
            segments.push(match (keyed, key) {
                (true, Some((_, key))) => {
                    keyed_at.push(at);
                    Segment::Key(key)
                }
                (true, None) => return Err(NamePathError::MissingKey { at }),
                (false, Some(_)) => return Err(NamePathError::StrayKey { at }),
                (false, None) => Segment::Index(index),
            });
            at += 1;
        }
        level = match &item.kind {
            ApiItemKindOwned::Trait { trait_idx, .. } => Level::Bundle(
                bundle
                    .get_trait(trait_idx.0)
                    .map_err(NamePathError::Trait)?,
            ),
            ApiItemKindOwned::Keys { .. } => {
                Level::Keys(keys_level(lvl, item).map_err(NamePathError::Keys)?)
            }
            _ if at < path.len() => return Err(NamePathError::TooLong { at }),
            _ => break,
        };
    }
    if let Some((at, _)) = keys
        .iter()
        .find(|(at, _)| !keyed_at.contains(&(*at as usize)))
    {
        return Err(NamePathError::StrayKey { at: *at as usize });
    }
    Ok(segments)
}

/// The track key of the resource at `path` in `bundle`, under the [api_root_id] of the bundle's own crate: what a
/// reader checks a [TrackDef::key](crate::TrackDef::key) against. See [name_path].
pub fn key_in_bundle(
    bundle: &ApiBundleOwned,
    path: &[u32],
    keys: &[(u32, &[u8])],
) -> Result<u64, NamePathError> {
    let krate = bundle
        .crate_version(bundle.root.crate_idx.0)
        .map_err(NamePathError::Crate)?;
    Ok(track_key(
        api_root_id(&krate.crate_id),
        &name_path(bundle, path, keys)?,
    ))
}
