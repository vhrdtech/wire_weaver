//! Definitions an in-place refresh of a snapshot replaces (EVO-68).
//!
//! A crate version not published yet can change, and `ww api save` then refreshes its snapshot in place. Other
//! crates' snapshots, frozen API lines and devices built before still name the crate's traits and types by their old
//! signatures, so each own definition whose signature the refresh changes is kept: its canonical bundle (the
//! definition first, everything it refers to in-line, the bytes its signature hashes), as the definition cache stores
//! one ([ww_self::definition_cache]), in `api_snapshots/superseded/`. `just save-snapshots` copies them into
//! `wire_weaver_snapshots`, which finds them by signature
//! ([wire_weaver_snapshots::superseded]), and the checker compares the two definitions by the evolution rules.

use std::path::{Path, PathBuf};

use ww_self::definition_cache::{self, Kind};
use ww_self::signature::{self, no_resolve};
use ww_self::{ApiBundleOwned, ApiLevelLocationOwned, TypeLocationOwned, TypeOwned};

use crate::compat;

/// The directory in a crate's `api_snapshots/` that keeps superseded definitions.
pub const SUPERSEDED_DIR: &str = "superseded";

/// A definition of the crate's own that an in-place refresh of its snapshot replaces, kept by its old signature.
#[derive(Clone, Debug)]
pub struct Superseded {
    /// Where it is kept: `api_snapshots/superseded/<crate>_<version>-<kind>-<name>-<signature>.ww`.
    pub path: PathBuf,
    pub kind: Kind,
    /// The trait's name, or the type's qualified name (`units::Volt`).
    pub name: String,
    /// The old signature with docs, which the bytes hash to (EVO-69: both signatures are computed from the bytes when
    /// it is looked up).
    pub signature: Vec<u8>,
    /// The canonical bundle's bytes.
    pub bytes: Vec<u8>,
}

/// The own definitions of `old`, the snapshot about to be refreshed, whose signature differs from the one in
/// `definitions`, the source's bundle with every definition in-line (`wire_weaver_core::SourceNotes::definitions`),
/// or which the source no longer has, to be kept in `dir`. Also what couldn't be kept and why: a definition of
/// another crate the old one refers to that is known nowhere any more.
pub(crate) fn superseded(
    dir: &Path,
    old: &ApiBundleOwned,
    definitions: &ApiBundleOwned,
) -> (Vec<Superseded>, Vec<String>) {
    let Ok(own) = old.crate_version(0) else {
        return (vec![], vec![]);
    };
    // the other crates' definitions it refers to, by the signatures it recorded: from the embedded snapshots, or
    // kept by their own refresh
    let mut inlined = old.clone();
    compat::inline_known(&mut inlined);
    let mut kept = vec![];
    let mut warnings = vec![];
    for (kind, idx, name) in own_definitions(&inlined, own) {
        let Some(new_signature) = signature_in(definitions, own, kind, &name) else {
            continue;
        };
        let canonical = match kind {
            Kind::Trait => signature::canonical_trait_bytes(&inlined, idx, &no_resolve),
            Kind::Type => signature::canonical_type_bytes(&inlined, idx, &no_resolve),
        };
        let bytes = match canonical {
            Ok(bytes) => bytes,
            Err(e) => {
                warnings.push(format!(
                    "{} {}::{name}: the definition the refresh replaces can't be kept, {e:#}: a snapshot or device \
                     naming it by its signature is not checked against the new one",
                    kind.name(),
                    own.crate_id
                ));
                continue;
            }
        };
        let old_signature = match signature::signatures_of(&bytes) {
            Ok(signatures) => signatures.with_docs,
            Err(e) => {
                warnings.push(format!("{} {}::{name}: {e:#}", kind.name(), own.crate_id));
                continue;
            }
        };
        // kept on a change of the docs too: a reference saved before EVO-69 names a definition by its signature with
        // docs, and only the definition itself tells its shape
        if new_signature.as_deref() == Some(old_signature.as_slice()) {
            continue;
        }
        let path = dir.join(definition_cache::file_name(
            kind,
            own,
            &name,
            &old_signature,
        ));
        kept.push(Superseded {
            path,
            kind,
            name,
            signature: old_signature,
            bytes,
        });
    }
    (kept, warnings)
}

/// Keeps the definitions of `old`, an earlier snapshot of the crate version `current` is a snapshot of (one a refresh
/// in place replaced before EVO-68 kept anything, e.g. taken from git history), that `current` no longer has: as a
/// refresh in place does, in `dir` (`api_snapshots/superseded/`). Writes the ones not kept yet and returns them, with
/// what couldn't be kept and why. `ww api keep-superseded` calls it.
pub fn keep_superseded(
    dir: &Path,
    old: &ApiBundleOwned,
    current: &ApiBundleOwned,
) -> Result<(Vec<Superseded>, Vec<String>), String> {
    let (Ok(old_version), Ok(current_version)) = (old.crate_version(0), current.crate_version(0))
    else {
        return Err("not a crate snapshot, ext_crates[0] must be the crate itself".into());
    };
    if old_version != current_version {
        return Err(format!(
            "{} {} and {} {} are snapshots of different crate versions",
            old_version.crate_id,
            crate::snapshot_files::triplet(&old_version.version),
            current_version.crate_id,
            crate::snapshot_files::triplet(&current_version.version),
        ));
    }
    let mut definitions = current.clone();
    compat::inline_known(&mut definitions);
    let (kept, warnings) = superseded(dir, old, &definitions);
    let mut written = vec![];
    for kept in kept {
        if kept.path.exists() {
            continue;
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        std::fs::write(&kept.path, &kept.bytes)
            .map_err(|e| format!("writing {}: {e}", kept.path.display()))?;
        written.push(kept);
    }
    Ok((written, warnings))
}

/// The traits and types in-line in `bundle` that are of the crate `own`: kind, index and name.
fn own_definitions(
    bundle: &ApiBundleOwned,
    own: &ww_version::FullVersionOwned,
) -> Vec<(Kind, u32, String)> {
    let is_own = |crate_idx: u32| bundle.crate_version(crate_idx).is_ok_and(|v| v == own);
    let traits = bundle
        .traits
        .iter()
        .enumerate()
        .filter_map(|(idx, location)| match location {
            ApiLevelLocationOwned::InLine { level, crate_idx } if is_own(crate_idx.0) => {
                Some((Kind::Trait, idx as u32, level.trait_name.clone()))
            }
            _ => None,
        });
    let types = bundle
        .types
        .iter()
        .enumerate()
        .filter_map(|(idx, location)| match location {
            TypeLocationOwned::InLine {
                ty: TypeOwned::Struct(s),
                ..
            } if is_own(s.crate_idx.0) => Some((Kind::Type, idx as u32, s.qualified_name())),
            TypeLocationOwned::InLine {
                ty: TypeOwned::Enum(e),
                ..
            } if is_own(e.crate_idx.0) => Some((Kind::Type, idx as u32, e.qualified_name())),
            _ => None,
        });
    traits.chain(types).collect()
}

/// The signature with docs of the crate `own`'s `kind` named `name` in `bundle`, which has every definition in-line: `Some(None)`
/// when it has none, `None` when it can't be calculated (nothing is kept then).
fn signature_in(
    bundle: &ApiBundleOwned,
    own: &ww_version::FullVersionOwned,
    kind: Kind,
    name: &str,
) -> Option<Option<Vec<u8>>> {
    let Some((_, idx, _)) = own_definitions(bundle, own)
        .into_iter()
        .find(|(k, _, n)| *k == kind && n == name)
    else {
        return Some(None);
    };
    let found = match kind {
        Kind::Trait => signature::trait_signatures(bundle, idx, &no_resolve),
        Kind::Type => signature::type_signatures(bundle, idx, &no_resolve),
    };
    found.ok().map(|s| Some(s.with_docs))
}
