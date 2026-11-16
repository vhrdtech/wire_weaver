//! Snapshot files as `ww api save` writes them, and the checks every one passes before it is used, wherever it comes
//! from (a crate's `api_snapshots/`, the cache, Cargo's sources, an archive from the network).
//!
//! A snapshot is RON for people to read, headed by the SHA-256 of the shrink_wrap bytes it stands for
//! ([ww_self::snapshot::HASH_PREFIX]). Reading one encodes the bundle again and compares the hash with the header:
//! a file edited by hand, truncated, or written by a RON that doesn't round trip is refused. Then the bundle's own
//! crate (`ext_crates[0]`) must be the one the file name says. What a file holds is matched definition by definition
//! to the signature the device sent (see [crate::Resolver]), so these checks are about rejecting damaged files early
//! with a message that names them, not the line of trust.

use crate::limits::{RON_RECURSION_LIMIT, SNAPSHOT_FILE_BYTES_MAX};
use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::path::Path;
use ww_self::ApiBundleOwned;
use ww_self::snapshot;
use ww_version::{FullVersionOwned, VersionOwned};

/// Extension of a snapshot file.
pub const EXTENSION: &str = "ron";

/// A snapshot read from its text, checked.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub bundle: ApiBundleOwned,
    /// The shrink_wrap bytes the file stands for, which the header hashes.
    pub bytes: Vec<u8>,
    /// The file's text as it was.
    pub text: String,
}

impl Snapshot {
    /// The crate and version the snapshot is of: `ext_crates[0]`.
    pub fn version(&self) -> &FullVersionOwned {
        // checked by `parse`
        &self.bundle.ext_crates[0]
    }
}

/// `ww_adc_0_3_0.ron`: the file name of the snapshot of `version`.
pub fn file_name(version: &FullVersionOwned) -> String {
    format!("{}.{EXTENSION}", version.filename_friendly())
}

/// The crate and version a snapshot file name names, `None` for any other name: a per-root snapshot
/// (`api_0_1_0.wire_v2.Thermo@w2.1.ron`, EVO-38), a stray file, a name with path pieces in it.
pub fn parse_file_name(name: &str) -> Option<FullVersionOwned> {
    let stem = name.strip_suffix(".ron")?;
    let mut parts = stem.rsplitn(4, '_');
    let patch = parts.next()?;
    let minor = parts.next()?;
    let major = parts.next()?;
    let crate_id = parts.next()?;
    if !is_plain_crate_name(crate_id) {
        return None;
    }
    let number = |s: &str| -> Option<u32> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    Some(FullVersionOwned::new(
        crate_id.to_owned(),
        VersionOwned::new(number(major)?, number(minor)?, number(patch)?),
    ))
}

/// Whether `name` is a crate name as Cargo allows them (letters, digits, `_` and `-`), so that it is one path
/// component and nothing else as a file name. Nothing from a device or the network names a file otherwise.
pub fn is_plain_crate_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Reads and checks the snapshot file at `path`, `Ok(None)` when there is no such file.
pub fn read_if_exists(path: &Path) -> Result<Option<Snapshot>> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(anyhow!("{}: {e}", path.display())),
    };
    if meta.len() > SNAPSHOT_FILE_BYTES_MAX {
        bail!(
            "{}: {} bytes, more than a snapshot can be ({SNAPSHOT_FILE_BYTES_MAX}), refused",
            path.display(),
            meta.len()
        );
    }
    let text = fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let snapshot = parse(&text).with_context(|| path.display().to_string())?;
    let named = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(parse_file_name);
    if let Some(named) = named
        && named != *snapshot.version()
    {
        bail!(
            "{}: the file is named for {} {} but holds {} {}",
            path.display(),
            named.crate_id,
            triplet(&named.version),
            snapshot.version().crate_id,
            triplet(&snapshot.version().version)
        );
    }
    Ok(Some(snapshot))
}

/// Reads and checks the snapshot file at `path`.
pub fn read(path: &Path) -> Result<Snapshot> {
    read_if_exists(path)?.ok_or_else(|| anyhow!("{}: no such file", path.display()))
}

/// Checks a snapshot's text: it parses, encodes to the bytes its header hashes, and names its own crate.
pub fn parse(text: &str) -> Result<Snapshot> {
    if text.len() as u64 > SNAPSHOT_FILE_BYTES_MAX {
        bail!(
            "{} bytes, more than a snapshot can be ({SNAPSHOT_FILE_BYTES_MAX}), refused",
            text.len()
        );
    }
    let Some(saved) = snapshot::saved_hash(text) else {
        bail!(
            "no `{}` header line: not a snapshot `ww api save` wrote",
            snapshot::HASH_PREFIX.trim_end()
        );
    };
    let bundle: ApiBundleOwned = ron::Options::default()
        .with_recursion_limit(RON_RECURSION_LIMIT)
        .from_str(text)
        .map_err(|e| anyhow!("parsing RON: {e}"))?;
    let bytes = snapshot::encode(&bundle)?;
    let hash = snapshot::sha256_hex(&bytes);
    if hash != saved {
        bail!(
            "the RON encodes to shrink_wrap bytes with sha256 {hash}, the header says {saved}: the file was \
             changed after `ww api save` wrote it, or RON does not round trip"
        );
    }
    if bundle.ext_crates.is_empty() {
        bail!("no ext_crates[0], the snapshot's own crate");
    }
    Ok(Snapshot {
        bundle,
        bytes,
        text: text.to_owned(),
    })
}

/// `0.3.0` of a version.
pub fn triplet(v: &VersionOwned) -> String {
    format!("{}.{}.{}", v.major.0, v.minor.0, v.patch.0)
}

/// The text of a snapshot file as `ww api save` writes it: two comment lines naming the crate and version, the
/// [ww_self::snapshot::HASH_PREFIX] line with the SHA-256 of the bundle's shrink_wrap bytes, then the bundle as
/// pretty RON.
pub fn render_ron(bundle: &ApiBundleOwned) -> Result<String, String> {
    let version = bundle.crate_version(0).map_err(|e| e.to_string())?;
    let header = format!(
        "// {} {} traits and types, saved by `ww api save`.\n// Do not edit, bump the crate version and save a new snapshot instead.\n",
        version.crate_id,
        triplet(&version.version)
    );
    render_with_header(bundle, header)
}

/// [render_ron] for the snapshot of one API root of a crate with several (EVO-38), naming the root and its wire
/// version in the header.
pub fn render_root_ron(bundle: &ApiBundleOwned, root: &str) -> Result<String, String> {
    let version = bundle.crate_version(0).map_err(|e| e.to_string())?;
    let wire = bundle
        .wire
        .ok_or_else(|| format!("API root {root} has no wire version"))?;
    let header = format!(
        "// {} {}: API root {root} at wire {wire} and what it reaches, saved by `ww api save`.\n// Do not edit, \
         bump the crate version and save a new snapshot instead.\n",
        version.crate_id,
        triplet(&version.version)
    );
    render_with_header(bundle, header)
}

fn render_with_header(bundle: &ApiBundleOwned, header: String) -> Result<String, String> {
    let bytes = snapshot::encode(bundle).map_err(|e| e.to_string())?;
    let ron =
        ron::ser::to_string_pretty(bundle, ron::ser::PrettyConfig::new().compact_structs(true))
            .map_err(|e| e.to_string())?;
    Ok(header + &snapshot::hash_line(&bytes) + &ron)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_round_trip() {
        let version = FullVersionOwned::new("ww_board_info".into(), VersionOwned::new(0, 12, 3));
        assert_eq!(file_name(&version), "ww_board_info_0_12_3.ron");
        assert_eq!(parse_file_name("ww_board_info_0_12_3.ron"), Some(version));
        assert_eq!(
            parse_file_name("my-api_1_0_0.ron"),
            Some(FullVersionOwned::new(
                "my-api".into(),
                VersionOwned::new(1, 0, 0)
            ))
        );
        // a per-root snapshot, a stray file, path pieces, a plus sign
        assert_eq!(parse_file_name("api_0_1_0.wire_v2.Thermo@w2.1.ron"), None);
        assert_eq!(parse_file_name("notes.txt"), None);
        assert_eq!(parse_file_name("../x_0_1_0.ron"), None);
        assert_eq!(parse_file_name("x_0_+1_0.ron"), None);
        assert_eq!(parse_file_name("_0_1_0.ron"), None);
    }

    #[test]
    fn only_a_crate_name_names_a_file() {
        assert!(is_plain_crate_name("ww_gpio"));
        assert!(is_plain_crate_name("my-api2"));
        assert!(!is_plain_crate_name(""));
        assert!(!is_plain_crate_name("../.bashrc"));
        assert!(!is_plain_crate_name("/tmp/x"));
        assert!(!is_plain_crate_name("a/b"));
        assert!(!is_plain_crate_name("a\\b"));
        assert!(!is_plain_crate_name("ünïcode"));
    }

    #[test]
    fn a_rendered_snapshot_reads_back_and_a_changed_one_is_refused() {
        let bundle = ApiBundleOwned {
            magic: ww_self::MAGIC,
            ww_self_version: ww_self::VERSION,
            root: ww_self::ApiLevelOwned {
                docs: vec![],
                crate_idx: 0.into(),
                compact_id: None,
                meta: None,
                trait_name: "dep".into(),
                items: vec![],
            },
            types: vec![],
            traits: vec![],
            ext_crates: vec![FullVersionOwned::new(
                "dep".into(),
                VersionOwned::new(0, 1, 0),
            )],
            root_omitted: None,
            wire: None,
            ext_wire: None,
        };
        let text = render_ron(&bundle).unwrap();
        let read = parse(&text).unwrap();
        assert_eq!(read.bytes, snapshot::encode(&bundle).unwrap());
        assert_eq!(read.version().crate_id, "dep");
        assert_eq!(
            snapshot::sha256_hex(&read.bytes),
            snapshot::saved_hash(&text).unwrap()
        );

        let edited = text.replace("\"dep\"", "\"dap\"");
        let e = parse(&edited).unwrap_err().to_string();
        assert!(e.contains("the header says"), "{e}");
        let e = parse("(magic: 1)").unwrap_err().to_string();
        assert!(e.contains("header line"), "{e}");
    }
}
