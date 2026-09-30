use super::{
    api::convert_trait,
    crate_walker::{CrateContext, Scratch},
    ty::convert_ty_path_segment,
    util::{collect_docs, use_tree_names},
};
use anyhow::{Context, Result};
use shrink_wrap::UNib32;
use std::path::Path;
use syn::{Attribute, Ident, Item, PathSegment, Visibility};
use ww_self::signature::{no_resolve, trait_signature, type_signature};
use ww_self::visit::{self, Visit};
use ww_self::visit_mut::{self, VisitMut};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    ItemEnumOwned, ItemStructOwned, Multiplicity, TypeLocationOwned, TypeOwned,
};
use ww_version::FullVersionOwned;

/// Load all `#[ww_trait]`/`#[ww_api_root]` traits and all `#[derive_shrink_wrap]` types defined in a crate's `src/lib.rs`,
/// and types re-exported from its modules with `pub use`.
///
/// Intended to be saved and kept around as a crate snapshot, so that API bundles can refer to its traits and types
/// by crate name, version and item name only ([TypeLocationOwned::SkippedFullVersion] and
/// [ApiLevelLocationOwned::SkippedFullVersion]), instead of carrying full definitions.
///
/// Returned bundle layout:
/// * `root` is named after the crate, contains crate-level docs, and one [ApiItemKindOwned::Trait] item per trait
///   in source order (empty for crates with data types only).
/// * `ext_crates[0]` is the crate itself.
/// * Traits and types defined in this crate are in-line, traits and types from other crates are skipped
///   (their definitions belong to those crates' snapshots), with a signature of the left out definition
///   (see [ww_self::signature]). Entries only needed by skipped definitions are dropped.
pub fn load_crate(crate_path: &Path) -> Result<ApiBundleOwned> {
    let mut scratch = Scratch::default();
    scratch.dedup_traits = true;
    // loaded first, so gets crate_idx 0
    let entry = CrateContext::load(crate_path, &mut scratch)?;
    let mut items = vec![];
    for item in &entry.lib_rs_ast.items {
        match item {
            Item::Trait(item_trait)
                if has_attr(&item_trait.attrs, &["ww_trait", "ww_api_root"]) =>
            {
                let trait_idx = convert_trait(item_trait, &entry, &mut scratch)?;
                items.push(ApiItemOwned {
                    id: UNib32(items.len() as u32),
                    kind: ApiItemKindOwned::Trait { trait_idx },
                    multiplicity: Multiplicity::Flat,
                    since: None,
                    ident: item_trait.ident.to_string(),
                    docs: vec![],
                });
            }
            Item::Struct(item_struct) if has_attr(&item_struct.attrs, &["derive_shrink_wrap"]) => {
                convert_named_ty(&item_struct.ident, &entry, &mut scratch)?;
            }
            Item::Enum(item_enum) if has_attr(&item_enum.attrs, &["derive_shrink_wrap"]) => {
                convert_named_ty(&item_enum.ident, &entry, &mut scratch)?;
            }
            // e.g. `mod ty; pub use ty::Ty;`
            Item::Use(item_use) if matches!(item_use.vis, Visibility::Public(_)) => {
                for (path, name) in use_tree_names(&item_use.tree) {
                    let is_local = path.first().is_some_and(|first| {
                        *first == "self"
                            || *first == "crate"
                            || entry.has_module(&first.to_string())
                    });
                    if is_local
                        && is_shrink_wrap_ty(&*entry.resolve_path(path, &mut scratch)?, name)
                    {
                        convert_named_ty(name, &entry, &mut scratch)?;
                    }
                }
            }
            _ => {}
        }
    }

    let scratch = scratch.root_bundle;
    let mut bundle = ApiBundleOwned {
        magic: ww_self::MAGIC,
        ww_self_version: ww_self::VERSION,
        root: ApiLevelOwned {
            docs: collect_docs(&entry.lib_rs_ast.attrs),
            crate_idx: UNib32(0),
            trait_name: scratch.ext_crates[0].crate_id.clone(),
            items,
        },
        types: scratch.types,
        traits: scratch.traits,
        ext_crates: scratch.ext_crates,
    };
    skip_foreign(&mut bundle)?;
    drop_unused(&mut bundle, true);
    Ok(bundle)
}

fn is_shrink_wrap_ty(cx: &CrateContext, name: &Ident) -> bool {
    cx.lib_rs_ast.items.iter().any(|item| match item {
        Item::Struct(item_struct) => {
            &item_struct.ident == name && has_attr(&item_struct.attrs, &["derive_shrink_wrap"])
        }
        Item::Enum(item_enum) => {
            &item_enum.ident == name && has_attr(&item_enum.attrs, &["derive_shrink_wrap"])
        }
        _ => false,
    })
}

fn has_attr(attrs: &[Attribute], names: &[&str]) -> bool {
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|s| names.iter().any(|name| s.ident == name))
    })
}

fn convert_named_ty(
    ident: &Ident,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<()> {
    convert_ty_path_segment(&PathSegment::from(ident.clone()), current_crate, scratch)
        .with_context(|| format!("converting type {ident}"))?;
    Ok(())
}

/// Replace definitions of traits and types from other crates with references to them.
fn skip_foreign(bundle: &mut ApiBundleOwned) -> Result<()> {
    skip(bundle, |bundle, entry| {
        let (crate_idx, signature) = match entry {
            Entry::Type(idx) => (
                type_crate_idx(&bundle.types[idx as usize]),
                type_signature(bundle, idx, &no_resolve),
            ),
            Entry::Trait(idx) => (
                trait_crate_idx(&bundle.traits[idx as usize]),
                trait_signature(bundle, idx, &no_resolve),
            ),
        };
        Ok(if crate_idx == 0 {
            None
        } else {
            Some(signature?)
        })
    })
}

/// Bundle as sent by a device: traits and types known from embedded snapshots (see [wire_weaver_snapshots]) are
/// replaced with references to them, if they have exactly the same definition (signature) as in the snapshot.
/// Everything only used by them is dropped. The API root itself is kept in-line.
pub fn skip_known(bundle: &ApiBundleOwned) -> Result<ApiBundleOwned> {
    let mut bundle = bundle.clone();
    skip(&mut bundle, |bundle, entry| {
        let (crate_idx, name) = match entry {
            Entry::Type(idx) => match &bundle.types[idx as usize] {
                TypeLocationOwned::InLine {
                    ty:
                        TypeOwned::Struct(ItemStructOwned { ident, .. })
                        | TypeOwned::Enum(ItemEnumOwned { ident, .. }),
                    crate_idx,
                } => (crate_idx.0, ident),
                _ => return Ok(None),
            },
            Entry::Trait(idx) => match &bundle.traits[idx as usize] {
                ApiLevelLocationOwned::InLine { level, crate_idx } => {
                    (crate_idx.0, &level.trait_name)
                }
                _ => return Ok(None),
            },
        };
        let version = bundle.crate_version(crate_idx)?;
        let Some(snapshot) = known(version) else {
            return Ok(None);
        };
        let (signature, known_signature) = match entry {
            Entry::Type(idx) => {
                let Some(known_idx) = find_type(snapshot, name) else {
                    return Ok(None);
                };
                (
                    type_signature(bundle, idx, &known)?,
                    type_signature(snapshot, known_idx, &known)?,
                )
            }
            Entry::Trait(idx) => {
                let Some(known_idx) = find_trait(snapshot, name) else {
                    return Ok(None);
                };
                (
                    trait_signature(bundle, idx, &known)?,
                    trait_signature(snapshot, known_idx, &known)?,
                )
            }
        };
        // different definition: crate changed without a version bump, or a snapshot of an unpublished version
        Ok((signature == known_signature).then_some(signature))
    })?;
    drop_unused(&mut bundle, false);
    Ok(bundle)
}

/// Snapshot of a crate version embedded in [wire_weaver_snapshots], as a [ww_self::signature::Resolve] callback.
fn known<'a>(version: &FullVersionOwned) -> Option<&'a ApiBundleOwned> {
    wire_weaver_snapshots::get(version)
}

fn find_type(snapshot: &ApiBundleOwned, name: &str) -> Option<u32> {
    snapshot
        .types
        .iter()
        .position(|location| {
            matches!(location, TypeLocationOwned::InLine {
                ty: TypeOwned::Struct(ItemStructOwned { ident, .. }) | TypeOwned::Enum(ItemEnumOwned { ident, .. }),
                crate_idx: UNib32(0),
            } if ident == name)
        })
        .map(|idx| idx as u32)
}

fn find_trait(snapshot: &ApiBundleOwned, name: &str) -> Option<u32> {
    snapshot
        .traits
        .iter()
        .position(|location| {
            matches!(location, ApiLevelLocationOwned::InLine { level, crate_idx: UNib32(0) } if level.trait_name == name)
        })
        .map(|idx| idx as u32)
}

fn type_crate_idx(location: &TypeLocationOwned) -> u32 {
    match location {
        TypeLocationOwned::InLine { crate_idx, .. }
        | TypeLocationOwned::SkippedFullVersion { crate_idx, .. } => crate_idx.0,
    }
}

fn trait_crate_idx(location: &ApiLevelLocationOwned) -> u32 {
    match location {
        ApiLevelLocationOwned::InLine { crate_idx, .. }
        | ApiLevelLocationOwned::SkippedFullVersion { crate_idx, .. } => crate_idx.0,
        ApiLevelLocationOwned::SkippedCompactVersion { .. } => u32::MAX,
    }
}

#[derive(Clone, Copy)]
enum Entry {
    Type(u32),
    Trait(u32),
}

/// Replace in-line user-defined types and traits with references to them, where `signature_if_skipped` returns a
/// signature.
///
/// Signatures are calculated before anything is skipped, while all the definitions are still in-line.
fn skip(
    bundle: &mut ApiBundleOwned,
    mut signature_if_skipped: impl FnMut(&ApiBundleOwned, Entry) -> Result<Option<Vec<u8>>>,
) -> Result<()> {
    let mut type_signatures = vec![];
    for (idx, location) in bundle.types.iter().enumerate() {
        if matches!(location, TypeLocationOwned::InLine { .. }) {
            type_signatures.push(signature_if_skipped(bundle, Entry::Type(idx as u32))?);
        } else {
            type_signatures.push(None);
        }
    }
    let mut trait_signatures = vec![];
    for (idx, location) in bundle.traits.iter().enumerate() {
        if matches!(location, ApiLevelLocationOwned::InLine { .. }) {
            trait_signatures.push(signature_if_skipped(bundle, Entry::Trait(idx as u32))?);
        } else {
            trait_signatures.push(None);
        }
    }
    for (location, signature) in bundle.types.iter_mut().zip(type_signatures) {
        let TypeLocationOwned::InLine { ty, crate_idx } = location else {
            continue;
        };
        let type_name = match ty {
            TypeOwned::Struct(ItemStructOwned { ident, .. })
            | TypeOwned::Enum(ItemEnumOwned { ident, .. }) => ident.clone(),
            // array index types that are not user-defined
            _ => continue,
        };
        if let Some(signature) = signature {
            *location = TypeLocationOwned::SkippedFullVersion {
                crate_idx: *crate_idx,
                type_name,
                signature,
            };
        }
    }
    for (location, signature) in bundle.traits.iter_mut().zip(trait_signatures) {
        if let ApiLevelLocationOwned::InLine { level, crate_idx } = location
            && let Some(signature) = signature
        {
            *location = ApiLevelLocationOwned::SkippedFullVersion {
                crate_idx: *crate_idx,
                trait_name: level.trait_name.clone(),
                signature,
            };
        }
    }
    Ok(())
}

/// Drop types, traits and crates that are no longer referenced after [skip] and fix up all the indices.
/// Roots are the API root and, if `keep_own_types`, all the types defined in crate 0 (the crate itself).
fn drop_unused(bundle: &mut ApiBundleOwned, keep_own_types: bool) {
    let mut refs = Refs::default();
    refs.visit_api_level(&bundle.root);
    for (idx, location) in bundle.types.iter().enumerate() {
        if !keep_own_types {
            break;
        }
        if let TypeLocationOwned::InLine {
            ty: TypeOwned::Struct(_) | TypeOwned::Enum(_),
            crate_idx: UNib32(0),
        } = location
        {
            refs.types.push(idx as u32);
        }
    }
    let mut used_types = vec![false; bundle.types.len()];
    let mut used_traits = vec![false; bundle.traits.len()];
    loop {
        if let Some(idx) = refs.types.pop() {
            if !std::mem::replace(&mut used_types[idx as usize], true) {
                refs.visit_type_location(&bundle.types[idx as usize]);
            }
        } else if let Some(idx) = refs.traits.pop() {
            if !std::mem::replace(&mut used_traits[idx as usize], true) {
                refs.visit_api_level_location(&bundle.traits[idx as usize]);
            }
        } else {
            break;
        }
    }
    let mut used_crates = vec![false; bundle.ext_crates.len()];
    for idx in refs.crates {
        used_crates[idx as usize] = true;
    }

    let mut remap = Remap {
        types: new_indices(&used_types),
        traits: new_indices(&used_traits),
        crates: new_indices(&used_crates),
    };
    retain_used(&mut bundle.types, &used_types);
    retain_used(&mut bundle.traits, &used_traits);
    retain_used(&mut bundle.ext_crates, &used_crates);
    remap.visit_api_bundle(bundle);
}

fn new_indices(used: &[bool]) -> Vec<u32> {
    let mut next = 0;
    used.iter()
        .map(|used| {
            let idx = next;
            if *used {
                next += 1;
            }
            idx
        })
        .collect()
}

fn retain_used<T>(items: &mut Vec<T>, used: &[bool]) {
    let mut used = used.iter();
    items.retain(|_| *used.next().unwrap());
}

/// Collects type, trait and crate indices referenced from visited nodes.
#[derive(Default)]
struct Refs {
    types: Vec<u32>,
    traits: Vec<u32>,
    crates: Vec<u32>,
}

impl Visit<'_> for Refs {
    fn visit_type_location(&mut self, node: &TypeLocationOwned) {
        match node {
            TypeLocationOwned::InLine { crate_idx, .. }
            | TypeLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                self.crates.push(crate_idx.0)
            }
        }
        visit::visit_type_location(self, node)
    }

    fn visit_api_level_location(&mut self, node: &ApiLevelLocationOwned) {
        match node {
            ApiLevelLocationOwned::InLine { crate_idx, .. }
            | ApiLevelLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                self.crates.push(crate_idx.0)
            }
            ApiLevelLocationOwned::SkippedCompactVersion { .. } => {}
        }
        visit::visit_api_level_location(self, node)
    }

    fn visit_api_level(&mut self, node: &ApiLevelOwned) {
        self.crates.push(node.crate_idx.0);
        visit::visit_api_level(self, node)
    }

    fn visit_api_item(&mut self, node: &ApiItemOwned) {
        if let Multiplicity::Array {
            index_type_idx: Some(type_idx),
        } = &node.multiplicity
        {
            self.types.push(type_idx.0);
        }
        visit::visit_api_item(self, node)
    }

    fn visit_api_item_kind(&mut self, node: &ApiItemKindOwned) {
        if let ApiItemKindOwned::Trait { trait_idx } = node {
            self.traits.push(trait_idx.0);
        }
        visit::visit_api_item_kind(self, node)
    }

    fn visit_type(&mut self, node: &TypeOwned) {
        if let TypeOwned::OutOfLine { type_idx } = node {
            self.types.push(type_idx.0);
        }
        visit::visit_type(self, node)
    }

    fn visit_item_struct(&mut self, node: &ItemStructOwned) {
        self.crates.push(node.crate_idx.0);
        visit::visit_item_struct(self, node)
    }

    fn visit_item_enum(&mut self, node: &ItemEnumOwned) {
        self.crates.push(node.crate_idx.0);
        visit::visit_item_enum(self, node)
    }
}

/// Rewrites type, trait and crate indices, old index -> new index.
struct Remap {
    types: Vec<u32>,
    traits: Vec<u32>,
    crates: Vec<u32>,
}

impl Remap {
    fn crate_idx(&self, idx: &mut UNib32) {
        idx.0 = self.crates[idx.0 as usize];
    }
}

impl VisitMut for Remap {
    fn visit_type_location(&mut self, node: &mut TypeLocationOwned) {
        match node {
            TypeLocationOwned::InLine { crate_idx, .. }
            | TypeLocationOwned::SkippedFullVersion { crate_idx, .. } => self.crate_idx(crate_idx),
        }
        visit_mut::visit_type_location(self, node)
    }

    fn visit_api_level_location(&mut self, node: &mut ApiLevelLocationOwned) {
        match node {
            ApiLevelLocationOwned::InLine { crate_idx, .. }
            | ApiLevelLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                self.crate_idx(crate_idx)
            }
            ApiLevelLocationOwned::SkippedCompactVersion { .. } => {}
        }
        visit_mut::visit_api_level_location(self, node)
    }

    fn visit_api_level(&mut self, node: &mut ApiLevelOwned) {
        self.crate_idx(&mut node.crate_idx);
        visit_mut::visit_api_level(self, node)
    }

    fn visit_api_item(&mut self, node: &mut ApiItemOwned) {
        if let Multiplicity::Array {
            index_type_idx: Some(type_idx),
        } = &mut node.multiplicity
        {
            type_idx.0 = self.types[type_idx.0 as usize];
        }
        visit_mut::visit_api_item(self, node)
    }

    fn visit_api_item_kind(&mut self, node: &mut ApiItemKindOwned) {
        if let ApiItemKindOwned::Trait { trait_idx } = node {
            trait_idx.0 = self.traits[trait_idx.0 as usize];
        }
        visit_mut::visit_api_item_kind(self, node)
    }

    fn visit_type(&mut self, node: &mut TypeOwned) {
        if let TypeOwned::OutOfLine { type_idx } = node {
            type_idx.0 = self.types[type_idx.0 as usize];
        }
        visit_mut::visit_type(self, node)
    }

    fn visit_item_struct(&mut self, node: &mut ItemStructOwned) {
        self.crate_idx(&mut node.crate_idx);
        visit_mut::visit_item_struct(self, node)
    }

    fn visit_item_enum(&mut self, node: &mut ItemEnumOwned) {
        self.crate_idx(&mut node.crate_idx);
        visit_mut::visit_item_enum(self, node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_version::FullVersionOwned;

    #[test]
    fn ww_uart_snapshot() {
        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ww_stdlib/ww_uart");
        let bundle = load_crate(&crate_path).unwrap();
        assert_eq!(bundle.root.trait_name, "ww_uart");
        let crates: Vec<_> = bundle
            .ext_crates
            .iter()
            .map(|c| c.crate_id.as_str())
            .collect();
        assert_eq!(crates, ["ww_uart", "ww_si"]);

        // own types in-line, ww_si ones only referenced, nothing ww_si types refer to is left over
        for location in &bundle.types {
            match location {
                TypeLocationOwned::InLine { crate_idx, .. } => assert_eq!(crate_idx.0, 0),
                TypeLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                    assert_eq!(crate_idx.0, 1)
                }
            }
        }
        let skipped = |name: &str| {
            bundle.types.iter().any(|l| {
                matches!(l, TypeLocationOwned::SkippedFullVersion { type_name, .. } if type_name == name)
            })
        };
        assert!(skipped("Volt"));
        assert!(skipped("Second"));

        // every index points to an existing entry
        let mut refs = Refs::default();
        visit::visit_api_bundle(&mut refs, &bundle);
        assert!(
            refs.types
                .iter()
                .all(|&idx| (idx as usize) < bundle.types.len())
        );
        assert!(
            refs.traits
                .iter()
                .all(|&idx| (idx as usize) < bundle.traits.len())
        );
        assert!(
            refs.crates
                .iter()
                .all(|&idx| (idx as usize) < bundle.ext_crates.len())
        );
    }

    #[test]
    fn traits_are_not_duplicated() {
        // Bank refers to Pin, both are saved once
        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ww_stdlib/ww_gpio");
        let bundle = load_crate(&crate_path).unwrap();
        let names: Vec<_> = bundle.root.items.iter().map(|i| i.ident.as_str()).collect();
        assert_eq!(names, ["Bank", "Pin"]);
        assert_eq!(bundle.traits.len(), 2);
    }

    fn load(relative_path: &str) -> ApiBundleOwned {
        load_crate(&Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path)).unwrap()
    }

    fn type_idx(bundle: &ApiBundleOwned, name: &str) -> u32 {
        bundle
            .types
            .iter()
            .position(|l| match l {
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Struct(ItemStructOwned { ident, .. }),
                    ..
                }
                | TypeLocationOwned::InLine {
                    ty: TypeOwned::Enum(ItemEnumOwned { ident, .. }),
                    ..
                }
                | TypeLocationOwned::SkippedFullVersion {
                    type_name: ident, ..
                } => ident == name,
                _ => false,
            })
            .unwrap() as u32
    }

    fn trait_idx(bundle: &ApiBundleOwned, name: &str) -> u32 {
        bundle
            .traits
            .iter()
            .position(|l| match l {
                ApiLevelLocationOwned::InLine { level, .. } => level.trait_name == name,
                ApiLevelLocationOwned::SkippedFullVersion { trait_name, .. } => trait_name == name,
                ApiLevelLocationOwned::SkippedCompactVersion { .. } => false,
            })
            .unwrap() as u32
    }

    /// Resolves skipped definitions from snapshots of exactly the same crate version.
    fn from_snapshots<'a>(
        snapshots: &'a [ApiBundleOwned],
    ) -> impl Fn(&FullVersionOwned) -> Option<&'a ApiBundleOwned> {
        |version| snapshots.iter().find(|s| &s.ext_crates[0] == version)
    }

    #[test]
    fn skipped_type_signature_matches_its_crate_snapshot() {
        let uart = load("../ww_stdlib/ww_uart");
        let volt = type_idx(&uart, "Volt");
        let TypeLocationOwned::SkippedFullVersion { signature, .. } = &uart.types[volt as usize]
        else {
            panic!("Volt is not skipped");
        };
        assert_eq!(signature.len(), ww_self::signature::SIGNATURE_LEN);

        // Volt refers to ww_numeric types, which are skipped in the ww_si snapshot as well
        let snapshots = [load("../ww_stdlib/ww_si"), load("../ww_stdlib/ww_numeric")];
        let resolve = from_snapshots(&snapshots);
        assert!(type_signature(&uart, volt, &no_resolve).is_err());
        assert_eq!(signature, &type_signature(&uart, volt, &resolve).unwrap());

        let second = type_idx(&uart, "Second");
        assert_ne!(signature, &type_signature(&uart, second, &resolve).unwrap());
    }

    #[test]
    fn skipped_trait_signature_matches_its_crate_snapshot() {
        let all_gpio = load("../examples/all_gpio_api");
        let bank = trait_idx(&all_gpio, "Bank");
        let ApiLevelLocationOwned::SkippedFullVersion { signature, .. } =
            &all_gpio.traits[bank as usize]
        else {
            panic!("Bank is not skipped");
        };
        let snapshots = [
            load("../ww_stdlib/ww_gpio"),
            load("../ww_stdlib/ww_si"),
            load("../ww_stdlib/ww_numeric"),
        ];
        let resolve = from_snapshots(&snapshots);
        assert_eq!(
            signature,
            &trait_signature(&all_gpio, bank, &resolve).unwrap()
        );
        // same when starting from the ww_gpio snapshot itself
        let gpio = &snapshots[0];
        let gpio_bank = trait_idx(gpio, "Bank");
        assert_eq!(
            signature,
            &trait_signature(gpio, gpio_bank, &resolve).unwrap()
        );
    }

    #[test]
    fn trait_signature_resolves_through_several_snapshots() {
        // uart_api uses ww_uart traits, which refer to ww_si types, which refer to ww_numeric types
        let uart_api = load("../examples/uart_api");
        let (idx, signature) = uart_api
            .traits
            .iter()
            .enumerate()
            .find_map(|(idx, l)| match l {
                ApiLevelLocationOwned::SkippedFullVersion { signature, .. } => {
                    Some((idx as u32, signature))
                }
                _ => None,
            })
            .expect("a skipped ww_uart trait");
        let snapshots = [
            load("../ww_stdlib/ww_uart"),
            load("../ww_stdlib/ww_si"),
            load("../ww_stdlib/ww_numeric"),
        ];
        let resolve = from_snapshots(&snapshots);
        assert_eq!(
            signature,
            &trait_signature(&uart_api, idx, &resolve).unwrap()
        );
        assert!(trait_signature(&uart_api, idx, &from_snapshots(&snapshots[..2])).is_err());
    }

    /// Codegen leaves out ww_uart and ww_si definitions, putting them back from snapshots gives the same API.
    #[test]
    fn known_definitions_are_left_out_and_put_back() {
        use shrink_wrap::SerializeShrinkWrapOwned;
        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/uart_api");
        let full = crate::load(&crate_path, None, false).unwrap();
        let sent = skip_known(&full).unwrap();
        assert!(
            sent.traits
                .iter()
                .all(|l| matches!(l, ApiLevelLocationOwned::SkippedFullVersion { .. }))
        );
        assert!(
            sent.types
                .iter()
                .all(|l| matches!(l, TypeLocationOwned::SkippedFullVersion { .. }))
        );
        assert!(sent.to_ww_bytes_owned().unwrap().len() < full.to_ww_bytes_owned().unwrap().len());

        let mut inlined = sent.clone();
        let not_inlined = wire_weaver_snapshots::inline_skipped(&mut inlined);
        assert!(not_inlined.is_empty(), "{not_inlined:?}");
        assert_eq!(root_signature(&inlined), root_signature(&full));
    }

    /// A definition that differs from its snapshot (e.g., changed without a version bump) is kept in-line.
    #[test]
    fn changed_definition_is_kept_in_line() {
        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/uart_api");
        let mut full = crate::load(&crate_path, None, false).unwrap();
        let ApiLevelLocationOwned::InLine { level, .. } = &mut full.traits[0] else {
            panic!("expected an in-line trait");
        };
        level.docs.push("changed".into());
        let sent = skip_known(&full).unwrap();
        assert!(matches!(
            sent.traits[0],
            ApiLevelLocationOwned::InLine { .. }
        ));
    }

    /// Signature of the API root, as if it was a trait.
    fn root_signature(bundle: &ApiBundleOwned) -> Vec<u8> {
        let mut bundle = bundle.clone();
        bundle.traits.push(ApiLevelLocationOwned::InLine {
            level: bundle.root.clone(),
            crate_idx: bundle.root.crate_idx,
        });
        let idx = bundle.traits.len() as u32 - 1;
        trait_signature(&bundle, idx, &no_resolve).unwrap()
    }

    /// Snapshots are saved for the current source of each crate, and the embedded copies are the same.
    #[test]
    fn snapshots_are_up_to_date() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut crate_paths = vec![root.join("ww_global")];
        for entry in std::fs::read_dir(root.join("ww_stdlib")).unwrap() {
            let path = entry.unwrap().path();
            // ww_client_server is the protocol itself, not used in APIs
            if path.join("Cargo.toml").exists() && !path.ends_with("ww_client_server") {
                crate_paths.push(path);
            }
        }
        let hint =
            "run `just save-snapshots` (with --force for versions that were never published)";
        let embedded = wire_weaver_snapshots::files();
        let mut saved_count = 0;
        for crate_path in crate_paths {
            let bundle = load_crate(&crate_path).unwrap();
            let file_name = format!("{}.ron", bundle.ext_crates[0].filename_friendly());
            let saved = std::fs::read_to_string(crate_path.join("api_snapshots").join(&file_name))
                .unwrap_or_else(|e| panic!("{file_name}: {e}, {hint}"));
            let mut saved: ApiBundleOwned = ron::from_str(&saved).unwrap();
            // records which ww_self saved it, doesn't make a snapshot out of date
            saved.ww_self_version = bundle.ww_self_version;
            assert_eq!(
                ron::to_string(&saved).unwrap(),
                ron::to_string(&bundle).unwrap(),
                "{file_name} is out of date, {hint}"
            );
            for entry in std::fs::read_dir(crate_path.join("api_snapshots")).unwrap() {
                let path = entry.unwrap().path();
                let file_name = path.file_name().unwrap().to_str().unwrap();
                let (_, contents) = embedded
                    .iter()
                    .find(|(name, _)| *name == file_name)
                    .unwrap_or_else(|| panic!("{file_name} is not embedded, {hint}"));
                assert_eq!(
                    *contents,
                    std::fs::read_to_string(&path).unwrap(),
                    "embedded {file_name} differs, {hint}"
                );
                saved_count += 1;
            }
        }
        assert_eq!(
            saved_count,
            embedded.len(),
            "stale snapshots are embedded, {hint}"
        );
    }
}
