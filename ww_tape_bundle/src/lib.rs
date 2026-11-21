//! The name path of a `ww_tape` track's resource, from the API bundle (STD-125): what
//! [track_key] hashes.
//!
//! A WireWeaver track's [path](ww_tape::Items::WireWeaver::path) is numeric (item ids and array indices); the
//! bundle (`ww_self`) gives each id its identity name, and [keys](ww_tape::Items::WireWeaver::keys) the key that
//! stands for the index into a keyed array (API-67). Producers and readers build the name path here, so they agree
//! on the key. The tape format itself knows no bundle: this crate sits above both `ww_tape` and `ww_self`.

use ww_self::keys::{KeysError, item_name, keys_level};
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, Multiplicity};
use ww_tape::{Segment, api_root_id, track_key};

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

/// The item of id `id` in `level`, at path position `at`.
fn find(level: &ApiLevelOwned, at: usize, id: u32) -> Result<&ApiItemOwned, NamePathError> {
    let found = level.items.iter().find(|item| item.id.0 == id);
    found.ok_or(NamePathError::NoItem { at, id })
}

/// The name path of the resource at the numeric `path` in `bundle`: each item's identity name, each array index,
/// and for an index into a keyed array its key from `keys` (path position and the key's shrink_wrap bytes, as in
/// [Items::WireWeaver::keys](ww_tape::Items::WireWeaver::keys)).
pub fn name_path<'a>(
    bundle: &'a ApiBundleOwned,
    path: &[u32],
    keys: &[(usize, &'a [u8])],
) -> Result<Vec<Segment<'a>>, NamePathError> {
    let mut level = Level::Bundle(&bundle.root);
    let mut segments = Vec::with_capacity(path.len());
    let mut keyed_at = Vec::new();
    let mut steps = path.iter().copied().enumerate().peekable();
    while let Some((at, id)) = steps.next() {
        let name = match &level {
            Level::Bundle(lvl) => {
                let lvl: &'a ApiLevelOwned = lvl;
                find(lvl, at, id)?.identity_name()
            }
            Level::Keys(lvl) => {
                find(lvl, at, id)?;
                item_name(id).ok_or(NamePathError::NoItem { at, id })?
            }
        };
        segments.push(Segment::Name(name));
        let lvl = level.level();
        let item = find(lvl, at, id)?;
        if let Multiplicity::Array { .. } = item.multiplicity {
            let (at, index) = steps
                .next()
                .ok_or(NamePathError::MissingIndex { at: path.len() })?;
            let keyed = lvl.items.iter().any(
                |other| matches!(&other.kind, ApiItemKindOwned::Keys { of, .. } if of.0 == id),
            );
            let key = keys.iter().find(|(k, _)| *k == at);
            segments.push(match (keyed, key) {
                (true, Some((_, key))) => {
                    keyed_at.push(at);
                    Segment::Key(key)
                }
                (true, None) => return Err(NamePathError::MissingKey { at }),
                (false, Some(_)) => return Err(NamePathError::StrayKey { at }),
                (false, None) => Segment::Index(index),
            });
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
            _ => match steps.peek() {
                Some(&(at, _)) => return Err(NamePathError::TooLong { at }),
                None => break,
            },
        };
    }
    if let Some(&(at, _)) = keys.iter().find(|(at, _)| !keyed_at.contains(at)) {
        return Err(NamePathError::StrayKey { at });
    }
    Ok(segments)
}

/// The track key of the resource at `path` in `bundle`, under the [api_root_id] of the bundle's own crate: what a
/// reader checks a [TrackDef::key](ww_tape::TrackDef::key) against. See [name_path].
pub fn key_in_bundle(
    bundle: &ApiBundleOwned,
    path: &[u32],
    keys: &[(usize, &[u8])],
) -> Result<ww_tape::TrackKey, NamePathError> {
    let krate = bundle
        .crate_version(bundle.root.crate_idx.0)
        .map_err(NamePathError::Crate)?;
    Ok(track_key(
        api_root_id(&krate.crate_id),
        &name_path(bundle, path, keys)?,
    ))
}
