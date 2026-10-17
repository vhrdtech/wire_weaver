//! Putting definitions of skipped traits and types back into a bundle, e.g. from crate snapshots, so that it looks as
//! if nothing was skipped.
//!
//! Each definition is only put back if its [signature](crate::signature) matches the one stored in the skipped
//! location. A definition whose signature the snapshot doesn't have (a build from another revision of a crate that
//! is not published yet) can still be found by signature in the [definition cache](crate::definition_cache). Indices differ from a bundle that had everything in-line from the start: definitions put back replace
//! the skipped locations in place, and the ones they refer to are appended.

use crate::definition_cache::{self, Kind};
use crate::signature::{Resolve, no_resolve, trait_signature, type_signature};
use crate::visit_mut::{self, VisitMut};
use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    ItemEnumOwned, ItemStructOwned, Multiplicity, TypeLocationOwned, TypeOwned,
};
use anyhow::{Result, anyhow};
use core::fmt::{Display, Formatter};
use shrink_wrap::UNib32;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use ww_version::FullVersionOwned;

/// A skipped trait or type that was not put back.
#[derive(Debug)]
pub struct NotInlined {
    /// "trait" or "type"
    pub kind: &'static str,
    pub crate_version: Option<FullVersionOwned>,
    pub name: String,
    pub reason: anyhow::Error,
}

impl Display for NotInlined {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match &self.crate_version {
            Some(v) => {
                let (major, minor, patch) =
                    (v.version.major.0, v.version.minor.0, v.version.patch.0);
                write!(
                    f,
                    "{} {}::{} {major}.{minor}.{patch}: {:#}",
                    self.kind, v.crate_id, self.name, self.reason
                )
            }
            None => write!(f, "{} {}: {:#}", self.kind, self.name, self.reason),
        }
    }
}

/// A skipped trait or type that was put back from the [definition cache](crate::definition_cache): the snapshot has
/// another definition under the same crate version, or none.
#[derive(Debug)]
pub struct FromCache {
    pub kind: Kind,
    pub crate_version: FullVersionOwned,
    pub name: String,
    pub signature: Vec<u8>,
    /// Signature of the snapshot's definition, `None` without one.
    pub snapshot_signature: Option<Vec<u8>>,
}

impl Display for FromCache {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let v = &self.crate_version;
        let (major, minor, patch) = (v.version.major.0, v.version.minor.0, v.version.patch.0);
        write!(
            f,
            "{} {}::{} {major}.{minor}.{patch} with signature {} ",
            self.kind.name(),
            v.crate_id,
            self.name,
            hex(&self.signature)
        )?;
        match &self.snapshot_signature {
            Some(snapshot) => write!(f, "(this host's is {})", hex(snapshot)),
            None => write!(f, "(this host doesn't have it)"),
        }
    }
}

/// What [inline_skipped_or_cached] did.
#[derive(Debug, Default)]
pub struct Inlined {
    /// Left skipped: not found, or found with a different signature and not in the cache.
    pub not_inlined: Vec<NotInlined>,
    /// Put back from the definition cache.
    pub from_cache: Vec<FromCache>,
}

/// Replace skipped traits and types with their definitions found with `resolve`, together with everything they refer
/// to. Returns the ones that are left skipped: not found, or found with a different signature.
pub fn inline_skipped<'a>(
    bundle: &mut ApiBundleOwned,
    resolve: Resolve<'a, '_>,
) -> Vec<NotInlined> {
    inline_skipped_or_cached(bundle, resolve, None).not_inlined
}

/// Same as [inline_skipped], a definition that `resolve` doesn't find with the same signature is looked up in the
/// [definition cache](crate::definition_cache) in `cache_dir`.
pub fn inline_skipped_or_cached<'a>(
    bundle: &mut ApiBundleOwned,
    resolve: Resolve<'a, '_>,
    cache_dir: Option<&Path>,
) -> Inlined {
    let mut from_cache = vec![];
    let mut not_inlined = vec![];
    let mut failed_types = HashSet::new();
    let mut failed_traits = HashSet::new();
    // definitions put back can append more skipped ones
    loop {
        let mut progress = false;
        for idx in 0..bundle.types.len() {
            let TypeLocationOwned::SkippedFullVersion { type_name, .. } = &bundle.types[idx] else {
                continue;
            };
            if failed_types.contains(&idx) {
                continue;
            }
            let name = type_name.clone();
            match inline_type(bundle, idx, resolve, cache_dir) {
                Ok(cached) => {
                    progress = true;
                    from_cache.extend(cached);
                }
                Err(reason) => {
                    failed_types.insert(idx);
                    not_inlined.push(NotInlined {
                        kind: "type",
                        crate_version: skipped_crate(bundle, &bundle.types[idx]),
                        name,
                        reason,
                    });
                }
            }
        }
        for idx in 0..bundle.traits.len() {
            let ApiLevelLocationOwned::SkippedFullVersion { trait_name, .. } = &bundle.traits[idx]
            else {
                continue;
            };
            if failed_traits.contains(&idx) {
                continue;
            }
            let name = trait_name.clone();
            match inline_trait(bundle, idx, resolve, cache_dir) {
                Ok(cached) => {
                    progress = true;
                    from_cache.extend(cached);
                }
                Err(reason) => {
                    failed_traits.insert(idx);
                    let crate_version = match &bundle.traits[idx] {
                        ApiLevelLocationOwned::SkippedFullVersion { crate_idx, .. } => {
                            bundle.crate_version(crate_idx.0).ok().cloned()
                        }
                        _ => None,
                    };
                    not_inlined.push(NotInlined {
                        kind: "trait",
                        crate_version,
                        name,
                        reason,
                    });
                }
            }
        }
        if !progress {
            return Inlined {
                not_inlined,
                from_cache,
            };
        }
    }
}

fn skipped_crate(
    bundle: &ApiBundleOwned,
    location: &TypeLocationOwned,
) -> Option<FullVersionOwned> {
    match location {
        TypeLocationOwned::SkippedFullVersion { crate_idx, .. } => {
            bundle.crate_version(crate_idx.0).ok().cloned()
        }
        TypeLocationOwned::InLine { .. } => None,
    }
}

fn inline_type<'a>(
    bundle: &mut ApiBundleOwned,
    idx: usize,
    resolve: Resolve<'a, '_>,
    cache_dir: Option<&Path>,
) -> Result<Option<FromCache>> {
    let TypeLocationOwned::SkippedFullVersion {
        crate_idx,
        type_name,
        signature,
    } = &bundle.types[idx]
    else {
        return Ok(None);
    };
    let (type_name, signature) = (type_name.clone(), signature.clone());
    let version = bundle.crate_version(crate_idx.0)?.clone();
    let found = resolve(&version)
        .and_then(|src| Some((src, find_type(src, &version, &type_name)?)))
        .map(|(src, src_idx)| (src, src_idx, type_signature(src, src_idx, resolve)));
    let snapshot_signature = match &found {
        Some((_, _, Ok(found_signature))) if *found_signature == signature => None,
        Some((_, _, Ok(found_signature))) => Some(found_signature.clone()),
        _ => None,
    };
    if let Some((src, src_idx, found_signature)) = found {
        let found_signature = found_signature?;
        if found_signature == signature || cache_dir.is_none() {
            check_signature(&signature, &found_signature)?;
            let mut location = src.types[src_idx as usize].clone();
            let mut import = Import::new(bundle, src, resolve);
            import.visit_type_location(&mut location);
            import.finish()?;
            bundle.types[idx] = location;
            return Ok(None);
        }
    }
    let not_found = || match &snapshot_signature {
        Some(snapshot) => anyhow!(
            "found definition is different (signature {} instead of {})",
            hex(snapshot),
            hex(&signature)
        ),
        None => anyhow!("definition not found"),
    };
    let Some(dir) = cache_dir else {
        return Err(not_found());
    };
    let Some(src) = definition_cache::load(dir, Kind::Type, &version, &type_name, &signature)?
    else {
        return Err(not_found());
    };
    let mut location = src.types[0].clone();
    let mut import = Import::new(bundle, &src, &no_resolve).inline_all();
    import.visit_type_location(&mut location);
    import.finish()?;
    bundle.types[idx] = location;
    Ok(Some(FromCache {
        kind: Kind::Type,
        crate_version: version,
        name: type_name,
        signature,
        snapshot_signature,
    }))
}

fn inline_trait<'a>(
    bundle: &mut ApiBundleOwned,
    idx: usize,
    resolve: Resolve<'a, '_>,
    cache_dir: Option<&Path>,
) -> Result<Option<FromCache>> {
    let ApiLevelLocationOwned::SkippedFullVersion {
        crate_idx,
        trait_name,
        signature,
    } = &bundle.traits[idx]
    else {
        return Ok(None);
    };
    let (trait_name, signature) = (trait_name.clone(), signature.clone());
    let version = bundle.crate_version(crate_idx.0)?.clone();
    let found = resolve(&version)
        .and_then(|src| Some((src, find_trait(src, &version, &trait_name)?)))
        .map(|(src, src_idx)| (src, src_idx, trait_signature(src, src_idx, resolve)));
    let snapshot_signature = match &found {
        Some((_, _, Ok(found_signature))) if *found_signature == signature => None,
        Some((_, _, Ok(found_signature))) => Some(found_signature.clone()),
        _ => None,
    };
    if let Some((src, src_idx, found_signature)) = found {
        let found_signature = found_signature?;
        if found_signature == signature || cache_dir.is_none() {
            check_signature(&signature, &found_signature)?;
            let mut location = src.traits[src_idx as usize].clone();
            let mut import = Import::new(bundle, src, resolve);
            import.visit_api_level_location(&mut location);
            import.finish()?;
            bundle.traits[idx] = location;
            return Ok(None);
        }
    }
    let not_found = || match &snapshot_signature {
        Some(snapshot) => anyhow!(
            "found definition is different (signature {} instead of {})",
            hex(snapshot),
            hex(&signature)
        ),
        None => anyhow!("definition not found"),
    };
    let Some(dir) = cache_dir else {
        return Err(not_found());
    };
    let Some(src) = definition_cache::load(dir, Kind::Trait, &version, &trait_name, &signature)?
    else {
        return Err(not_found());
    };
    let mut location = src.traits[0].clone();
    let mut import = Import::new(bundle, &src, &no_resolve).inline_all();
    import.visit_api_level_location(&mut location);
    import.finish()?;
    bundle.traits[idx] = location;
    Ok(Some(FromCache {
        kind: Kind::Trait,
        crate_version: version,
        name: trait_name,
        signature,
        snapshot_signature,
    }))
}

fn check_signature(expected: &[u8], found: &[u8]) -> Result<()> {
    if expected.is_empty() {
        return Err(anyhow!("no signature to check the definition against"));
    }
    if expected != found {
        return Err(anyhow!(
            "found definition is different (signature {} instead of {}), likely changed without bumping the crate version",
            hex(found),
            hex(expected)
        ));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Crate version and qualified name (`units::Volt`, API-46) of a user-defined type (in-line struct or enum, or
/// skipped).
fn type_key<'b>(
    bundle: &'b ApiBundleOwned,
    location: &'b TypeLocationOwned,
) -> Option<(&'b FullVersionOwned, String)> {
    let (crate_idx, name) = match location {
        TypeLocationOwned::InLine {
            ty: TypeOwned::Struct(s),
            crate_idx,
        } => (crate_idx, s.qualified_name()),
        TypeLocationOwned::InLine {
            ty: TypeOwned::Enum(e),
            crate_idx,
        } => (crate_idx, e.qualified_name()),
        TypeLocationOwned::SkippedFullVersion {
            crate_idx,
            type_name,
            ..
        } => (crate_idx, type_name.clone()),
        TypeLocationOwned::InLine { .. } => return None,
    };
    Some((bundle.crate_version(crate_idx.0).ok()?, name))
}

/// Crate version and name of a trait (in-line or skipped by full version).
fn trait_key<'b>(
    bundle: &'b ApiBundleOwned,
    location: &'b ApiLevelLocationOwned,
) -> Option<(&'b FullVersionOwned, &'b str)> {
    let (crate_idx, name) = match location {
        ApiLevelLocationOwned::InLine { level, crate_idx } => (crate_idx, &level.trait_name),
        ApiLevelLocationOwned::SkippedFullVersion {
            crate_idx,
            trait_name,
            ..
        } => (crate_idx, trait_name),
        ApiLevelLocationOwned::SkippedCompactVersion { .. } => return None,
    };
    Some((bundle.crate_version(crate_idx.0).ok()?, name))
}

/// Index of the in-line definition of a type in `bundle`.
fn find_type(bundle: &ApiBundleOwned, version: &FullVersionOwned, name: &str) -> Option<u32> {
    bundle
        .types
        .iter()
        .position(|location| {
            matches!(location, TypeLocationOwned::InLine { .. })
                && type_key(bundle, location).is_some_and(|(v, n)| v == version && n == name)
        })
        .map(|idx| idx as u32)
}

/// Index of the in-line definition of a trait in `bundle`.
fn find_trait(bundle: &ApiBundleOwned, version: &FullVersionOwned, name: &str) -> Option<u32> {
    bundle
        .traits
        .iter()
        .position(|location| {
            matches!(location, ApiLevelLocationOwned::InLine { .. })
                && trait_key(bundle, location) == Some((version, name))
        })
        .map(|idx| idx as u32)
}

/// Remaps indices of a definition copied from `src` into `dst`: crates, traits and user-defined types are matched by
/// crate version and name, missing ones are appended (traits and types as skipped, to be put back later).
struct Import<'d, 'a, 'r> {
    dst: &'d mut ApiBundleOwned,
    src: &'a ApiBundleOwned,
    resolve: Resolve<'a, 'r>,
    /// Definitions missing from `dst` are copied in-line instead of appended as skipped: `src` is a cached
    /// definition, its parts are not in any snapshot under their signatures.
    inline_all: bool,
    /// Entries of `src` that are not user-defined, copied as they are: index in `dst`, also while being copied.
    copied: HashMap<u32, u32>,
    error: Option<anyhow::Error>,
}

impl<'d, 'a, 'r> Import<'d, 'a, 'r> {
    fn new(dst: &'d mut ApiBundleOwned, src: &'a ApiBundleOwned, resolve: Resolve<'a, 'r>) -> Self {
        Import {
            dst,
            src,
            resolve,
            inline_all: false,
            copied: HashMap::new(),
            error: None,
        }
    }

    fn inline_all(mut self) -> Self {
        self.inline_all = true;
        self
    }

    fn finish(self) -> Result<()> {
        match self.error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn crate_idx(&mut self, idx: u32) -> Result<u32> {
        let version = self.src.crate_version(idx)?;
        if let Some(found) = self.dst.ext_crates.iter().position(|v| v == version) {
            return Ok(found as u32);
        }
        self.dst.ext_crates.push(version.clone());
        Ok(self.dst.ext_crates.len() as u32 - 1)
    }

    fn type_idx(&mut self, idx: u32) -> Result<u32> {
        let location = self
            .src
            .types
            .get(idx as usize)
            .ok_or_else(|| anyhow!("Bad ApiBundle: no type with index: {idx}"))?;
        let Some((version, name)) = type_key(self.src, location) else {
            // not user-defined (e.g. an array index type), copy as is, once: it can refer to itself (a malformed
            // bundle)
            if let Some(&new) = self.copied.get(&idx) {
                return Ok(new);
            }
            let new = self.dst.types.len();
            self.copied.insert(idx, new as u32);
            self.dst.types.push(location.clone());
            let mut location = location.clone();
            self.visit_type_location(&mut location);
            self.dst.types[new] = location;
            return Ok(new as u32);
        };
        let dst = &*self.dst;
        if let Some(found) = dst
            .types
            .iter()
            .position(|l| type_key(dst, l).is_some_and(|(v, n)| v == version && n == name))
        {
            return Ok(found as u32);
        }
        if self.inline_all && matches!(location, TypeLocationOwned::InLine { .. }) {
            // a placeholder first, so that the definition's references to itself find it
            let crate_idx = UNib32(self.crate_idx_of_version(version));
            let new = self.dst.types.len();
            self.dst.types.push(TypeLocationOwned::SkippedFullVersion {
                crate_idx,
                type_name: name.to_string(),
                signature: vec![],
            });
            let mut location = location.clone();
            self.visit_type_location(&mut location);
            self.dst.types[new] = location;
            return Ok(new as u32);
        }
        let signature = match location {
            TypeLocationOwned::SkippedFullVersion { signature, .. } => signature.clone(),
            TypeLocationOwned::InLine { .. } => type_signature(self.src, idx, self.resolve)?,
        };
        let skipped = TypeLocationOwned::SkippedFullVersion {
            crate_idx: UNib32(self.crate_idx_of_version(version)),
            type_name: name.to_string(),
            signature,
        };
        self.dst.types.push(skipped);
        Ok(self.dst.types.len() as u32 - 1)
    }

    fn trait_idx(&mut self, idx: u32) -> Result<u32> {
        let location = self
            .src
            .traits
            .get(idx as usize)
            .ok_or_else(|| anyhow!("Bad ApiBundle: no trait with index: {idx}"))?;
        let Some((version, name)) = trait_key(self.src, location) else {
            // compact version, copy as is
            self.dst.traits.push(location.clone());
            return Ok(self.dst.traits.len() as u32 - 1);
        };
        let dst = &*self.dst;
        if let Some(found) = dst
            .traits
            .iter()
            .position(|l| trait_key(dst, l) == Some((version, name)))
        {
            return Ok(found as u32);
        }
        if self.inline_all && matches!(location, ApiLevelLocationOwned::InLine { .. }) {
            let crate_idx = UNib32(self.crate_idx_of_version(version));
            let new = self.dst.traits.len();
            self.dst
                .traits
                .push(ApiLevelLocationOwned::SkippedFullVersion {
                    crate_idx,
                    trait_name: name.to_string(),
                    signature: vec![],
                });
            let mut location = location.clone();
            self.visit_api_level_location(&mut location);
            self.dst.traits[new] = location;
            return Ok(new as u32);
        }
        let signature = match location {
            ApiLevelLocationOwned::SkippedFullVersion { signature, .. } => signature.clone(),
            _ => trait_signature(self.src, idx, self.resolve)?,
        };
        let skipped = ApiLevelLocationOwned::SkippedFullVersion {
            crate_idx: UNib32(self.crate_idx_of_version(version)),
            trait_name: name.to_string(),
            signature,
        };
        self.dst.traits.push(skipped);
        Ok(self.dst.traits.len() as u32 - 1)
    }

    fn crate_idx_of_version(&mut self, version: &FullVersionOwned) -> u32 {
        if let Some(found) = self.dst.ext_crates.iter().position(|v| v == version) {
            return found as u32;
        }
        self.dst.ext_crates.push(version.clone());
        self.dst.ext_crates.len() as u32 - 1
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

impl VisitMut for Import<'_, '_, '_> {
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
        self.remap(&mut node.crate_idx, Self::crate_idx);
        visit_mut::visit_item_struct(self, node)
    }

    fn visit_item_enum(&mut self, node: &mut ItemEnumOwned) {
        self.remap(&mut node.crate_idx, Self::crate_idx);
        visit_mut::visit_item_enum(self, node)
    }
}
