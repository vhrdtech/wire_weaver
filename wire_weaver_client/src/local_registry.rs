//! Local cache of API bundles at the root of a resolver's cache ([ww_snapshots::Resolver::cache_root],
//! `~/.wire_weaver/` for a host's).
//!
//! Filled by codegen (see `wire_weaver_core/src/local_registry.rs`) and by bundles downloaded from devices.
//! File names are `<crate>_<major>_<minor>_<patch>-<hash>[+docs].ron`, keep in sync with codegen: the crate is the
//! bundle's root crate. A lookup with the version a device reports opens its file by name first, without listing the
//! directory (it holds thousands of files on a busy host); the reported version is for display only and may name
//! another crate, so a miss lists the directory.

use anyhow::{Result, bail};
use ron::ser::{PrettyConfig, to_string_pretty};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use tracing::{debug, warn};
use ww_self::ApiBundleOwned;
use ww_version::{ApiHashOwned, ApiHashPairOwned, FullVersionOwned};

/// Load a bundle matching the hash pair, preferring the one with doc strings: by the file name `version` gives, then
/// from a listing of `root` ([load_by_hash]). `version` is the one the device reports, which may not be the bundle's
/// root crate. Any error is logged and treated as a cache miss.
pub(crate) fn load(
    root: &Path,
    version: &FullVersionOwned,
    hash: &ApiHashPairOwned,
) -> Option<ApiBundleOwned> {
    let by_name = is_plain_crate_name(&version.crate_id)
        .then(|| {
            load_first(hash, |hash, contains_docs| {
                let file_name = format!(
                    "{}{}",
                    version.filename_friendly(),
                    file_suffix(hash, contains_docs)
                );
                read_file(&root.join(&file_name), &file_name)
            })
        })
        .flatten();
    by_name.or_else(|| load_by_hash(root, hash))
}

/// The bundle of hash pair `hash` in the local cache at `root` (a resolver's `cache_root`), where a client keeps what
/// devices sent: looks through every file in `root`. Any error is logged and treated as a cache miss.
pub fn load_by_hash(root: &Path, hash: &ApiHashPairOwned) -> Option<ApiBundleOwned> {
    load_first(hash, |hash, contains_docs| {
        let suffix = file_suffix(hash, contains_docs);
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if file_name.ends_with(&suffix) {
                return read_file(&entry.path(), file_name);
            }
        }
        Ok(None)
    })
}

/// The first bundle `find` gives for the hash with docs, then without.
fn load_first(
    hash: &ApiHashPairOwned,
    find: impl Fn(&ApiHashOwned, bool) -> Result<Option<ApiBundleOwned>>,
) -> Option<ApiBundleOwned> {
    let candidates = [(&hash.with_docs, true), (&hash.no_docs, false)];
    for (hash, contains_docs) in candidates {
        if hash.hash.is_empty() {
            continue;
        }
        match find(hash, contains_docs) {
            Ok(Some(bundle)) => return Some(bundle),
            Ok(None) => {}
            Err(e) => warn!("Failed to read API bundle from cache: {e:?}"),
        }
    }
    None
}

/// The bundle in the cache file at `path`, `None` when there is none.
fn read_file(path: &Path, file_name: &str) -> Result<Option<ApiBundleOwned>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let api_bundle: ApiBundleOwned = ron::from_str(&contents)?;
    debug!("got API bundle from cache: {file_name}");
    Ok(Some(api_bundle))
}

/// Cache a bundle downloaded from a device, logging a failure ([store_checked]).
pub(crate) fn store(
    root: &Path,
    api_bundle: &ApiBundleOwned,
    ww_self_bytes: &[u8],
    hash: &ApiHashPairOwned,
) {
    if let Err(e) = store_checked(root, api_bundle, ww_self_bytes, hash) {
        warn!("Failed to cache API bundle: {e:?}");
    }
}

/// Cache `api_bundle`, decoded from `ww_self_bytes`, under `root`. The bytes are hashed the same way as in codegen
/// to find out whether they match `hash` with or without docs; bytes matching neither are an error. The file is
/// written whole and renamed into place, so a reader never sees half of it.
pub(crate) fn store_checked(
    root: &Path,
    api_bundle: &ApiBundleOwned,
    ww_self_bytes: &[u8],
    hash: &ApiHashPairOwned,
) -> Result<()> {
    let digest = ww_self::signature::api_hash(ww_self_bytes);
    let digest = &digest[..];
    let (hash, contains_docs) = if digest == hash.no_docs.hash.as_slice() {
        (&hash.no_docs, false)
    } else if !hash.with_docs.hash.is_empty() && digest == hash.with_docs.hash.as_slice() {
        (&hash.with_docs, true)
    } else {
        bail!(
            "API bundle hash {} matches none of the device reported hashes {hash:?}, not caching it",
            hex::encode(digest)
        );
    };

    let api_crate = api_bundle.crate_version(api_bundle.root.crate_idx.0)?;
    // the crate name comes from the device and names the file: a plain crate name, or no file
    if !is_plain_crate_name(&api_crate.crate_id) {
        warn!(
            "API crate name {:?} from the device is not a crate name, not caching its bundle",
            api_crate.crate_id
        );
        return Ok(());
    }
    let file_name = format!(
        "{}{}",
        api_crate.filename_friendly(),
        file_suffix(hash, contains_docs)
    );
    let file_path = root.join(&file_name);
    // kept when it reads back: a file of an older ww_self format is written again
    if read_back(&file_path) {
        return Ok(());
    }
    let as_ron = to_string_pretty(api_bundle, PrettyConfig::new().compact_structs(true))?;
    ww_snapshots::cache::store_file(root, &file_name, as_ron.as_bytes())?;
    debug!("cached API bundle: {file_name}");
    Ok(())
}

fn file_suffix(hash: &ApiHashOwned, contains_docs: bool) -> String {
    let docs = if contains_docs { "+docs" } else { "" };
    format!("-{}{docs}.ron", hash)
}

/// Whether the cached bundle at `path` exists and is readable in this ww_self format.
fn read_back(path: &std::path::Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .is_some_and(|s| ron::from_str::<ApiBundleOwned>(&s).is_ok())
}

/// Whether `name` is a crate name as Cargo allows them (letters, digits, `_` and `-`), so that it is one path
/// component and nothing else as a file name.
fn is_plain_crate_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lookup with the crate version opens the bundle's file by name: it never lists the cache directory, which holds
    /// thousands of files on a busy host (CLT-79). A directory that can't be listed shows it.
    #[cfg(unix)]
    #[test]
    fn load_opens_the_file_by_name() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("ww_local_registry_by_name_{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let version =
            FullVersionOwned::new("test_api".into(), ww_version::VersionOwned::new(0, 1, 0));
        let hash = ww_version::ApiHashPair {
            no_docs: ww_version::ApiHash::new(&[0xab, 0xcd]),
            with_docs: ww_version::ApiHash::new(&[]),
        }
        .make_owned();
        let bundle = crate::compat::tests::bundle("client", "0.1.0", crate::compat::tests::BLINKY);
        let file_name = format!(
            "{}{}",
            version.filename_friendly(),
            file_suffix(&hash.no_docs, false)
        );
        fs::write(root.join(file_name), ron::to_string(&bundle).unwrap()).unwrap();
        // write and search, no read: the file opens by name, the directory doesn't list
        fs::set_permissions(&root, fs::Permissions::from_mode(0o300)).unwrap();
        let found = load(&root, &version, &hash);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir_all(&root).unwrap();
        let as_ron = |b: &ApiBundleOwned| ron::to_string(b).unwrap();
        assert_eq!(found.as_ref().map(as_ron), Some(as_ron(&bundle)));
    }

    /// The version a device reports is for display and can name another crate than the bundle's root, which names
    /// the file: the lookup still finds the bundle by its hash.
    #[test]
    fn load_finds_a_file_named_after_another_crate() {
        let root = std::env::temp_dir().join(format!(
            "ww_local_registry_other_name_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let reported = FullVersionOwned::new(
            "reported_api".into(),
            ww_version::VersionOwned::new(0, 3, 0),
        );
        let root_crate =
            FullVersionOwned::new("root_api".into(), ww_version::VersionOwned::new(0, 3, 0));
        let hash = ww_version::ApiHashPair {
            no_docs: ww_version::ApiHash::new(&[0x12, 0x34]),
            with_docs: ww_version::ApiHash::new(&[]),
        }
        .make_owned();
        let bundle = crate::compat::tests::bundle("client", "0.1.0", crate::compat::tests::BLINKY);
        let file_name = format!(
            "{}{}",
            root_crate.filename_friendly(),
            file_suffix(&hash.no_docs, false)
        );
        fs::write(root.join(file_name), ron::to_string(&bundle).unwrap()).unwrap();
        let found = load(&root, &reported, &hash);
        fs::remove_dir_all(&root).unwrap();
        let as_ron = |b: &ApiBundleOwned| ron::to_string(b).unwrap();
        assert_eq!(found.as_ref().map(as_ron), Some(as_ron(&bundle)));
    }

    #[test]
    fn only_a_crate_name_names_a_cache_file() {
        assert!(is_plain_crate_name("ww_gpio"));
        assert!(is_plain_crate_name("my-api2"));
        assert!(!is_plain_crate_name(""));
        assert!(!is_plain_crate_name("../.bashrc"));
        assert!(!is_plain_crate_name("/tmp/x"));
        assert!(!is_plain_crate_name("a/b"));
        assert!(!is_plain_crate_name("a\\b"));
        assert!(!is_plain_crate_name("ünïcode"));
    }
}
