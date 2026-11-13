//! A bundle's definitions in an order that doesn't depend on where they were declared (EVO-75).
//!
//! The walker numbers a crate's traits, types and dependencies in the order it converts them, and that follows the
//! source layout: moving types from `lib.rs` into a `pub mod` (re-exported, same public paths) keeps every definition
//! but moves its index. Two bundles of the same definitions are then different bytes. [canonical_order] renumbers a
//! bundle so that its bytes depend only on its definitions, for comparing two of them: the order of first use from the
//! root, then what nothing reaches by name. Saved snapshots and what a device sends keep the order they were made
//! with; only the copies being compared are renumbered.

use crate::visit::{self, Visit};
use crate::visit_mut::{self, VisitMut};
use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    ItemEnumOwned, ItemStructOwned, Multiplicity, TypeLocationOwned, TypeOwned,
};
use shrink_wrap::UNib32;
use std::collections::VecDeque;

/// Renumbers `bundle`'s types, traits and crates (crate 0, the bundle's own, stays first) in the order of first use,
/// starting from the root; definitions nothing reaches (a crate snapshot's own types) follow in the order of their
/// crate and qualified name (`units::Volt`), each followed by what it uses. A crate snapshot's root (named after the
/// crate, one `Trait` item per trait, [ApiBundleOwned::root] of `load_crate`) is a list of the crate's traits, not an
/// API level with ids on the wire: its items are sorted by name and numbered again. Every other root keeps its items
/// as they are, their ids are the API's.
///
/// Two bundles with the same definitions are the same bytes after this, wherever the definitions were declared.
pub fn canonical_order(bundle: &mut ApiBundleOwned) {
    if is_crate_directory(bundle) {
        bundle.root.items.sort_by(|a, b| a.ident.cmp(&b.ident));
        for (id, item) in bundle.root.items.iter_mut().enumerate() {
            item.id = UNib32(id as u32);
        }
    }

    let mut order = Order {
        types: Numbering::new(bundle.types.len()),
        traits: Numbering::new(bundle.traits.len()),
        crates: Numbering::new(bundle.ext_crates.len()),
        queue: VecDeque::new(),
    };
    // the bundle's own crate first, whatever the root names
    order.crates.reach(0);
    order.visit_api_level(&bundle.root);
    order.drain(bundle);

    let crate_id = |idx: u32| {
        bundle
            .ext_crates
            .get(idx as usize)
            .map(|c| c.crate_id.clone())
            .unwrap_or_default()
    };
    let mut rest_types: Vec<(String, String, u32)> = (0..bundle.types.len() as u32)
        .filter(|idx| !order.types.is_reached(*idx))
        .map(|idx| {
            let (crate_idx, name) = match &bundle.types[idx as usize] {
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Struct(s),
                    crate_idx,
                } => (crate_idx.0, s.qualified_name()),
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Enum(e),
                    crate_idx,
                } => (crate_idx.0, e.qualified_name()),
                TypeLocationOwned::InLine { crate_idx, .. } => (crate_idx.0, String::new()),
                TypeLocationOwned::SkippedFullVersion {
                    crate_idx,
                    type_name,
                    ..
                } => (crate_idx.0, type_name.clone()),
            };
            (crate_id(crate_idx), name, idx)
        })
        .collect();
    // stable: two without a name (never in a snapshot, the walker drops what nothing uses) keep their order
    rest_types.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    for (_, _, idx) in rest_types {
        order.reach_type(idx);
        order.drain(bundle);
    }

    let mut rest_traits: Vec<(String, String, u32)> = (0..bundle.traits.len() as u32)
        .filter(|idx| !order.traits.is_reached(*idx))
        .map(|idx| match &bundle.traits[idx as usize] {
            ApiLevelLocationOwned::InLine { level, crate_idx } => {
                (crate_id(crate_idx.0), level.trait_name.clone(), idx)
            }
            ApiLevelLocationOwned::SkippedFullVersion {
                crate_idx,
                trait_name,
                ..
            } => (crate_id(crate_idx.0), trait_name.clone(), idx),
            ApiLevelLocationOwned::SkippedCompactVersion { .. } => {
                (String::new(), String::new(), idx)
            }
        })
        .collect();
    rest_traits.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    for (_, _, idx) in rest_traits {
        order.reach_trait(idx);
        order.drain(bundle);
    }

    let mut rest_crates: Vec<u32> = (0..bundle.ext_crates.len() as u32)
        .filter(|idx| !order.crates.is_reached(*idx))
        .collect();
    rest_crates.sort_by_key(|idx| {
        let c = &bundle.ext_crates[*idx as usize];
        let v = &c.version;
        (c.crate_id.clone(), v.major.0, v.minor.0, v.patch.0)
    });
    for idx in rest_crates {
        order.crates.reach(idx);
    }

    let mut remap = Remap {
        types: order.types.new_of_old(),
        traits: order.traits.new_of_old(),
        crates: order.crates.new_of_old(),
    };
    remap.visit_api_bundle(bundle);
    permute(&mut bundle.types, &remap.types);
    permute(&mut bundle.traits, &remap.traits);
    if let Some(ext_wire) = &mut bundle.ext_wire {
        // shorter than ext_crates: the crates past its end declare none
        ext_wire.resize(remap.crates.len(), None);
        permute(ext_wire, &remap.crates);
        while ext_wire.last() == Some(&None) {
            ext_wire.pop();
        }
    }
    permute(&mut bundle.ext_crates, &remap.crates);
}

/// A crate snapshot's root: named after the crate itself (traits are named in `CamelCase`, crates in `snake_case`),
/// only `Trait` items, none of them left out.
fn is_crate_directory(bundle: &ApiBundleOwned) -> bool {
    let root = &bundle.root;
    bundle
        .ext_crates
        .get(root.crate_idx.0 as usize)
        .is_some_and(|c| c.crate_id == root.trait_name)
        && root.items.iter().all(|item| {
            matches!(item.kind, ApiItemKindOwned::Trait { .. })
                && matches!(item.multiplicity, Multiplicity::Flat)
        })
        && bundle.root_omitted.as_ref().is_none_or(|o| o.is_empty())
}

/// New indices in the order things are reached.
struct Numbering {
    new: Vec<Option<u32>>,
    next: u32,
}

impl Numbering {
    fn new(len: usize) -> Self {
        Numbering {
            new: vec![None; len],
            next: 0,
        }
    }

    fn is_reached(&self, idx: u32) -> bool {
        self.new.get(idx as usize).is_some_and(Option::is_some)
    }

    /// Whether `idx` was reached for the first time (an index out of range never is: the bundle is broken, and
    /// renumbering leaves it as it is).
    fn reach(&mut self, idx: u32) -> bool {
        match self.new.get_mut(idx as usize) {
            Some(new @ None) => {
                *new = Some(self.next);
                self.next += 1;
                true
            }
            _ => false,
        }
    }

    /// Old index -> new index, for every index.
    fn new_of_old(self) -> Vec<u32> {
        self.new.into_iter().map(|new| new.unwrap()).collect()
    }
}

enum Pending {
    Type(u32),
    Trait(u32),
}

/// Numbers what it visits in the order of first use, and queues the definitions to visit next.
struct Order {
    types: Numbering,
    traits: Numbering,
    crates: Numbering,
    queue: VecDeque<Pending>,
}

impl Order {
    fn reach_type(&mut self, idx: u32) {
        if self.types.reach(idx) {
            self.queue.push_back(Pending::Type(idx));
        }
    }

    fn reach_trait(&mut self, idx: u32) {
        if self.traits.reach(idx) {
            self.queue.push_back(Pending::Trait(idx));
        }
    }

    fn drain(&mut self, bundle: &ApiBundleOwned) {
        while let Some(pending) = self.queue.pop_front() {
            match pending {
                Pending::Type(idx) => self.visit_type_location(&bundle.types[idx as usize]),
                Pending::Trait(idx) => self.visit_api_level_location(&bundle.traits[idx as usize]),
            }
        }
    }
}

impl Visit<'_> for Order {
    fn visit_type_location(&mut self, node: &TypeLocationOwned) {
        match node {
            TypeLocationOwned::InLine { crate_idx, .. }
            | TypeLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                self.crates.reach(crate_idx.0);
            }
        }
        visit::visit_type_location(self, node)
    }

    fn visit_api_level_location(&mut self, node: &ApiLevelLocationOwned) {
        match node {
            ApiLevelLocationOwned::InLine { crate_idx, .. }
            | ApiLevelLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                self.crates.reach(crate_idx.0);
            }
            ApiLevelLocationOwned::SkippedCompactVersion { .. } => {}
        }
        visit::visit_api_level_location(self, node)
    }

    fn visit_api_level(&mut self, node: &ApiLevelOwned) {
        self.crates.reach(node.crate_idx.0);
        visit::visit_api_level(self, node)
    }

    fn visit_api_item(&mut self, node: &ApiItemOwned) {
        if let Multiplicity::Array {
            index_type_idx: Some(type_idx),
        } = &node.multiplicity
        {
            self.reach_type(type_idx.0);
        }
        visit::visit_api_item(self, node)
    }

    fn visit_api_item_kind(&mut self, node: &ApiItemKindOwned) {
        if let ApiItemKindOwned::Trait { trait_idx, .. } = node {
            self.reach_trait(trait_idx.0);
        }
        visit::visit_api_item_kind(self, node)
    }

    fn visit_type(&mut self, node: &TypeOwned) {
        if let TypeOwned::OutOfLine { type_idx } | TypeOwned::Instance { type_idx, .. } = node {
            self.reach_type(type_idx.0);
        }
        visit::visit_type(self, node)
    }

    fn visit_item_struct(&mut self, node: &ItemStructOwned) {
        self.crates.reach(node.crate_idx.0);
        visit::visit_item_struct(self, node)
    }

    fn visit_item_enum(&mut self, node: &ItemEnumOwned) {
        self.crates.reach(node.crate_idx.0);
        visit::visit_item_enum(self, node)
    }
}

/// Rewrites type, trait and crate indices, old index -> new index; one out of range is left as it is.
struct Remap {
    types: Vec<u32>,
    traits: Vec<u32>,
    crates: Vec<u32>,
}

fn remap(idx: &mut UNib32, new: &[u32]) {
    if let Some(new) = new.get(idx.0 as usize) {
        idx.0 = *new;
    }
}

impl VisitMut for Remap {
    fn visit_type_location(&mut self, node: &mut TypeLocationOwned) {
        match node {
            TypeLocationOwned::InLine { crate_idx, .. }
            | TypeLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                remap(crate_idx, &self.crates)
            }
        }
        visit_mut::visit_type_location(self, node)
    }

    fn visit_api_level_location(&mut self, node: &mut ApiLevelLocationOwned) {
        match node {
            ApiLevelLocationOwned::InLine { crate_idx, .. }
            | ApiLevelLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                remap(crate_idx, &self.crates)
            }
            ApiLevelLocationOwned::SkippedCompactVersion { .. } => {}
        }
        visit_mut::visit_api_level_location(self, node)
    }

    fn visit_api_level(&mut self, node: &mut ApiLevelOwned) {
        remap(&mut node.crate_idx, &self.crates);
        visit_mut::visit_api_level(self, node)
    }

    fn visit_api_item(&mut self, node: &mut ApiItemOwned) {
        if let Multiplicity::Array {
            index_type_idx: Some(type_idx),
        } = &mut node.multiplicity
        {
            remap(type_idx, &self.types);
        }
        visit_mut::visit_api_item(self, node)
    }

    fn visit_api_item_kind(&mut self, node: &mut ApiItemKindOwned) {
        if let ApiItemKindOwned::Trait { trait_idx, .. } = node {
            remap(trait_idx, &self.traits);
        }
        visit_mut::visit_api_item_kind(self, node)
    }

    fn visit_type(&mut self, node: &mut TypeOwned) {
        if let TypeOwned::OutOfLine { type_idx } | TypeOwned::Instance { type_idx, .. } = node {
            remap(type_idx, &self.types);
        }
        visit_mut::visit_type(self, node)
    }

    fn visit_item_struct(&mut self, node: &mut ItemStructOwned) {
        remap(&mut node.crate_idx, &self.crates);
        visit_mut::visit_item_struct(self, node)
    }

    fn visit_item_enum(&mut self, node: &mut ItemEnumOwned) {
        remap(&mut node.crate_idx, &self.crates);
        visit_mut::visit_item_enum(self, node)
    }
}

/// `items[old]` moved to `items[new[old]]`.
fn permute<T>(items: &mut Vec<T>, new: &[u32]) {
    let mut slots: Vec<Option<T>> = std::iter::repeat_with(|| None).take(items.len()).collect();
    for (old, item) in std::mem::take(items).into_iter().enumerate() {
        slots[new[old] as usize] = Some(item);
    }
    items.extend(slots.into_iter().map(Option::unwrap));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FieldOwned, FieldsOwned, Repr, VariantOwned};
    use shrink_wrap::ElementSize;
    use shrink_wrap::SerializeShrinkWrapOwned;
    use ww_version::{FullVersionOwned, VersionOwned};

    fn level(name: &str) -> ApiLevelOwned {
        ApiLevelOwned {
            docs: vec![],
            crate_idx: UNib32(0),
            trait_name: name.into(),
            compact_id: None,
            meta: None,
            items: vec![],
        }
    }

    fn s(e_idx: u32) -> TypeLocationOwned {
        TypeLocationOwned::InLine {
            ty: TypeOwned::Struct(ItemStructOwned {
                instance_of: None,
                size: ElementSize::Unsized,
                crate_idx: UNib32(0),
                docs: vec![],
                ident: "S".into(),
                path: None,
                generics: vec![],
                fields: FieldsOwned::Named(vec![FieldOwned {
                    docs: vec![],
                    ident: Some("e".into()),
                    default: None,
                    since: None,
                    ty: TypeOwned::OutOfLine {
                        type_idx: UNib32(e_idx),
                    },
                    slot: None,
                    series_opaque: false,
                }]),
                sparse: false,
                extra: None,
            }),
            crate_idx: UNib32(0),
        }
    }

    fn e() -> TypeLocationOwned {
        TypeLocationOwned::InLine {
            ty: TypeOwned::Enum(ItemEnumOwned {
                instance_of: None,
                future_version: None,
                size: ElementSize::Unsized,
                repr: Repr::UNib32,
                crate_idx: UNib32(0),
                docs: vec![],
                ident: "E".into(),
                path: Some(vec!["modes".into()]),
                generics: vec![],
                variants: vec![VariantOwned {
                    docs: vec![],
                    ident: "V".into(),
                    fields: FieldsOwned::Unit,
                    discriminant: UNib32(0),
                    since: None,
                }],
            }),
            crate_idx: UNib32(0),
        }
    }

    /// A bundle of crate `c` with traits `A` and `B` as root items and types `S { e: E }`, `modes::E` that nothing
    /// reaches, all in the order a walk of one source layout gives (`a_first`) or of another; the root named `root`.
    fn bundle(root: &str, a_first: bool) -> ApiBundleOwned {
        let (names, types) = match a_first {
            true => (["A", "B"], vec![s(1), e()]),
            false => (["B", "A"], vec![e(), s(0)]),
        };
        let mut root = level(root);
        root.items = names
            .iter()
            .enumerate()
            .map(|(idx, name)| ApiItemOwned {
                id: UNib32(idx as u32),
                kind: ApiItemKindOwned::Trait {
                    trait_idx: UNib32(idx as u32),
                    omitted: None,
                },
                multiplicity: Multiplicity::Flat,
                since: None,
                ident: name.to_string(),
                docs: vec![],
                optional: None,
                meta: None,
            })
            .collect();
        ApiBundleOwned {
            magic: crate::MAGIC,
            ww_self_version: crate::VERSION,
            root,
            types,
            traits: names
                .iter()
                .map(|name| ApiLevelLocationOwned::InLine {
                    level: level(name),
                    crate_idx: UNib32(0),
                })
                .collect(),
            ext_crates: vec![FullVersionOwned::new(
                "c".into(),
                VersionOwned::new(0, 1, 0),
            )],
            root_omitted: None,
            wire: None,
            ext_wire: None,
        }
    }

    fn canonical(mut bundle: ApiBundleOwned) -> Vec<u8> {
        canonical_order(&mut bundle);
        bundle.to_ww_bytes_owned().unwrap()
    }

    /// Two layouts of one crate snapshot are one canonical bundle; renumbering it again changes nothing.
    #[test]
    fn layouts_of_a_crate_snapshot_are_the_same() {
        let a_first = bundle("c", true);
        let b_first = bundle("c", false);
        assert_ne!(
            a_first.to_ww_bytes_owned().unwrap(),
            b_first.to_ww_bytes_owned().unwrap()
        );
        assert_eq!(canonical(a_first.clone()), canonical(b_first));
        // the walker's order of this layout already is the canonical one
        assert_eq!(
            canonical(a_first.clone()),
            a_first.to_ww_bytes_owned().unwrap()
        );
    }

    /// An API root's items have ids on the wire: two roots with the traits at other ids stay two.
    #[test]
    fn an_api_root_keeps_its_ids() {
        assert_ne!(
            canonical(bundle("Api", true)),
            canonical(bundle("Api", false))
        );
    }
}
