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

/// The file name of the entry of a definition: `ww_error_0_1_0-type-CommonError-<signature>.ww`. Superseded
/// definitions kept next to a crate's snapshots (EVO-68) use the same names.
pub fn file_name(kind: Kind, version: &FullVersionOwned, name: &str, signature: &[u8]) -> String {
    let signature: String = signature.iter().map(|b| format!("{b:02x}")).collect();
    // `units::Volt` (API-46) as `units.Volt`, `:` is not allowed in file names everywhere
    let name = name.replace("::", ".");
    format!(
        "{}-{}-{name}-{signature}.ww",
        version.filename_friendly(),
        kind.name()
    )
}

fn file_path(
    dir: &Path,
    kind: Kind,
    version: &FullVersionOwned,
    name: &str,
    signature: &[u8],
) -> PathBuf {
    dir.join(file_name(kind, version, name, signature))
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
    decode(kind, name, signature, &bytes)
        .map(Some)
        .map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// The canonical bundle of an entry's bytes, checked to hash to `signature` and to hold the `kind` named `name` as
/// its first trait or type.
pub fn decode(kind: Kind, name: &str, signature: &[u8], bytes: &[u8]) -> Result<ApiBundleOwned> {
    if signature_of(bytes) != signature {
        return Err(anyhow!("doesn't match its signature, delete it"));
    }
    let bundle = ApiBundleOwned::from_ww_bytes_owned(bytes).map_err(|e| anyhow!("{e:?}"))?;
    let found = match kind {
        Kind::Trait => matches!(
            bundle.traits.first(),
            Some(ApiLevelLocationOwned::InLine { level, .. }) if level.trait_name == name
        ),
        Kind::Type => {
            matches!(
                bundle.types.first(),
                Some(TypeLocationOwned::InLine { ty: TypeOwned::Struct(s), .. }) if s.qualified_name() == name
            ) || matches!(
                bundle.types.first(),
                Some(TypeLocationOwned::InLine { ty: TypeOwned::Enum(e), .. }) if e.qualified_name() == name
            )
        }
    };
    if !found {
        return Err(anyhow!("doesn't hold {} {name}", kind.name()));
    }
    Ok(bundle)
}
