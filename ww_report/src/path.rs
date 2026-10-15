//! Where in a device's API a global trait is implemented, from its introspection data.
//!
//! Calls and properties go by a global trait path the device resolves itself (API-20), but counted and reliable
//! streams are opened on an absolute path (CLT-23), so the stored log needs this.

use wire_weaver::prelude::UNib32;
use wire_weaver_client::ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiLevelLocationOwned, ApiLevelOwned,
};

/// Absolute resource path of the first implementation of `crate_id`'s trait `trait_name` in the device's API:
/// the root itself is `[]`. Arrays of resources are not entered.
pub fn find_trait(
    bundle: &ApiBundleOwned,
    crate_id: &str,
    trait_name: &str,
) -> Option<Vec<UNib32>> {
    if is_trait(bundle, &bundle.root, crate_id, trait_name) {
        return Some(vec![]);
    }
    let mut stack = vec![];
    search(bundle, &bundle.root, crate_id, trait_name, &mut stack, 0)
}

fn is_trait(bundle: &ApiBundleOwned, level: &ApiLevelOwned, crate_id: &str, name: &str) -> bool {
    level.trait_name == name
        && bundle
            .ext_crates
            .get(level.crate_idx.0 as usize)
            .is_some_and(|c| c.crate_id == crate_id)
}

fn search(
    bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    crate_id: &str,
    name: &str,
    path: &mut Vec<UNib32>,
    depth: usize,
) -> Option<Vec<UNib32>> {
    // trait implementations nest, but not without a bound
    if depth > 16 {
        return None;
    }
    for item in &level.items {
        let ApiItemKindOwned::Trait { trait_idx, .. } = &item.kind else {
            continue;
        };
        if item.is_array() {
            continue;
        }
        path.push(item.id);
        let found = match bundle.traits.get(trait_idx.0 as usize) {
            Some(ApiLevelLocationOwned::InLine { level, .. }) => {
                if is_trait(bundle, level, crate_id, name) {
                    Some(path.clone())
                } else {
                    search(bundle, level, crate_id, name, path, depth + 1)
                }
            }
            Some(ApiLevelLocationOwned::SkippedFullVersion {
                crate_idx,
                trait_name,
                ..
            }) if trait_name == name
                && bundle
                    .ext_crates
                    .get(crate_idx.0 as usize)
                    .is_some_and(|c| c.crate_id == crate_id) =>
            {
                Some(path.clone())
            }
            _ => None,
        };
        path.pop();
        if found.is_some() {
            return found;
        }
    }
    None
}
