//! Optional items a server leaves out (API-16): `ww_codegen!(.., omit = [..], omit_if(<cfg>) = [..])`.
//!
//! See `docs/api/optional_items.md`.

use proc_macro2::TokenStream;
use shrink_wrap::UNib32;
use std::collections::{BTreeMap, HashMap};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
};

/// Items left out of a server: `path` is `<trait item>.<..>.<item or group>` from the API root (`"i2c.pullups"`,
/// `"reset"` for an item of the root itself), `cfg` the condition under which they are left out, `None` for always
/// (`omit = [..]`), `Some("not(feature = \"pullups\")")` for `omit_if(not(feature = "pullups")) = [..]`.
///
/// Only items marked `#[optional]` / `#[optional(group)]` in their trait can be left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Omit {
    pub path: String,
    pub cfg: Option<String>,
}

impl Omit {
    /// Left out always.
    pub fn always(path: impl Into<String>) -> Self {
        Omit {
            path: path.into(),
            cfg: None,
        }
    }

    /// Left out when `cfg` is true (the inside of a `#[cfg(..)]`).
    pub fn when(cfg: impl Into<String>, path: impl Into<String>) -> Self {
        Omit {
            path: path.into(),
            cfg: Some(cfg.into()),
        }
    }
}

/// At most this many different `omit_if` conditions, the introspection data is generated for each combination.
const MAX_CFGS: usize = 4;

/// Omitted items resolved against an API bundle.
#[derive(Default)]
pub(crate) struct Omissions {
    /// Trait item idents from the root to a level -> ids of the level's omitted items -> index into `cfgs`, or
    /// `None` when always omitted.
    levels: HashMap<Vec<String>, BTreeMap<u32, Option<usize>>>,
    /// Different `omit_if` conditions, in order of first use.
    cfgs: Vec<String>,
}

/// How an item is left out of a server.
pub(crate) enum Omission {
    Always,
    /// When the cfg (the inside of a `#[cfg(..)]`) is true.
    When(TokenStream),
}

impl Omissions {
    pub(crate) fn resolve(bundle: &ApiBundleOwned, omit: &[Omit]) -> Result<Self, String> {
        let mut omissions = Omissions::default();
        for o in omit {
            let cfg = match &o.cfg {
                Some(cfg) => {
                    let cfg = parse_cfg(cfg)?.to_string();
                    Some(match omissions.cfgs.iter().position(|c| *c == cfg) {
                        Some(idx) => idx,
                        None => {
                            omissions.cfgs.push(cfg);
                            omissions.cfgs.len() - 1
                        }
                    })
                }
                None => None,
            };
            let segments: Vec<&str> = o.path.split('.').collect();
            let (last, leading) = segments.split_last().unwrap();
            let mut level = &bundle.root;
            for segment in leading {
                let item = level
                    .items
                    .iter()
                    .find(|item| item.ident == *segment)
                    .ok_or_else(|| {
                        format!(
                            "omit \"{}\": no item `{segment}` in trait {}",
                            o.path, level.trait_name
                        )
                    })?;
                level = item.get_as_level(bundle).map_err(|_| {
                    format!(
                        "omit \"{}\": `{segment}` in trait {} is not a trait (ww_impl!)",
                        o.path, level.trait_name
                    )
                })?;
            }
            let ids =
                optional_items(level, last).map_err(|e| format!("omit \"{}\": {e}", o.path))?;
            let level_omissions = omissions
                .levels
                .entry(leading.iter().map(|s| s.to_string()).collect())
                .or_default();
            for id in ids {
                if level_omissions.insert(id, cfg).is_some() {
                    return Err(format!(
                        "omit \"{}\": an item is left out twice (in another omit or omit_if entry, or its group)",
                        o.path
                    ));
                }
            }
        }
        if omissions.cfgs.len() > MAX_CFGS {
            return Err(format!(
                "at most {MAX_CFGS} different omit_if conditions are supported, got {}",
                omissions.cfgs.len()
            ));
        }
        Ok(omissions)
    }

    /// Omitted items of the level reached through `path` (trait item idents from the root), by id.
    pub(crate) fn level(&self, path: &[String]) -> BTreeMap<u32, Omission> {
        let Some(level) = self.levels.get(path) else {
            return BTreeMap::new();
        };
        level
            .iter()
            .map(|(id, cfg)| {
                let omission = match cfg {
                    Some(idx) => Omission::When(self.cfgs[*idx].parse().unwrap()),
                    None => Omission::Always,
                };
                (*id, omission)
            })
            .collect()
    }

    /// Ids of the items of the level reached through `path` (trait item idents from the root) that are left out in a
    /// combination of the `omit_if` conditions (`active`, see [Self::cfg_combinations]).
    pub(crate) fn omitted_in(&self, path: &[String], active: &[bool]) -> Vec<u32> {
        let Some(level) = self.levels.get(path) else {
            return vec![];
        };
        level
            .iter()
            .filter(|(_, cfg)| cfg.is_none_or(|idx| active[idx]))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Every combination of the `omit_if` conditions: `#[cfg(..)]` contents (`None` when there are no conditions)
    /// and which conditions are true in it. One combination with nothing true when there are no conditions.
    pub(crate) fn cfg_combinations(&self) -> Vec<(Option<TokenStream>, Vec<bool>)> {
        if self.cfgs.is_empty() {
            return vec![(None, vec![])];
        }
        let cfgs: Vec<TokenStream> = self.cfgs.iter().map(|c| c.parse().unwrap()).collect();
        (0..1usize << cfgs.len())
            .map(|mask| {
                let active: Vec<bool> = (0..cfgs.len()).map(|i| mask & (1 << i) != 0).collect();
                let conditions = cfgs.iter().zip(&active).map(|(cfg, active)| {
                    if *active {
                        quote::quote! { #cfg }
                    } else {
                        quote::quote! { not(#cfg) }
                    }
                });
                (Some(quote::quote! { all(#(#conditions),*) }), active)
            })
            .collect()
    }

    /// The bundle as a device with these omissions describes itself: `omitted` lists filled in for the items
    /// omitted always and those whose condition is true in `active` (see [Self::cfg_combinations]).
    pub(crate) fn mark(&self, bundle: &ApiBundleOwned, active: &[bool]) -> ApiBundleOwned {
        let mut bundle = bundle.clone();
        let mut paths: Vec<_> = self.levels.keys().collect();
        paths.sort();
        for path in paths {
            let ids: Vec<UNib32> = self
                .omitted_in(path, active)
                .into_iter()
                .map(UNib32)
                .collect();
            if ids.is_empty() {
                continue;
            }
            let Some((last, leading)) = path.split_last() else {
                bundle.root_omitted = Some(ids);
                continue;
            };
            // the level holding the trait item, made unique to this path if its definition is shared
            let mut parent_idx = None;
            for segment in leading {
                let trait_idx = trait_item(level_mut(&mut bundle, parent_idx), segment);
                let trait_idx = unshare(&mut bundle, parent_idx, segment, trait_idx);
                parent_idx = Some(trait_idx);
            }
            let item = level_mut(&mut bundle, parent_idx)
                .items
                .iter_mut()
                .find(|item| item.ident == *last)
                .unwrap();
            if let ApiItemKindOwned::Trait { omitted, .. } = &mut item.kind {
                *omitted = Some(ids);
            }
        }
        bundle
    }
}

/// Ids of the items called `name` or in the group `name`, all of them `#[optional]`.
fn optional_items(level: &ApiLevelOwned, name: &str) -> Result<Vec<u32>, String> {
    let optional_names = || {
        let mut names = vec![];
        for item in &level.items {
            if let Some(optional) = &item.optional {
                names.push(format!("`{}`", item.ident));
                if let Some(group) = &optional.group {
                    let group = format!("`{group}` (group)");
                    if !names.contains(&group) {
                        names.push(group);
                    }
                }
            }
        }
        if names.is_empty() {
            format!("trait {} has no #[optional] items", level.trait_name)
        } else {
            format!(
                "optional in trait {}: {}",
                level.trait_name,
                names.join(", ")
            )
        }
    };
    if let Some(item) = level.items.iter().find(|item| item.ident == name) {
        if item.optional.is_none() {
            return Err(format!(
                "`{name}` is not #[optional] in trait {}, only optional items can be left out ({})",
                level.trait_name,
                optional_names()
            ));
        }
        return Ok(vec![item.id.0]);
    }
    let group: Vec<u32> = level
        .items
        .iter()
        .filter(|item| {
            item.optional
                .as_ref()
                .is_some_and(|o| o.group.as_deref() == Some(name))
        })
        .map(|item| item.id.0)
        .collect();
    if group.is_empty() {
        return Err(format!(
            "no item or group `{name}` in trait {} ({})",
            level.trait_name,
            optional_names()
        ));
    }
    Ok(group)
}

fn parse_cfg(cfg: &str) -> Result<TokenStream, String> {
    let ts: TokenStream = cfg
        .parse()
        .map_err(|e| format!("omit_if({cfg}): not a cfg predicate: {e}"))?;
    syn::parse2::<syn::Meta>(ts.clone())
        .map_err(|e| format!("omit_if({cfg}): not a cfg predicate: {e}"))?;
    Ok(ts)
}

fn level_mut(bundle: &mut ApiBundleOwned, trait_idx: Option<u32>) -> &mut ApiLevelOwned {
    match trait_idx {
        None => &mut bundle.root,
        Some(idx) => match &mut bundle.traits[idx as usize] {
            ApiLevelLocationOwned::InLine { level, .. } => level,
            _ => unreachable!("a server's bundle has every trait in-line"),
        },
    }
}

fn trait_item(level: &ApiLevelOwned, ident: &str) -> u32 {
    let item = level.items.iter().find(|item| item.ident == ident).unwrap();
    let ApiItemKindOwned::Trait { trait_idx, .. } = &item.kind else {
        unreachable!("checked in resolve")
    };
    trait_idx.0
}

/// Gives the trait item `ident` of a level its own copy of the trait's definition if other items refer to it too,
/// so that `omitted` can be set inside it for this path only. Returns the trait index to follow.
fn unshare(bundle: &mut ApiBundleOwned, parent: Option<u32>, ident: &str, trait_idx: u32) -> u32 {
    let mut refs = 0;
    let mut count = |items: &[ApiItemOwned]| {
        refs += items
            .iter()
            .filter(|item| matches!(item.kind, ApiItemKindOwned::Trait { trait_idx: t, .. } if t.0 == trait_idx))
            .count();
    };
    count(&bundle.root.items);
    for location in &bundle.traits {
        if let ApiLevelLocationOwned::InLine { level, .. } = location {
            count(&level.items);
        }
    }
    if refs <= 1 {
        return trait_idx;
    }
    let copy = bundle.traits[trait_idx as usize].clone();
    bundle.traits.push(copy);
    let new_idx = (bundle.traits.len() - 1) as u32;
    let item = level_mut(bundle, parent)
        .items
        .iter_mut()
        .find(|item| item.ident == ident)
        .unwrap();
    if let ApiItemKindOwned::Trait { trait_idx, .. } = &mut item.kind {
        trait_idx.0 = new_idx;
    }
    new_idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn bundle() -> ApiBundleOwned {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/traits_api");
        crate::load(&path, Some("Traits".into()), false).unwrap()
    }

    fn resolve(omit: &[Omit]) -> Result<Omissions, String> {
        Omissions::resolve(&bundle(), omit)
    }

    #[test]
    fn items_and_groups() {
        let omissions = resolve(&[
            Omit::always("reset"),
            Omit::always("i2c.pullups"),
            Omit::when("feature = \"x\"", "i2c.i2c_wiggle_clock"),
        ])
        .unwrap();
        let root = omissions.level(&[]);
        assert_eq!(root.keys().copied().collect::<Vec<_>>(), [4]);
        let i2c = omissions.level(&["i2c".into()]);
        assert_eq!(i2c.keys().copied().collect::<Vec<_>>(), [1, 2, 3]);
        assert!(matches!(i2c[&1], Omission::When(_)));
        assert!(matches!(i2c[&2], Omission::Always));
        assert_eq!(omissions.cfg_combinations().len(), 2);

        let marked = omissions.mark(&bundle(), &[false]);
        assert_eq!(marked.root_omitted, Some(vec![UNib32(4)]));
        let i2c = marked.root.items.iter().find(|i| i.ident == "i2c").unwrap();
        let ApiItemKindOwned::Trait { omitted, .. } = &i2c.kind else {
            panic!()
        };
        assert_eq!(omitted, &Some(vec![UNib32(2), UNib32(3)]));
    }

    #[test]
    fn only_optional_items() {
        let e = resolve(&[Omit::always("i2c.i2c_write")]).err().unwrap();
        assert!(
            e.contains("`i2c_write` is not #[optional] in trait I2c"),
            "{e}"
        );
        assert!(e.contains("`pullups` (group)"), "{e}");
        let e = resolve(&[Omit::always("g1.m1")]).err().unwrap();
        assert!(e.contains("trait Subgroup has no #[optional] items"), "{e}");
        let e = resolve(&[Omit::always("i2c.nope")]).err().unwrap();
        assert!(e.contains("no item or group `nope` in trait I2c"), "{e}");
        let e = resolve(&[Omit::always("nope.x")]).err().unwrap();
        assert!(e.contains("no item `nope` in trait Traits"), "{e}");
        let e = resolve(&[Omit::always("reset.x")]).err().unwrap();
        assert!(e.contains("`reset` in trait Traits is not a trait"), "{e}");
        let e = resolve(&[
            Omit::always("i2c.pullups"),
            Omit::always("i2c.i2c_set_pullups"),
        ])
        .err()
        .unwrap();
        assert!(e.contains("left out twice"), "{e}");
        let e = resolve(&[Omit::when("not a cfg(", "reset")]).err().unwrap();
        assert!(e.contains("not a cfg predicate"), "{e}");
    }
}
