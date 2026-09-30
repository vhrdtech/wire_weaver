//! Putting definitions of skipped traits and types back into a bundle, e.g. from crate snapshots, so that it looks as
//! if nothing was skipped.
//!
//! Each definition is only put back if its [signature](crate::signature) matches the one stored in the skipped
//! location. Indices differ from a bundle that had everything in-line from the start: definitions put back replace
//! the skipped locations in place, and the ones they refer to are appended.

use crate::signature::{Resolve, trait_signature, type_signature};
use crate::visit_mut::{self, VisitMut};
use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    ItemEnumOwned, ItemStructOwned, Multiplicity, TypeLocationOwned, TypeOwned,
};
use anyhow::{Result, anyhow};
use core::fmt::{Display, Formatter};
use shrink_wrap::UNib32;
use std::collections::HashSet;
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

/// Replace skipped traits and types with their definitions found with `resolve`, together with everything they refer
/// to. Returns the ones that are left skipped: not found, or found with a different signature.
pub fn inline_skipped<'a>(
    bundle: &mut ApiBundleOwned,
    resolve: Resolve<'a, '_>,
) -> Vec<NotInlined> {
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
            match inline_type(bundle, idx, resolve) {
                Ok(()) => progress = true,
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
            match inline_trait(bundle, idx, resolve) {
                Ok(()) => progress = true,
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
            return not_inlined;
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
) -> Result<()> {
    let TypeLocationOwned::SkippedFullVersion {
        crate_idx,
        type_name,
        signature,
    } = &bundle.types[idx]
    else {
        return Ok(());
    };
    let version = bundle.crate_version(crate_idx.0)?.clone();
    let src = resolve(&version).ok_or_else(|| anyhow!("definition not found"))?;
    let src_idx =
        find_type(src, &version, type_name).ok_or_else(|| anyhow!("definition not found"))?;
    check_signature(signature, &type_signature(src, src_idx, resolve)?)?;
    let mut location = src.types[src_idx as usize].clone();
    let mut import = Import::new(bundle, src, resolve);
    import.visit_type_location(&mut location);
    import.finish()?;
    bundle.types[idx] = location;
    Ok(())
}

fn inline_trait<'a>(
    bundle: &mut ApiBundleOwned,
    idx: usize,
    resolve: Resolve<'a, '_>,
) -> Result<()> {
    let ApiLevelLocationOwned::SkippedFullVersion {
        crate_idx,
        trait_name,
        signature,
    } = &bundle.traits[idx]
    else {
        return Ok(());
    };
    let version = bundle.crate_version(crate_idx.0)?.clone();
    let src = resolve(&version).ok_or_else(|| anyhow!("definition not found"))?;
    let src_idx =
        find_trait(src, &version, trait_name).ok_or_else(|| anyhow!("definition not found"))?;
    check_signature(signature, &trait_signature(src, src_idx, resolve)?)?;
    let mut location = src.traits[src_idx as usize].clone();
    let mut import = Import::new(bundle, src, resolve);
    import.visit_api_level_location(&mut location);
    import.finish()?;
    bundle.traits[idx] = location;
    Ok(())
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

/// Crate version and name of a user-defined type (in-line struct or enum, or skipped).
fn type_key<'b>(
    bundle: &'b ApiBundleOwned,
    location: &'b TypeLocationOwned,
) -> Option<(&'b FullVersionOwned, &'b str)> {
    let (crate_idx, name) = match location {
        TypeLocationOwned::InLine {
            ty:
                TypeOwned::Struct(ItemStructOwned { ident, .. })
                | TypeOwned::Enum(ItemEnumOwned { ident, .. }),
            crate_idx,
        } => (crate_idx, ident),
        TypeLocationOwned::SkippedFullVersion {
            crate_idx,
            type_name,
            ..
        } => (crate_idx, type_name),
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
                && type_key(bundle, location) == Some((version, name))
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
    error: Option<anyhow::Error>,
}

impl<'d, 'a, 'r> Import<'d, 'a, 'r> {
    fn new(dst: &'d mut ApiBundleOwned, src: &'a ApiBundleOwned, resolve: Resolve<'a, 'r>) -> Self {
        Import {
            dst,
            src,
            resolve,
            error: None,
        }
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
            // not user-defined (e.g. an array index type), copy as is
            let mut location = location.clone();
            self.visit_type_location(&mut location);
            self.dst.types.push(location);
            return Ok(self.dst.types.len() as u32 - 1);
        };
        let dst = &*self.dst;
        if let Some(found) = dst
            .types
            .iter()
            .position(|l| type_key(dst, l) == Some((version, name)))
        {
            return Ok(found as u32);
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
