//! The traits of a model read in place, without a heap (SRV-82): a firmware walks the model of a module it serves in
//! process once, at load, into the table its global trait paths (`PathKind::GlobalFull`, `GlobalCompact`, API-20) are
//! looked up in.
//!
//! The walk visits traits as the model's own server finds them for a global path: depth first, in the order the model
//! lists the items (their id order in a generated model), never under a resource array; the first visit of a trait is
//! the one a global path reaches. A trait the model leaves out (a dependency's, `ApiLevelLocation::SkippedFullVersion`)
//! is visited without its `ww_global` id, and nothing below it is: its items are not in the model.
//!
//! Reaching an item of a shrink_wrap model decodes everything before it, so a walk costs a decode of the model:
//! walk once and keep what is needed, never once per request.

use core::ops::ControlFlow;

use shrink_wrap::{BufReader, Error as ShrinkWrapError, RefVec};
use ww_version::FullVersion;

use crate::{ApiBundle, ApiItem, ApiItemKind, ApiLevelLocation, Multiplicity};

/// Trait levels below the root a walk goes down at most.
pub const TRAIT_WALK_DEPTH_MAX: usize = 16;
/// Trait items a walk looks at at most: a model whose traits refer to each other many times over ends the walk here.
pub const TRAIT_WALK_ITEMS_MAX: usize = 256;

/// A trait a walk reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraitAt<'p, 'm> {
    /// Its path from the root: empty for the root.
    pub path: &'p [u32],
    /// Its crate and the crate's version.
    pub crate_version: FullVersion<'m>,
    pub trait_name: &'m str,
    /// Its `ww_global` id, `None` without one or for a trait the model leaves out.
    pub compact_id: Option<u32>,
}

/// How a walk ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraitWalk {
    /// Every trait was visited.
    Done,
    /// The model holds more trait items than [TRAIT_WALK_ITEMS_MAX], or traits deeper than the walk may go: the traits
    /// visited before stand, the others weren't.
    Limit,
    /// The visitor stopped it.
    Stopped,
    /// The model doesn't decode where the walk read it: the traits visited before stand.
    Malformed,
}

impl<'m> ApiBundle<'m> {
    /// Visit the traits of this model as a global path finds them, with their paths in `path`.
    ///
    /// Depth first, in the model's order, never under a resource array. A trait the model leaves out (a
    /// dependency's) is visited without its `ww_global` id and not descended into. The length of `path` limits the
    /// depth too, at most [TRAIT_WALK_DEPTH_MAX]. No heap; the stack of a model decode, so walk once, never per
    /// request.
    pub fn visit_traits(
        &self,
        path: &mut [u32],
        mut visit: impl FnMut(&TraitAt<'_, 'm>) -> ControlFlow<()>,
    ) -> TraitWalk {
        let crate_version = |idx: u32| -> Option<FullVersion<'m>> {
            self.ext_crates.iter().nth(usize::try_from(idx).ok()?)?.ok()
        };
        let Some(root_crate) = crate_version(self.root.crate_idx.0) else {
            return TraitWalk::Malformed;
        };
        let root = TraitAt {
            path: &[],
            crate_version: root_crate,
            trait_name: self.root.trait_name,
            compact_id: self.root.compact_id.map(|id| id.0),
        };
        if visit(&root).is_break() {
            return TraitWalk::Stopped;
        }
        let depth_max = path.len().min(TRAIT_WALK_DEPTH_MAX);
        let mut root_items = Items::of(&self.root.items);
        if depth_max == 0 {
            return match root_items.next_trait() {
                Some(_) => TraitWalk::Limit,
                None => TraitWalk::Done,
            };
        }
        let mut frames: [Option<Items<'m>>; TRAIT_WALK_DEPTH_MAX] =
            [const { None }; TRAIT_WALK_DEPTH_MAX];
        frames[0] = Some(root_items);
        let mut depth = 1;
        let mut items = 0;
        while depth > 0 {
            let Some(next) = frames[depth - 1].as_mut().and_then(Items::next_trait) else {
                frames[depth - 1] = None;
                depth -= 1;
                continue;
            };
            let Ok((id, trait_idx)) = next else {
                return TraitWalk::Malformed;
            };
            items += 1;
            if items > TRAIT_WALK_ITEMS_MAX {
                return TraitWalk::Limit;
            }
            let Some(level) = Level::at(&self.traits, trait_idx) else {
                return TraitWalk::Malformed;
            };
            let Some(level) = level else {
                continue;
            };
            let Some(level_crate) = crate_version(level.crate_idx) else {
                return TraitWalk::Malformed;
            };
            path[depth - 1] = id;
            let at = TraitAt {
                path: &path[..depth],
                crate_version: level_crate,
                trait_name: level.trait_name,
                compact_id: level.compact_id,
            };
            if visit(&at).is_break() {
                return TraitWalk::Stopped;
            }
            let Some(mut below) = level.items else {
                continue;
            };
            if depth < depth_max {
                frames[depth] = Some(below);
                depth += 1;
            } else if below.next_trait().is_some() {
                return TraitWalk::Limit;
            }
        }
        TraitWalk::Done
    }
}

/// What the walk needs of a trait level: copied out of the model, never a clone of its items.
struct Level<'m> {
    crate_idx: u32,
    trait_name: &'m str,
    compact_id: Option<u32>,
    /// `None` for a trait the model leaves out.
    items: Option<Items<'m>>,
}

impl<'m> Level<'m> {
    /// The level at `idx` of the model's traits; `Some(None)` for one the walk doesn't know (a left out trait by its
    /// `ww_global` id only), `None` when it doesn't decode.
    fn at(traits: &RefVec<'m, ApiLevelLocation<'m>>, idx: u32) -> Option<Option<Self>> {
        let idx = usize::try_from(idx).ok()?;
        match traits {
            RefVec::Slice { slice } => Some(Self::of(slice.get(idx)?)),
            RefVec::Buf {
                buf,
                elements_count,
            } => {
                if idx >= *elements_count {
                    return None;
                }
                let mut buf = *buf;
                for _ in 0..idx {
                    buf.read::<ApiLevelLocation<'m>>().ok()?;
                }
                Some(Self::of(&buf.read::<ApiLevelLocation<'m>>().ok()?))
            }
        }
    }

    fn of(location: &ApiLevelLocation<'m>) -> Option<Self> {
        match location {
            ApiLevelLocation::InLine { level, .. } => Some(Level {
                crate_idx: level.crate_idx.0,
                trait_name: level.trait_name,
                compact_id: level.compact_id.map(|id| id.0),
                items: Some(Items::of(&level.items)),
            }),
            ApiLevelLocation::SkippedFullVersion {
                crate_idx,
                trait_name,
                ..
            } => Some(Level {
                crate_idx: crate_idx.0,
                trait_name,
                compact_id: None,
                items: None,
            }),
            ApiLevelLocation::SkippedCompactVersion { .. } => None,
        }
    }
}

/// The items of a level, read one by one: by reference from a slice, decoded from bytes, never cloned.
enum Items<'m> {
    Slice(core::slice::Iter<'m, ApiItem<'m>>),
    Buf { buf: BufReader<'m>, left: usize },
}

impl<'m> Items<'m> {
    fn of(items: &RefVec<'m, ApiItem<'m>>) -> Self {
        match items {
            RefVec::Slice { slice } => Items::Slice(slice.iter()),
            RefVec::Buf {
                buf,
                elements_count,
            } => Items::Buf {
                buf: *buf,
                left: *elements_count,
            },
        }
    }

    /// The id and trait index of the next trait item that isn't a resource array; `Some(Err)` when an item doesn't
    /// decode.
    fn next_trait(&mut self) -> Option<Result<(u32, u32), ShrinkWrapError>> {
        loop {
            let found = match self {
                Items::Slice(items) => flat_trait(items.next()?),
                Items::Buf { buf, left } => {
                    *left = left.checked_sub(1)?;
                    match buf.read::<ApiItem<'m>>() {
                        Ok(item) => flat_trait(&item),
                        Err(e) => return Some(Err(e)),
                    }
                }
            };
            if let Some(found) = found {
                return Some(Ok(found));
            }
        }
    }
}

fn flat_trait(item: &ApiItem<'_>) -> Option<(u32, u32)> {
    match (&item.kind, &item.multiplicity) {
        (ApiItemKind::Trait { trait_idx, .. }, Multiplicity::Flat) => {
            Some((item.id.0, trait_idx.0))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
