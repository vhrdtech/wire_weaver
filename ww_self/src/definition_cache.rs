//! Definitions of skipped traits and types by signature, kept on the dev machine in
//! `~/.wire_weaver/definitions/`.
//!
//! A device leaves out of its introspection data the traits and types known from crate snapshots, sending only crate,
//! version, name and [signatures](crate::signature). A crate that is not published yet changes under the same version,
//! so a device built from another revision of it can send a signature the host's snapshot doesn't have. Codegen
//! stores the [canonical bundle](crate::signature::canonical_type_bytes) of every definition it leaves out here, keyed
//! by its shape signature, so a host on the same machine finds the device's definition and compares it with its own by
//! the evolution rules.
//!
//! An entry is the canonical bundle's bytes with docs, named by the definition's shape signature (an entry written
//! before EVO-69 by its signature with docs). Both signatures are computed from the bytes when loading and the
//! reference is checked against them ([Signatures::matches]), so a reader with a newer `ww_self` still reads an entry
//! written by an older one.

use crate::signature::{self, Match, Signatures};
use crate::{ApiBundleOwned, ApiLevelLocationOwned, TypeLocationOwned, TypeOwned};
use anyhow::{Result, anyhow};
use shrink_wrap::DeserializeShrinkWrapOwned;
use std::fs;
use std::path::{Path, PathBuf};
use ww_version::FullVersionOwned;

/// A trait or a type.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
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

/// The most an entry can be: a canonical bundle of one definition and what it refers to, a few KB today. A larger
/// file in the cache is refused unread.
pub const ENTRY_BYTES_MAX: u64 = 4 << 20;

/// Whether `name` is a definition's name as the API model has it (`Volt`, `units::Volt`), so that it forms one file
/// name and nothing else. A name from a device or a file is checked before it reaches a path.
pub fn is_plain_definition_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
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

/// Store the canonical bytes of a definition, under its shape signature. An entry of the same shape with other docs
/// is replaced: the newest docs win.
pub fn store(
    dir: &Path,
    kind: Kind,
    version: &FullVersionOwned,
    name: &str,
    canonical_bytes: &[u8],
) -> Result<()> {
    if !is_plain_definition_name(name) {
        return Err(anyhow!("`{name}` is not a definition name, refused"));
    }
    let signatures = signature::signatures_of(canonical_bytes)?;
    let path = file_path(dir, kind, version, name, &signatures.shape);
    if fs::read(&path).is_ok_and(|bytes| bytes == canonical_bytes) {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    // written whole under a temporary name, so a reader never sees a partial entry
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, canonical_bytes)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Canonical bundle of the definition `reference` names (a `Skipped*` location's signature bytes), `Ok(None)` when
/// there is no such entry. The definition is `traits[0]` or `types[0]` of it.
pub fn load(
    dir: &Path,
    kind: Kind,
    version: &FullVersionOwned,
    name: &str,
    signature: &[u8],
) -> Result<Option<ApiBundleOwned>> {
    if !is_plain_definition_name(name) {
        return Err(anyhow!("`{name}` is not a definition name, refused"));
    }
    // named by the shape signature, or (written before EVO-69) by the signature with docs
    let path = file_path(dir, kind, version, name, signature::shape_part(signature));
    match read_entry(&path) {
        Ok(Some(bytes)) => {
            return decode(kind, name, signature, &bytes)
                .map(Some)
                .map_err(|e| anyhow!("{}: {e}", path.display()));
        }
        Ok(None) => {}
        Err(e) => return Err(e),
    }
    // a reference from before EVO-69 is a signature with docs, while the entry is named by its shape: the entries of
    // this definition are few, each is checked
    let prefix = file_name(kind, version, name, &[]);
    let prefix = prefix.strip_suffix(".ww").unwrap_or(&prefix);
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(None);
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(rest) = file_name.to_str().and_then(|n| n.strip_prefix(prefix)) else {
            continue;
        };
        if rest.contains('-') || !rest.ends_with(".ww") {
            continue;
        }
        let Ok(Some(bytes)) = read_entry(&entry.path()) else {
            continue;
        };
        if let Ok((bundle, signatures)) = decode_with_signatures(kind, name, &bytes)
            && signatures.matches(signature) != Match::No
        {
            return Ok(Some(bundle));
        }
    }
    Ok(None)
}

/// The bytes of the entry at `path`, `Ok(None)` when there is none; an entry over [ENTRY_BYTES_MAX] is refused.
fn read_entry(path: &Path) -> Result<Option<Vec<u8>>> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(anyhow!("reading {}: {e}", path.display())),
    };
    if meta.len() > ENTRY_BYTES_MAX {
        return Err(anyhow!(
            "{}: {} bytes, more than a definition can be ({ENTRY_BYTES_MAX}), refused",
            path.display(),
            meta.len()
        ));
    }
    fs::read(path)
        .map(Some)
        .map_err(|e| anyhow!("reading {}: {e}", path.display()))
}

/// The canonical bundle of an entry's bytes, checked to be of the definition `reference` names (docs aside) and to
/// hold the `kind` named `name` as its first trait or type.
pub fn decode(kind: Kind, name: &str, reference: &[u8], bytes: &[u8]) -> Result<ApiBundleOwned> {
    let (bundle, signatures) = decode_with_signatures(kind, name, bytes)?;
    if signatures.matches(reference) == Match::No {
        return Err(anyhow!("doesn't match its signature, delete it"));
    }
    Ok(bundle)
}

/// The canonical bundle of an entry's bytes and the [Signatures] computed from them, checked to hold the `kind`
/// named `name` as its first trait or type.
pub fn decode_with_signatures(
    kind: Kind,
    name: &str,
    bytes: &[u8],
) -> Result<(ApiBundleOwned, Signatures)> {
    let signatures = signature::signatures_of(bytes)?;
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
    Ok((bundle, signatures))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_is_not_a_definition_never_reaches_a_path() {
        assert!(is_plain_definition_name("Volt"));
        assert!(is_plain_definition_name("units::Volt"));
        assert!(!is_plain_definition_name(""));
        assert!(!is_plain_definition_name("../x"));
        assert!(!is_plain_definition_name("a/b"));
        assert!(!is_plain_definition_name("a b"));
        let dir = std::env::temp_dir().join(format!("ww_definition_names_{}", std::process::id()));
        let version = FullVersionOwned::new("dep".into(), ww_version::VersionOwned::new(0, 1, 0));
        let e = load(&dir, Kind::Type, &version, "../x", &[0; 8]).unwrap_err();
        assert!(e.to_string().contains("not a definition name"), "{e}");
        let e = store(&dir, Kind::Type, &version, "../x", &[]).unwrap_err();
        assert!(e.to_string().contains("not a definition name"), "{e}");
    }

    #[test]
    fn an_entry_over_the_bound_is_refused_unread() {
        let dir = std::env::temp_dir().join(format!("ww_definition_bound_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let version = FullVersionOwned::new("dep".into(), ww_version::VersionOwned::new(0, 1, 0));
        let reference = [7u8; 8];
        let path = file_path(&dir, Kind::Type, &version, "Big", &reference);
        let file = fs::File::create(&path).unwrap();
        file.set_len(ENTRY_BYTES_MAX + 1).unwrap();
        let e = load(&dir, Kind::Type, &version, "Big", &reference).unwrap_err();
        fs::remove_dir_all(&dir).unwrap();
        assert!(
            e.to_string().contains("more than a definition can be"),
            "{e}"
        );
    }
}
