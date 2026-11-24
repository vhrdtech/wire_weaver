//! Global trait paths (`PathKind::GlobalFull`, `GlobalCompact`, API-20) looked up in a model read in place, without
//! a heap: a firmware finds a trait of a module it serves in process (SRV-82) from the module's model bytes.
//!
//! The walk finds a trait as the model's own server finds it: the first one depth first, in the order the model
//! lists the items (their id order in a generated model), never under a resource array, in a protocol compatible
//! version of its crate. A trait the model leaves out (a dependency's, `ApiLevelLocation::SkippedFullVersion`) is
//! found by its crate and name, but nothing below it is: its items are not in the model.

use shrink_wrap::{RefVec, RefVecIter};
use ww_version::{FullVersion, Version};

use crate::{ApiBundle, ApiItem, ApiItemKind, ApiLevel, ApiLevelLocation, Multiplicity};

/// Trait levels below the root a walk goes down at most.
pub const TRAIT_WALK_DEPTH_MAX: usize = 16;
/// Trait items a walk looks at at most: a model whose traits refer to each other many times over ends the walk here.
pub const TRAIT_WALK_ITEMS_MAX: usize = 256;

/// The trait a global path asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WantedTrait<'a> {
    /// `GlobalFull`: by its crate, the crate's version and the trait's name.
    Full {
        crate_version: FullVersion<'a>,
        trait_name: &'a str,
    },
    /// `GlobalCompact`: by its `ww_global` id and its crate's version.
    Compact { id: u32, version: Version<'a> },
}

impl WantedTrait<'_> {
    fn is_level(&self, level: &ApiLevel<'_>, crate_version: &FullVersion<'_>) -> bool {
        match self {
            WantedTrait::Full {
                crate_version: wanted,
                trait_name,
            } => level.trait_name == *trait_name && crate_version.is_protocol_compatible(wanted),
            WantedTrait::Compact { id, version } => {
                level.compact_id.map(|c| c.0) == Some(*id)
                    && crate_version.version.is_protocol_compatible(version)
            }
        }
    }

    fn is_skipped(&self, trait_name: &str, crate_version: &FullVersion<'_>) -> bool {
        match self {
            WantedTrait::Full {
                crate_version: wanted,
                trait_name: wanted_name,
            } => trait_name == *wanted_name && crate_version.is_protocol_compatible(wanted),
            // a left out trait carries no `ww_global` id
            WantedTrait::Compact { .. } => false,
        }
    }
}

/// Where a walk found the trait: its path from the root, ids written into the caller's buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraitWalk {
    /// Found: the first `n` ids of the buffer are its path (`0`: the root is the trait).
    Found(usize),
    /// The model has no such trait within the walk's limits.
    NotFound,
    /// The model doesn't decode where the walk read it.
    Malformed,
}

impl ApiBundle<'_> {
    /// Find `wanted` in this model, see the [module](self) docs. Its path goes into `path`, whose length limits the
    /// depth too, at most [TRAIT_WALK_DEPTH_MAX]. No heap, a few hundred bytes of stack.
    pub fn find_trait(&self, wanted: &WantedTrait<'_>, path: &mut [u32]) -> TraitWalk {
        let crate_version = |idx: u32| -> Option<FullVersion<'_>> {
            self.ext_crates.iter().nth(usize::try_from(idx).ok()?)?.ok()
        };
        let Some(root_crate) = crate_version(self.root.crate_idx.0) else {
            return TraitWalk::Malformed;
        };
        if wanted.is_level(&self.root, &root_crate) {
            return TraitWalk::Found(0);
        }
        let depth_max = path.len().min(TRAIT_WALK_DEPTH_MAX);
        if depth_max == 0 {
            return TraitWalk::NotFound;
        }
        let mut frames: [Option<RefVecIter<'_, ApiItem<'_>>>; TRAIT_WALK_DEPTH_MAX] =
            [const { None }; TRAIT_WALK_DEPTH_MAX];
        frames[0] = Some(self.root.items.iter());
        let mut depth = 1;
        let mut items = 0;
        while depth > 0 {
            let Some(next) = frames[depth - 1].as_mut().and_then(Iterator::next) else {
                frames[depth - 1] = None;
                depth -= 1;
                continue;
            };
            let Ok(item) = next else {
                return TraitWalk::Malformed;
            };
            let ApiItemKind::Trait { trait_idx, .. } = item.kind else {
                continue;
            };
            if matches!(item.multiplicity, Multiplicity::Array { .. }) {
                continue;
            }
            items += 1;
            if items > TRAIT_WALK_ITEMS_MAX {
                return TraitWalk::NotFound;
            }
            let Some(location) = nth(&self.traits, trait_idx.0) else {
                return TraitWalk::Malformed;
            };
            path[depth - 1] = item.id.0;
            match location {
                ApiLevelLocation::InLine { level, .. } => {
                    let Some(level_crate) = crate_version(level.crate_idx.0) else {
                        return TraitWalk::Malformed;
                    };
                    if wanted.is_level(&level, &level_crate) {
                        return TraitWalk::Found(depth);
                    }
                    if depth < depth_max {
                        frames[depth] = Some(level.items.iter());
                        depth += 1;
                    }
                }
                ApiLevelLocation::SkippedFullVersion {
                    crate_idx,
                    trait_name,
                    ..
                } => {
                    let Some(level_crate) = crate_version(crate_idx.0) else {
                        return TraitWalk::Malformed;
                    };
                    if wanted.is_skipped(trait_name, &level_crate) {
                        return TraitWalk::Found(depth);
                    }
                }
                ApiLevelLocation::SkippedCompactVersion { .. } => {}
            }
        }
        TraitWalk::NotFound
    }
}

fn nth<'i>(traits: &RefVec<'i, ApiLevelLocation<'i>>, idx: u32) -> Option<ApiLevelLocation<'i>> {
    traits.iter().nth(usize::try_from(idx).ok()?)?.ok()
}

#[cfg(test)]
mod tests;
