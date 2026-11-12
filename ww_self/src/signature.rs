//! Signatures of traits and types, stored in `Skipped*` locations, so that a definition restored from elsewhere
//! (e.g., a saved crate snapshot) can be checked to be the one that was left out (EVO-69).
//!
//! A definition has two signatures, both hashes of a canonical, stand-alone bundle holding the definition and
//! everything it refers to in-line (as if nothing was skipped):
//!
//! * the **shape signature**, with every doc comment left out: the definition's identity, its names, fields and wire
//!   shape. Snapshots, devices and the checker match definitions by it, so a doc fix changes no identity;
//! * the **signature with docs**, over the same bundle with its docs: it answers whether the text changed too. Under
//!   the same shape signature it differs exactly when some doc comment differs. Before EVO-69 this was the only
//!   signature, so a reference saved then holds it.
//!
//! A reference (the `signature` bytes of a `Skipped*` location) is the shape signature, followed by the signature
//! with docs where the bundle carries docs ([Signatures::reference]): 16 bytes in snapshots and in introspection data
//! with docs, 8 bytes without docs ([drop_docs]). An 8-byte reference saved before EVO-69 is a signature with docs:
//! [Signatures::matches] takes either. Nothing about a reference is trusted beyond that: whoever holds a definition (a
//! snapshot, a superseded definition, a definition cache entry) computes both signatures from it.
//!
//! Indices are assigned in the order of first use, starting from the definition itself, so neither signature depends
//! on where the definition was in the original bundle, or on what else was in it.

use crate::visit_mut::{self, VisitMut};
use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    ItemEnumOwned, ItemStructOwned, Multiplicity, TypeLocationOwned, TypeOwned,
};
use crate::{ApiLevel, ApiLevelLocation, TypeLocation};
use anyhow::{Result, anyhow};
use sha2::Digest;
use shrink_wrap::prelude::*;
use shrink_wrap::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned, UNib32};
use std::collections::{HashMap, VecDeque};
use ww_version::{FullVersion, WireVersion};
use ww_version::{FullVersionOwned, VersionTriplet};

/// The canonical bundle a signature hashes: [crate::ApiBundle] as it was when signatures were last allowed to move
/// (up to `wire`, EVO-24), frozen. A field appended to `ApiBundle` later (`ext_wire`, EVO-26) is the bundle format's,
/// not the definition's, and must not move every signature: only the definitions here are hashed, and every
/// field after them is always at its default. The bytes are exactly those of an `ApiBundle` of that time.
#[derive_shrink_wrap(owned(feature = "std"))]
#[allow(dead_code)]
struct CanonicalBundle<'i> {
    magic: u32,
    ww_self_version: VersionTriplet,
    root: ApiLevel<'i>,
    types: RefVec<'i, TypeLocation<'i>>,
    traits: RefVec<'i, ApiLevelLocation<'i>>,
    ext_crates: RefVec<'i, FullVersion<'i>>,
    #[default = None]
    root_omitted: Option<RefVec<'i, UNib32>>,
    #[default = None]
    wire: Option<WireVersion>,
}

/// Fields of the model a [Canonical] bundle clears, so they don't count in signatures: host-derived ones, the
/// registry id, and what a marker means (INT-13). `(owner, field)` with the owner's serde name (`ItemEnumOwned`).
const LEFT_OUT: &[(&str, &str)] = &[
    ("ApiLevelOwned", "compact_id"),
    ("ItemStructOwned", "instance_of"),
    ("ItemStructOwned", "extra"),
    ("ItemEnumOwned", "instance_of"),
    ("ItemEnumOwned", "future_version"),
];

/// Whether the field `field` of the model struct `owner` (its serde name, `ArgumentOwned`) counts in signatures:
/// a snapshot saved before such a field existed has signatures made over other bytes.
pub fn includes(owner: &str, field: &str) -> bool {
    !LEFT_OUT.contains(&(owner, field))
}

/// Length in bytes of one signature (truncated SHA-256), same as the API hash.
pub const SIGNATURE_LEN: usize = 8;

/// API hash (`API_HASH_NO_DOCS` / `API_HASH_WITH_DOCS`) of a serialized bundle, as a device sends it:
/// truncated SHA-256 over the bytes, `ww_self_version` included, so it changes with the introspection data.
pub fn api_hash(bundle_bytes: &[u8]) -> [u8; SIGNATURE_LEN] {
    let mut hash = [0u8; SIGNATURE_LEN];
    hash.copy_from_slice(&sha2::Sha256::digest(bundle_bytes)[..SIGNATURE_LEN]);
    hash
}

/// The two signatures of a definition, see the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signatures {
    /// Without docs: the definition's identity.
    pub shape: Vec<u8>,
    /// With docs, as every signature was before EVO-69.
    pub with_docs: Vec<u8>,
}

/// How a reference relates to a definition, see [Signatures::matches].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Match {
    /// Another definition.
    No,
    /// The same shape and the same docs.
    Exact,
    /// The same shape, other docs.
    DocsDiffer,
    /// The same shape; the reference carries no docs to compare (introspection data without docs).
    Shape,
}

impl Match {
    /// Whether the reference names this definition, docs aside.
    pub fn is_same_shape(self) -> bool {
        self != Match::No
    }
}

impl Signatures {
    /// The bytes a `Skipped*` location stores: the shape signature, followed by the signature with docs when
    /// `with_docs`.
    pub fn reference(&self, with_docs: bool) -> Vec<u8> {
        let mut reference = self.shape.clone();
        if with_docs {
            reference.extend_from_slice(&self.with_docs);
        }
        reference
    }

    /// How `reference` relates to this definition. 16 bytes: shape, then docs. 8 bytes: a shape signature, or a
    /// signature with docs saved before EVO-69 (equal to [Self::with_docs] only for the same docs).
    pub fn matches(&self, reference: &[u8]) -> Match {
        match reference.len() {
            n if n == 2 * SIGNATURE_LEN => {
                let (shape, docs) = reference.split_at(SIGNATURE_LEN);
                if shape != self.shape {
                    Match::No
                } else if docs == self.with_docs {
                    Match::Exact
                } else {
                    Match::DocsDiffer
                }
            }
            SIGNATURE_LEN if reference == self.with_docs => Match::Exact,
            SIGNATURE_LEN if reference == self.shape => Match::Shape,
            _ => Match::No,
        }
    }
}

/// The shape part of a reference: what names the definition, docs aside. An 8-byte reference saved before EVO-69 is
/// a signature with docs, so two references compare by this only when both are of EVO-69 or later.
pub fn shape_part(reference: &[u8]) -> &[u8] {
    &reference[..reference.len().min(SIGNATURE_LEN)]
}

/// The docs part of a reference, `None` for an 8-byte one.
pub fn docs_part(reference: &[u8]) -> Option<&[u8]> {
    (reference.len() == 2 * SIGNATURE_LEN).then(|| &reference[SIGNATURE_LEN..])
}

/// Every doc comment of `bundle` cleared and every reference cut to its shape signature: the bundle a device sends
/// without docs, and the one `API_HASH_NO_DOCS` hashes. A dependency's doc fix then changes neither.
pub fn drop_docs(bundle: &mut ApiBundleOwned) {
    struct Clear;
    impl VisitMut for Clear {
        fn visit_docs(&mut self, node: &mut Vec<String>) {
            node.clear();
        }
    }
    Clear.visit_api_bundle(bundle);
    for location in &mut bundle.types {
        if let TypeLocationOwned::SkippedFullVersion { signature, .. } = location {
            signature.truncate(SIGNATURE_LEN);
        }
    }
    for location in &mut bundle.traits {
        if let ApiLevelLocationOwned::SkippedFullVersion { signature, .. }
        | ApiLevelLocationOwned::SkippedCompactVersion { signature, .. } = location
        {
            signature.truncate(SIGNATURE_LEN);
        }
    }
}

/// Finds a bundle with in-line definitions of a crate's traits and types (e.g., that crate's snapshot),
/// used to look up definitions that are skipped.
pub type Resolve<'a, 'r> = &'r dyn Fn(&FullVersionOwned) -> Option<&'a ApiBundleOwned>;

/// Resolver that finds nothing, for bundles with all the needed definitions in-line.
pub fn no_resolve<'a>(_: &FullVersionOwned) -> Option<&'a ApiBundleOwned> {
    None
}

/// Signatures of `bundle.traits[trait_idx]`.
///
/// Skipped definitions (including the trait itself) are looked up with `resolve`, fails if one is not found.
pub fn trait_signatures<'a>(
    bundle: &'a ApiBundleOwned,
    trait_idx: u32,
    resolve: Resolve<'a, '_>,
) -> Result<Signatures> {
    signatures_of(&canonical_trait_bytes(bundle, trait_idx, resolve)?)
}

/// Signatures of `bundle.types[type_idx]`.
///
/// Skipped definitions (including the type itself) are looked up with `resolve`, fails if one is not found.
pub fn type_signatures<'a>(
    bundle: &'a ApiBundleOwned,
    type_idx: u32,
    resolve: Resolve<'a, '_>,
) -> Result<Signatures> {
    signatures_of(&canonical_type_bytes(bundle, type_idx, resolve)?)
}

/// Serialized canonical bundle of `bundle.traits[trait_idx]`, with docs, the bytes its signature with docs is the
/// hash of: the trait is `traits[0]`, everything it refers to is in-line. See [crate::definition_cache].
pub fn canonical_trait_bytes<'a>(
    bundle: &'a ApiBundleOwned,
    trait_idx: u32,
    resolve: Resolve<'a, '_>,
) -> Result<Vec<u8>> {
    let mut canonical = Canonical::new(bundle, resolve);
    canonical.trait_idx(trait_idx)?;
    canonical.bytes()
}

/// Serialized canonical bundle of `bundle.types[type_idx]`, with docs, the bytes its signature with docs is the
/// hash of: the type is `types[0]`, everything it refers to is in-line. See [crate::definition_cache].
pub fn canonical_type_bytes<'a>(
    bundle: &'a ApiBundleOwned,
    type_idx: u32,
    resolve: Resolve<'a, '_>,
) -> Result<Vec<u8>> {
    let mut canonical = Canonical::new(bundle, resolve);
    canonical.type_idx(type_idx)?;
    canonical.bytes()
}

/// Signatures of a definition from its [canonical bytes](canonical_type_bytes): the signature with docs hashes the
/// bytes as they are (also those an older `ww_self` wrote), the shape signature the same bundle read back with its
/// docs left out.
pub fn signatures_of(canonical_bytes: &[u8]) -> Result<Signatures> {
    let mut bundle = CanonicalBundleOwned::from_ww_bytes_owned(canonical_bytes)
        .map_err(|e| anyhow!("reading a canonical bundle: {e:?}"))?;
    bundle.drop_docs();
    let shape = bundle
        .to_ww_bytes_owned()
        .map_err(|e| anyhow!("serializing canonical bundle: {e:?}"))?;
    Ok(Signatures {
        shape: hash(&shape),
        with_docs: hash(canonical_bytes),
    })
}

fn hash(bytes: &[u8]) -> Vec<u8> {
    sha2::Sha256::digest(bytes)[..SIGNATURE_LEN].to_vec()
}

impl CanonicalBundleOwned {
    /// Clears the docs of every definition in it: they are all in-line, so no reference is left to cut.
    fn drop_docs(&mut self) {
        struct Clear;
        impl VisitMut for Clear {
            fn visit_docs(&mut self, node: &mut Vec<String>) {
                node.clear();
            }
        }
        Clear.visit_api_level(&mut self.root);
        for location in &mut self.types {
            Clear.visit_type_location(location);
        }
        for location in &mut self.traits {
            Clear.visit_api_level_location(location);
        }
    }
}

/// Copies definitions into a new bundle, remapping indices in the order of first use.
///
/// Definitions can come from different bundles (when skipped ones are resolved), indices in them are relative to the
/// bundle they came from, so the maps are keyed by bundle address as well.
struct Canonical<'a, 'r> {
    /// Bundle that indices in the definition being remapped refer to.
    src: &'a ApiBundleOwned,
    resolve: Resolve<'a, 'r>,
    types: HashMap<(usize, u32), u32>,
    traits: HashMap<(usize, u32), u32>,
    crates: HashMap<FullVersionOwned, u32>,
    out_types: Vec<Option<TypeLocationOwned>>,
    out_traits: Vec<Option<ApiLevelLocationOwned>>,
    out_crates: Vec<FullVersionOwned>,
    pending: VecDeque<Pending<'a>>,
    error: Option<anyhow::Error>,
}

enum Pending<'a> {
    Type(u32, &'a ApiBundleOwned, TypeLocationOwned),
    Trait(u32, &'a ApiBundleOwned, ApiLevelLocationOwned),
}

fn key(bundle: &ApiBundleOwned, idx: u32) -> (usize, u32) {
    (bundle as *const ApiBundleOwned as usize, idx)
}

impl<'a, 'r> Canonical<'a, 'r> {
    fn new(src: &'a ApiBundleOwned, resolve: Resolve<'a, 'r>) -> Self {
        Canonical {
            src,
            resolve,
            types: HashMap::new(),
            traits: HashMap::new(),
            crates: HashMap::new(),
            out_types: vec![],
            out_traits: vec![],
            out_crates: vec![],
            pending: VecDeque::new(),
            error: None,
        }
    }

    fn type_idx(&mut self, idx: u32) -> Result<u32> {
        let (src, idx) = self.resolve_type(self.src, idx)?;
        if let Some(new) = self.types.get(&key(src, idx)) {
            return Ok(*new);
        }
        let new = self.out_types.len() as u32;
        self.types.insert(key(src, idx), new);
        self.out_types.push(None);
        let location = src.types[idx as usize].clone();
        self.pending.push_back(Pending::Type(new, src, location));
        Ok(new)
    }

    fn trait_idx(&mut self, idx: u32) -> Result<u32> {
        let (src, idx) = self.resolve_trait(self.src, idx)?;
        if let Some(new) = self.traits.get(&key(src, idx)) {
            return Ok(*new);
        }
        let new = self.out_traits.len() as u32;
        self.traits.insert(key(src, idx), new);
        self.out_traits.push(None);
        let location = src.traits[idx as usize].clone();
        self.pending.push_back(Pending::Trait(new, src, location));
        Ok(new)
    }

    fn crate_idx(&mut self, idx: u32) -> Result<u32> {
        let version = self.src.crate_version(idx)?;
        if let Some(new) = self.crates.get(version) {
            return Ok(*new);
        }
        let new = self.out_crates.len() as u32;
        self.out_crates.push(version.clone());
        self.crates.insert(version.clone(), new);
        Ok(new)
    }

    /// Bundle and index of the in-line definition of `bundle.types[idx]`.
    fn resolve_type(
        &self,
        bundle: &'a ApiBundleOwned,
        idx: u32,
    ) -> Result<(&'a ApiBundleOwned, u32)> {
        let (crate_idx, name) = match bundle.types.get(idx as usize) {
            Some(TypeLocationOwned::InLine { .. }) => return Ok((bundle, idx)),
            Some(TypeLocationOwned::SkippedFullVersion {
                crate_idx,
                type_name,
                ..
            }) => (crate_idx.0, type_name),
            None => return Err(anyhow!("Bad ApiBundle: no type with index: {idx}")),
        };
        let version = bundle.crate_version(crate_idx)?;
        let not_found = || {
            anyhow!(
                "definition of {}::{name} is needed to calculate a signature",
                version.crate_id
            )
        };
        let other = (self.resolve)(version).ok_or_else(not_found)?;
        let found = other.types.iter().position(|location| {
            let TypeLocationOwned::InLine { ty, .. } = location else {
                return false;
            };
            let (crate_idx, qualified_name) = match ty {
                TypeOwned::Struct(s) => (s.crate_idx, s.qualified_name()),
                TypeOwned::Enum(e) => (e.crate_idx, e.qualified_name()),
                _ => return false,
            };
            qualified_name == *name
                && other
                    .crate_version(crate_idx.0)
                    .is_ok_and(|v| v.crate_id == version.crate_id)
        });
        Ok((other, found.ok_or_else(not_found)? as u32))
    }

    /// Bundle and index of the in-line definition of `bundle.traits[idx]`.
    fn resolve_trait(
        &self,
        bundle: &'a ApiBundleOwned,
        idx: u32,
    ) -> Result<(&'a ApiBundleOwned, u32)> {
        let (crate_idx, name) = match bundle.traits.get(idx as usize) {
            Some(ApiLevelLocationOwned::InLine { .. }) => return Ok((bundle, idx)),
            Some(ApiLevelLocationOwned::SkippedFullVersion {
                crate_idx,
                trait_name,
                ..
            }) => (crate_idx.0, trait_name),
            Some(ApiLevelLocationOwned::SkippedCompactVersion { .. }) => {
                return Err(anyhow!(
                    "trait with index {idx} is referred to by a compact version, can't calculate a signature"
                ));
            }
            None => return Err(anyhow!("Bad ApiBundle: no trait with index: {idx}")),
        };
        let version = bundle.crate_version(crate_idx)?;
        let not_found = || {
            anyhow!(
                "definition of {}::{name} is needed to calculate a signature",
                version.crate_id
            )
        };
        let other = (self.resolve)(version).ok_or_else(not_found)?;
        let found = other.traits.iter().position(|location| {
            let ApiLevelLocationOwned::InLine { level, crate_idx } = location else {
                return false;
            };
            &level.trait_name == name
                && other
                    .crate_version(crate_idx.0)
                    .is_ok_and(|v| v.crate_id == version.crate_id)
        });
        Ok((other, found.ok_or_else(not_found)? as u32))
    }

    /// Remaps pending definitions (which can add more) and serializes the resulting bundle.
    fn bytes(mut self) -> Result<Vec<u8>> {
        while let Some(pending) = self.pending.pop_front() {
            match pending {
                Pending::Type(new, src, mut location) => {
                    self.src = src;
                    self.visit_type_location(&mut location);
                    self.out_types[new as usize] = Some(location);
                }
                Pending::Trait(new, src, mut location) => {
                    self.src = src;
                    self.visit_api_level_location(&mut location);
                    self.out_traits[new as usize] = Some(location);
                }
            }
            if let Some(e) = self.error.take() {
                return Err(e);
            }
        }
        let bundle = CanonicalBundleOwned {
            magic: crate::MAGIC,
            // A signature identifies a definition, not the format: a device and a host built with different
            // ww_self versions must compute the same one, or the host can't put the definition back.
            ww_self_version: VersionTriplet::new(0, 0, 0),
            root: ApiLevelOwned {
                docs: vec![],
                crate_idx: UNib32(0),
                trait_name: String::new(),
                compact_id: None,
                meta: None,
                items: vec![],
            },
            types: self.out_types.into_iter().map(Option::unwrap).collect(),
            traits: self.out_traits.into_iter().map(Option::unwrap).collect(),
            ext_crates: self.out_crates,
            root_omitted: None,
            wire: None,
        };
        bundle
            .to_ww_bytes_owned()
            .map_err(|e| anyhow!("serializing canonical bundle: {e:?}"))
    }

    fn remap(&mut self, idx: &mut UNib32, f: fn(&mut Self, u32) -> Result<u32>) {
        if self.error.is_some() {
            return;
        }
        match f(self, idx.0) {
            Ok(new) => idx.0 = new,
            Err(e) => self.error = Some(e),
        }
    }
}

impl VisitMut for Canonical<'_, '_> {
    fn visit_type_location(&mut self, node: &mut TypeLocationOwned) {
        match node {
            TypeLocationOwned::InLine { crate_idx, .. }
            | TypeLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                self.remap(crate_idx, Self::crate_idx)
            }
        }
        visit_mut::visit_type_location(self, node)
    }

    fn visit_api_level_location(&mut self, node: &mut ApiLevelLocationOwned) {
        match node {
            ApiLevelLocationOwned::InLine { crate_idx, .. }
            | ApiLevelLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                self.remap(crate_idx, Self::crate_idx)
            }
            ApiLevelLocationOwned::SkippedCompactVersion { .. } => {}
        }
        visit_mut::visit_api_level_location(self, node)
    }

    fn visit_api_level(&mut self, node: &mut ApiLevelOwned) {
        self.remap(&mut node.crate_idx, Self::crate_idx);
        // the registry id names the trait, it is not part of its definition: assigning one changes no signature
        node.compact_id = None;
        visit_mut::visit_api_level(self, node)
    }

    fn visit_api_item(&mut self, node: &mut ApiItemOwned) {
        if let Multiplicity::Array {
            index_type_idx: Some(type_idx),
        } = &mut node.multiplicity
        {
            self.remap(type_idx, Self::type_idx);
        }
        visit_mut::visit_api_item(self, node)
    }

    fn visit_api_item_kind(&mut self, node: &mut ApiItemKindOwned) {
        if let ApiItemKindOwned::Trait { trait_idx, .. } = node {
            self.remap(trait_idx, Self::trait_idx);
        }
        visit_mut::visit_api_item_kind(self, node)
    }

    fn visit_type(&mut self, node: &mut TypeOwned) {
        if let TypeOwned::OutOfLine { type_idx } | TypeOwned::Instance { type_idx, .. } = node {
            self.remap(type_idx, Self::type_idx);
        }
        visit_mut::visit_type(self, node)
    }

    fn visit_item_struct(&mut self, node: &mut ItemStructOwned) {
        // host-derived, not part of what was sent
        node.instance_of = None;
        // what a marker means, not what is on the wire (INT-13): a marker defined since keeps the signature it had
        // before, so a device built earlier still finds a newer snapshot's definition of `Second` and what uses it
        node.extra = ItemStructOwned::extra_of(node.is_transparent(), None);
        self.remap(&mut node.crate_idx, Self::crate_idx);
        visit_mut::visit_item_struct(self, node)
    }

    fn visit_item_enum(&mut self, node: &mut ItemEnumOwned) {
        // host-derived, not part of what was sent
        node.instance_of = None;
        // the same bytes on the wire either way, and definitions saved before EVO-10 keep their signature
        node.future_version = None;
        self.remap(&mut node.crate_idx, Self::crate_idx);
        visit_mut::visit_item_enum(self, node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FieldOwned, FieldsOwned, Repr, VariantOwned};
    use shrink_wrap::ElementSize;
    use ww_version::VersionOwned;

    /// `struct S { e: E }` and `enum E { V }` of a crate `c`, with `docs` on every definition and member.
    fn bundle(docs: &str) -> ApiBundleOwned {
        let docs = || vec![docs.to_string()];
        let s = TypeOwned::Struct(ItemStructOwned {
            instance_of: None,
            size: ElementSize::Unsized,
            crate_idx: UNib32(0),
            docs: docs(),
            ident: "S".into(),
            path: None,
            generics: vec![],
            fields: FieldsOwned::Named(vec![FieldOwned {
                docs: docs(),
                ident: Some("e".into()),
                default: None,
                since: None,
                ty: TypeOwned::OutOfLine {
                    type_idx: UNib32(1),
                },
                slot: None,
                series_opaque: false,
            }]),
            sparse: false,
            extra: None,
        });
        let e = TypeOwned::Enum(ItemEnumOwned {
            instance_of: None,
            future_version: None,
            size: ElementSize::Unsized,
            repr: Repr::UNib32,
            crate_idx: UNib32(0),
            docs: docs(),
            ident: "E".into(),
            path: None,
            generics: vec![],
            variants: vec![VariantOwned {
                docs: docs(),
                ident: "V".into(),
                fields: FieldsOwned::Unit,
                discriminant: UNib32(0),
                since: None,
            }],
        });
        ApiBundleOwned {
            magic: crate::MAGIC,
            ww_self_version: crate::VERSION,
            root: ApiLevelOwned {
                docs: vec![],
                crate_idx: UNib32(0),
                trait_name: "Root".into(),
                compact_id: None,
                meta: None,
                items: vec![],
            },
            types: vec![
                TypeLocationOwned::InLine {
                    ty: s,
                    crate_idx: UNib32(0),
                },
                TypeLocationOwned::InLine {
                    ty: e,
                    crate_idx: UNib32(0),
                },
            ],
            traits: vec![],
            ext_crates: vec![FullVersionOwned::new(
                "c".into(),
                VersionOwned::new(0, 1, 0),
            )],
            root_omitted: None,
            wire: None,
            ext_wire: None,
        }
    }

    fn signatures(bundle: &ApiBundleOwned) -> Signatures {
        type_signatures(bundle, 0, &no_resolve).unwrap()
    }

    /// Docs move only the signature with docs, also the docs of a type referred to; the shape signature moves with
    /// a name (EVO-69).
    #[test]
    fn shape_leaves_docs_out() {
        let old = signatures(&bundle("old"));
        let new = signatures(&bundle("new"));
        assert_eq!(old.shape, new.shape);
        assert_ne!(old.with_docs, new.with_docs);

        let mut enum_docs = bundle("old");
        if let TypeLocationOwned::InLine {
            ty: TypeOwned::Enum(e),
            ..
        } = &mut enum_docs.types[1]
        {
            e.variants[0].docs = vec!["other".into()];
        }
        let enum_docs = signatures(&enum_docs);
        assert_eq!(enum_docs.shape, old.shape);
        assert_ne!(enum_docs.with_docs, old.with_docs);

        let mut renamed = bundle("old");
        if let TypeLocationOwned::InLine {
            ty: TypeOwned::Enum(e),
            ..
        } = &mut renamed.types[1]
        {
            e.variants[0].ident = "W".into();
        }
        assert_ne!(signatures(&renamed).shape, old.shape);
    }

    /// The signature with docs is the one every reference held before EVO-69: the hash of the canonical bytes.
    #[test]
    fn with_docs_is_the_hash_of_the_canonical_bytes() {
        let b = bundle("docs");
        let bytes = canonical_type_bytes(&b, 0, &no_resolve).unwrap();
        assert_eq!(signatures(&b).with_docs, hash(&bytes));
        assert_eq!(signatures_of(&bytes).unwrap(), signatures(&b));
    }

    #[test]
    fn references_match_by_shape() {
        let old = signatures(&bundle("old"));
        let new = signatures(&bundle("new"));
        let other = Signatures {
            shape: vec![0; SIGNATURE_LEN],
            with_docs: vec![1; SIGNATURE_LEN],
        };
        assert_eq!(old.reference(true).len(), 2 * SIGNATURE_LEN);
        assert_eq!(old.reference(false), old.shape);
        assert_eq!(old.matches(&old.reference(true)), Match::Exact);
        assert_eq!(new.matches(&old.reference(true)), Match::DocsDiffer);
        assert_eq!(new.matches(&old.reference(false)), Match::Shape);
        // saved before EVO-69: the signature with docs alone
        assert_eq!(old.matches(&old.with_docs), Match::Exact);
        assert_eq!(new.matches(&old.with_docs), Match::No);
        assert_eq!(other.matches(&old.reference(true)), Match::No);
        assert_eq!(old.matches(&[]), Match::No);
        assert_eq!(shape_part(&old.reference(true)), old.shape);
        assert_eq!(docs_part(&old.reference(true)), Some(&old.with_docs[..]));
        assert_eq!(docs_part(&old.with_docs), None);
    }

    /// Without docs: no doc comment, and every reference cut to its shape.
    #[test]
    fn drop_docs_cuts_references() {
        let mut b = bundle("docs");
        let reference = signatures(&b).reference(true);
        b.types.push(TypeLocationOwned::SkippedFullVersion {
            crate_idx: UNib32(0),
            type_name: "T".into(),
            signature: reference.clone(),
        });
        drop_docs(&mut b);
        let TypeLocationOwned::SkippedFullVersion { signature, .. } = &b.types[2] else {
            unreachable!()
        };
        assert_eq!(signature, shape_part(&reference));
        let TypeLocationOwned::InLine {
            ty: TypeOwned::Struct(s),
            ..
        } = &b.types[0]
        else {
            unreachable!()
        };
        assert!(s.docs.is_empty());
    }
}
