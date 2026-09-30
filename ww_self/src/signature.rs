//! Signatures of traits and types, stored in `Skipped*` locations, so that a definition restored from elsewhere
//! (e.g., a saved crate snapshot) can be checked to be exactly the one that was left out.
//!
//! A signature is a hash of a canonical, stand-alone bundle holding the definition and everything it refers to,
//! with docs, in-line (as if nothing was skipped). Indices are assigned in the order of first use, starting from the
//! definition itself, so the signature doesn't depend on where the definition was in the original bundle, or on what
//! else was in it.

use crate::visit_mut::{self, VisitMut};
use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    ItemEnumOwned, ItemStructOwned, Multiplicity, TypeLocationOwned, TypeOwned,
};
use anyhow::{Result, anyhow};
use sha2::Digest;
use shrink_wrap::{SerializeShrinkWrapOwned, UNib32};
use std::collections::{HashMap, VecDeque};
use ww_version::FullVersionOwned;

/// Signature length in bytes (truncated SHA-256), same as the API hash.
pub const SIGNATURE_LEN: usize = 8;

/// Finds a bundle with in-line definitions of a crate's traits and types (e.g., that crate's snapshot),
/// used to look up definitions that are skipped.
pub type Resolve<'a, 'r> = &'r dyn Fn(&FullVersionOwned) -> Option<&'a ApiBundleOwned>;

/// Resolver that finds nothing, for bundles with all the needed definitions in-line.
pub fn no_resolve<'a>(_: &FullVersionOwned) -> Option<&'a ApiBundleOwned> {
    None
}

/// Signature of `bundle.traits[trait_idx]`.
///
/// Skipped definitions (including the trait itself) are looked up with `resolve`, fails if one is not found.
pub fn trait_signature<'a>(
    bundle: &'a ApiBundleOwned,
    trait_idx: u32,
    resolve: Resolve<'a, '_>,
) -> Result<Vec<u8>> {
    let mut canonical = Canonical::new(bundle, resolve);
    canonical.trait_idx(trait_idx)?;
    canonical.hash()
}

/// Signature of `bundle.types[type_idx]`.
///
/// Skipped definitions (including the type itself) are looked up with `resolve`, fails if one is not found.
pub fn type_signature<'a>(
    bundle: &'a ApiBundleOwned,
    type_idx: u32,
    resolve: Resolve<'a, '_>,
) -> Result<Vec<u8>> {
    let mut canonical = Canonical::new(bundle, resolve);
    canonical.type_idx(type_idx)?;
    canonical.hash()
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
            let (crate_idx, ident) = match ty {
                TypeOwned::Struct(s) => (s.crate_idx, &s.ident),
                TypeOwned::Enum(e) => (e.crate_idx, &e.ident),
                _ => return false,
            };
            ident == name
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

    /// Remaps pending definitions (which can add more) and hashes the resulting bundle.
    fn hash(mut self) -> Result<Vec<u8>> {
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
        let bundle = ApiBundleOwned {
            magic: crate::MAGIC,
            ww_self_version: crate::VERSION,
            root: ApiLevelOwned {
                docs: vec![],
                crate_idx: UNib32(0),
                trait_name: String::new(),
                items: vec![],
            },
            types: self.out_types.into_iter().map(Option::unwrap).collect(),
            traits: self.out_traits.into_iter().map(Option::unwrap).collect(),
            ext_crates: self.out_crates,
        };
        let bytes = bundle
            .to_ww_bytes_owned()
            .map_err(|e| anyhow!("serializing canonical bundle: {e:?}"))?;
        Ok(sha2::Sha256::digest(&bytes)[..SIGNATURE_LEN].to_vec())
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
        if let ApiItemKindOwned::Trait { trait_idx } = node {
            self.remap(trait_idx, Self::trait_idx);
        }
        visit_mut::visit_api_item_kind(self, node)
    }

    fn visit_type(&mut self, node: &mut TypeOwned) {
        if let TypeOwned::OutOfLine { type_idx } = node {
            self.remap(type_idx, Self::type_idx);
        }
        visit_mut::visit_type(self, node)
    }

    fn visit_item_struct(&mut self, node: &mut ItemStructOwned) {
        self.remap(&mut node.crate_idx, Self::crate_idx);
        visit_mut::visit_item_struct(self, node)
    }

    fn visit_item_enum(&mut self, node: &mut ItemEnumOwned) {
        self.remap(&mut node.crate_idx, Self::crate_idx);
        visit_mut::visit_item_enum(self, node)
    }
}
