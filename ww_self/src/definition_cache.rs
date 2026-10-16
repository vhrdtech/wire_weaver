//! Definitions of skipped traits and types by signature, kept on the dev machine in
//! `~/.wire_weaver/definitions/`.
//!
//! A device leaves out of its introspection data the traits and types known from crate snapshots, sending only crate,
//! version, name and [signature](crate::signature). A crate that is not published yet changes under the same version,
//! so a device built from another revision of it can send a signature the host's snapshot doesn't have. Codegen
//! stores the [canonical bundle](crate::signature::canonical_type_bytes) of every definition it leaves out here, keyed
//! by its signature, so a host on the same machine finds the device's exact definition and compares it with its own
//! by the evolution rules.
//!
//! An entry is the canonical bundle's bytes as they were hashed: the signature is checked against them when loading,
//! so a reader with a newer `ww_self` still reads an entry written by an older one.

use crate::signature::signature_of;
use crate::{ApiBundleOwned, ApiLevelLocationOwned, TypeLocationOwned, TypeOwned};
use anyhow::{Result, anyhow};
use shrink_wrap::DeserializeShrinkWrapOwned;
use std::fs;
use std::path::{Path, PathBuf};
use ww_version::FullVersionOwned;

/// A trait or a type.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Trait,
    Type,
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Trait => "trait",
            Kind::Type => "type",
        }
    }
}

/// `~/.wire_weaver/definitions`, `None` without a home directory.
pub fn default_dir() -> Option<PathBuf> {
    Some(
        std::env::home_dir()?
            .join(".wire_weaver")
            .join("definitions"),
    )
}

fn file_path(
    dir: &Path,
    kind: Kind,
    version: &FullVersionOwned,
    name: &str,
    signature: &[u8],
) -> PathBuf {
    let signature: String = signature.iter().map(|b| format!("{b:02x}")).collect();
    dir.join(format!(
        "{}-{}-{name}-{signature}.ww",
        version.filename_friendly(),
        kind.name()
    ))
}

/// Store the canonical bytes of a definition, under their signature. An existing entry is left as is.
pub fn store(
    dir: &Path,
    kind: Kind,
    version: &FullVersionOwned,
    name: &str,
    canonical_bytes: &[u8],
) -> Result<()> {
    let path = file_path(dir, kind, version, name, &signature_of(canonical_bytes));
    if path.exists() {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    // written whole under a temporary name, so a reader never sees a partial entry
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, canonical_bytes)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Canonical bundle of the definition with this signature, `Ok(None)` when there is no such entry. The definition is
/// `traits[0]` or `types[0]` of it.
pub fn load(
    dir: &Path,
    kind: Kind,
    version: &FullVersionOwned,
    name: &str,
    signature: &[u8],
) -> Result<Option<ApiBundleOwned>> {
    let path = file_path(dir, kind, version, name, signature);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(anyhow!("reading {}: {e}", path.display())),
    };
    if signature_of(&bytes) != signature {
        return Err(anyhow!(
            "{} doesn't match its signature, delete it",
            path.display()
        ));
    }
    let bundle = ApiBundleOwned::from_ww_bytes_owned(&bytes)
        .map_err(|e| anyhow!("reading {}: {e:?}", path.display()))?;
    let found = match kind {
        Kind::Trait => matches!(
            bundle.traits.first(),
            Some(ApiLevelLocationOwned::InLine { level, .. }) if level.trait_name == name
        ),
        Kind::Type => {
            matches!(
                bundle.types.first(),
                Some(TypeLocationOwned::InLine { ty: TypeOwned::Struct(s), .. }) if s.ident == name
            ) || matches!(
                bundle.types.first(),
                Some(TypeLocationOwned::InLine { ty: TypeOwned::Enum(e), .. }) if e.ident == name
            )
        }
    };
    if !found {
        return Err(anyhow!(
            "{} doesn't hold {} {name}",
            path.display(),
            kind.name()
        ));
    }
    Ok(Some(bundle))
}
