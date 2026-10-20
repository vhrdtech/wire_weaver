//! Where in a device's API a global trait is implemented, from its introspection data.
//!
//! Calls and properties go by a global trait path the device resolves itself (API-20), but counted and reliable
//! streams are opened on an absolute path (CLT-23), so the stored log needs this. A trait implemented at several
//! places (two `PinDiag` boards behind one device) is told apart by the path's names ([find_all_traits]).

use wire_weaver::prelude::UNib32;
use wire_weaver_client::ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiLevelLocationOwned, ApiLevelOwned,
};

/// One place a trait is implemented at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraitAt {
    /// Absolute resource path, `[]` for the root itself.
    pub path: Vec<UNib32>,
    /// The items' names along the path joined with `.`, as the API declares them: `diag.pins`, empty for the root.
    pub name: String,
}

/// Absolute resource path of the first implementation of `crate_id`'s trait `trait_name` in the device's API:
/// the root itself is `[]`. Arrays of resources are not entered.
pub fn find_trait(
    bundle: &ApiBundleOwned,
    crate_id: &str,
    trait_name: &str,
) -> Option<Vec<UNib32>> {
    find_all_traits(bundle, crate_id, trait_name)
        .into_iter()
        .next()
        .map(|at| at.path)
}

/// Every implementation of `crate_id`'s trait `trait_name` in the device's API, depth first in declaration order,
/// the root first. Arrays of resources are not entered, nor an implementation's own items.
pub fn find_all_traits(bundle: &ApiBundleOwned, crate_id: &str, trait_name: &str) -> Vec<TraitAt> {
    let mut found = vec![];
    if is_trait(bundle, &bundle.root, crate_id, trait_name) {
        found.push(TraitAt {
            path: vec![],
            name: String::new(),
        });
        return found;
    }
    let mut path = vec![];
    search(
        bundle,
        &bundle.root,
        (crate_id, trait_name),
        &mut path,
        &mut found,
        0,
    );
    found
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
    (crate_id, name): (&str, &str),
    path: &mut Vec<(UNib32, String)>,
    found: &mut Vec<TraitAt>,
    depth: usize,
) {
    // trait implementations nest, but not without a bound
    if depth > 16 {
        return;
    }
    for item in &level.items {
        let ApiItemKindOwned::Trait { trait_idx, .. } = &item.kind else {
            continue;
        };
        if item.is_array() {
            continue;
        }
        path.push((item.id, item.ident.clone()));
        let at = || TraitAt {
            path: path.iter().map(|(id, _)| *id).collect(),
            name: path
                .iter()
                .map(|(_, n)| n.as_str())
                .collect::<Vec<_>>()
                .join("."),
        };
        match bundle.traits.get(trait_idx.0 as usize) {
            Some(ApiLevelLocationOwned::InLine { level, .. }) => {
                if is_trait(bundle, level, crate_id, name) {
                    found.push(at());
                } else {
                    search(bundle, level, (crate_id, name), path, found, depth + 1);
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
                found.push(at());
            }
            _ => {}
        }
        path.pop();
    }
}
