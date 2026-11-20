//! The last source of the chain: a snapshot fetched from crates.io or the crate's git repository (feature `fetch`).
//!
//! `https://static.crates.io/crates/<name>/<name>-<version>.crate` first, the exact version as published; then, for a
//! crate `sources.toml` names a repository for ([Sources]), `https://github.com/<owner>/<repo>/archive/<ref>.tar.gz`
//! (only GitHub is fetched from): old versions' snapshots never leave a crate's `api_snapshots/`, so the default
//! branch has every version's. Both are tar.gz archives read the same way, in memory, within [crate::limits]: only
//! regular entries named `**/api_snapshots/<plain>.ron` and `**/api_snapshots/superseded/<plain>.ww` under a
//! directory whose `Cargo.toml` names the crate are kept, each checked ([crate::file], the cache's superseded check).
//! Nothing from the archive forms a path on disk: what is kept goes into the cache under file names computed from
//! the checked content. TLS is rustls through `ureq`, no system library. One `info` line names each URL tried; a
//! fetch that found nothing is remembered in the cache ([cache::mark_missing]) and not tried again for
//! [FETCH_RETRY_AFTER]. `WW_OFFLINE=1`, or `offline = true` in `sources.toml`, turns the fetch off.

use crate::cache::{self, Sources};
use crate::dir::Dir;
use crate::file::{self, Snapshot};
use crate::limits::{
    ARCHIVE_BYTES_MAX, ARCHIVE_ENTRIES_MAX, ARCHIVE_FILES_KEPT_MAX, ARCHIVE_UNPACKED_BYTES_MAX,
    FETCH_CONNECT_TIMEOUT, FETCH_REDIRECTS_MAX, FETCH_RETRY_AFTER, FETCH_TIMEOUT,
    MANIFEST_BYTES_MAX, SNAPSHOT_FILE_BYTES_MAX, SUPERSEDED_FILE_BYTES_MAX,
};
use crate::locked::Locked;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::io::Read;
use std::path::Component;
use std::sync::Mutex;
use tracing::{debug, info, warn};
use ww_self::ApiBundleOwned;
use ww_self::definition_cache::{self, Kind};
use ww_self::signature::Match;
use ww_version::FullVersionOwned;

/// Fetches snapshots over the network, each crate version tried once per process.
pub struct Fetcher {
    sources: Sources,
    /// The cache's snapshots directory: what a fetch found goes there, a fetch that found nothing is marked there.
    /// `None` without a cache: found files live in this process only.
    cache: Option<Dir>,
    agent: ureq::Agent,
    /// Versions a fetch was tried for, with what the archive had of the crate when it found something.
    tried: Mutex<HashMap<FullVersionOwned, Option<Fetched>>>,
}

/// The files of one crate an archive had, checked.
#[derive(Debug, Default)]
pub(crate) struct Fetched {
    pub snapshots: Vec<Snapshot>,
    /// (file name computed from the content, bytes)
    pub superseded: Vec<(String, Vec<u8>)>,
}

/// An entry of an archive worth keeping, before the manifests are known.
enum Kept {
    Snapshot(String),
    Superseded { name: String, bytes: Vec<u8> },
}

impl Fetcher {
    /// A fetcher that stores into `cache` (the cache's snapshots directory) and goes where `sources` allows.
    pub fn new(sources: Sources, cache: Option<Dir>) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(FETCH_TIMEOUT))
            .timeout_connect(Some(FETCH_CONNECT_TIMEOUT))
            .max_redirects(FETCH_REDIRECTS_MAX)
            .http_status_as_error(true)
            .user_agent(concat!("ww_snapshots/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Fetcher {
            sources,
            cache,
            agent,
            tried: Mutex::new(HashMap::new()),
        }
    }

    /// The snapshot of `version` from the network, `None` when no source has it, the fetch is off, or a fetch of it
    /// found nothing less than [FETCH_RETRY_AFTER] ago. Tried once per process; a hit is stored in the cache.
    pub fn snapshot(&self, version: &FullVersionOwned) -> Option<Snapshot> {
        if self.sources.offline || !file::is_plain_crate_name(&version.crate_id) {
            return None;
        }
        let mut tried = self.tried.locked();
        if let Some(fetched) = tried.get(version) {
            return fetched.as_ref().and_then(|f| f.snapshot_of(version));
        }
        if let Some(cache) = &self.cache
            && cache::missing_recently(cache, version, FETCH_RETRY_AFTER)
        {
            debug!(
                "{} {}: a fetch found nothing recently, not tried again",
                version.crate_id,
                file::triplet(&version.version)
            );
            tried.insert(version.clone(), None);
            return None;
        }
        let fetched = self.fetch_version(version);
        if let Some(cache) = &self.cache {
            match &fetched {
                Some(fetched) => {
                    fetched.store(cache);
                    cache::clear_missing(cache, version);
                }
                None => {
                    if let Err(e) = cache::mark_missing(cache, version) {
                        warn!("remembering a miss: {e:#}");
                    }
                }
            }
        }
        let snapshot = fetched.as_ref().and_then(|f| f.snapshot_of(version));
        tried.insert(version.clone(), fetched);
        snapshot
    }

    /// A superseded definition (EVO-68) of `version` that `reference` names, among the files a fetch of this process
    /// brought: what the cache answers from too, when there is one.
    pub(crate) fn superseded(
        &self,
        kind: Kind,
        version: &FullVersionOwned,
        name: &str,
        reference: &[u8],
    ) -> Option<ApiBundleOwned> {
        let tried = self.tried.locked();
        let fetched = tried.get(version)?.as_ref()?;
        superseded_among(
            fetched
                .superseded
                .iter()
                .map(|(n, b)| (n.as_str(), b.as_slice())),
            kind,
            version,
            name,
            reference,
        )
    }

    /// Every URL tried for `version`, then the first archive that has the crate's snapshot of it.
    fn fetch_version(&self, version: &FullVersionOwned) -> Option<Fetched> {
        for url in self.urls(version) {
            info!("fetching {url}");
            let archive = match self.download(&url) {
                Ok(Some(archive)) => archive,
                Ok(None) => {
                    debug!("{url}: not found");
                    continue;
                }
                Err(e) => {
                    warn!("{url}: {e:#}");
                    continue;
                }
            };
            match read_archive(&archive, &version.crate_id) {
                Ok(fetched) if fetched.snapshot_of(version).is_some() => return Some(fetched),
                Ok(fetched) => debug!(
                    "{url}: {} snapshots of {}, none of {}",
                    fetched.snapshots.len(),
                    version.crate_id,
                    file::triplet(&version.version)
                ),
                Err(e) => warn!("{url}: {e:#}"),
            }
        }
        None
    }

    /// crates.io, then the repository `sources.toml` names for the crate.
    fn urls(&self, version: &FullVersionOwned) -> Vec<String> {
        let name = &version.crate_id;
        let triplet = file::triplet(&version.version);
        let mut urls = vec![format!(
            "https://static.crates.io/crates/{name}/{name}-{triplet}.crate"
        )];
        if let Some(repository) = self.sources.repositories.get(name) {
            match github_archive_url(&repository.url, repository.git_ref.as_deref()) {
                Ok(url) => urls.push(url),
                Err(e) => warn!("sources.toml, repository of {name}: {e:#}"),
            }
        }
        urls
    }

    /// The body at `url`, at most [ARCHIVE_BYTES_MAX] bytes; `Ok(None)` for a 404.
    fn download(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let mut response = match self.agent.get(url).call() {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(404)) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let body = response
            .body_mut()
            .with_config()
            .limit(ARCHIVE_BYTES_MAX)
            .read_to_vec()
            .with_context(|| format!("reading the body (at most {ARCHIVE_BYTES_MAX} bytes)"))?;
        Ok(Some(body))
    }
}

impl Fetched {
    fn snapshot_of(&self, version: &FullVersionOwned) -> Option<Snapshot> {
        self.snapshots
            .iter()
            .find(|s| s.version() == version)
            .cloned()
    }

    /// Stores every file into the cache; a failure is named in the log, the files stay usable in this process.
    fn store(&self, cache: &Dir) {
        for snapshot in &self.snapshots {
            if let Err(e) = cache::store_snapshot(cache, snapshot) {
                warn!("storing a fetched snapshot: {e:#}");
            }
        }
        let superseded_dir = cache.superseded_dir();
        for (name, bytes) in &self.superseded {
            if let Err(e) = cache::store_file(&superseded_dir, name, bytes) {
                warn!("storing a fetched superseded definition: {e:#}");
            }
        }
    }
}

/// `https://github.com/<owner>/<repo>/archive/<ref>.tar.gz` for a repository URL `sources.toml` gives; `HEAD`
/// without a ref. Owner, repository and ref are checked to be plain names before they go into a URL.
fn github_archive_url(url: &str, git_ref: Option<&str>) -> Result<String> {
    let path = url
        .strip_prefix("https://github.com/")
        .context("only https://github.com/<owner>/<repo> is fetched from")?;
    let path = path.strip_suffix('/').unwrap_or(path);
    let path = path.strip_suffix(".git").unwrap_or(path);
    let plain = |s: &str| {
        !s.is_empty()
            && s.len() <= 256
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
            && s != "."
            && s != ".."
    };
    let Some((owner, repo)) = path.split_once('/') else {
        bail!("`{url}`: not https://github.com/<owner>/<repo>");
    };
    if !plain(owner) || !plain(repo) {
        bail!("`{url}`: owner and repository must be plain names");
    }
    let git_ref = git_ref.unwrap_or("HEAD");
    if !git_ref.split('/').all(plain) {
        bail!("ref `{git_ref}`: not a plain branch, tag or commit name");
    }
    Ok(format!(
        "https://github.com/{owner}/{repo}/archive/{git_ref}.tar.gz"
    ))
}

/// Everything of the crate `crate_name` in a tar.gz archive: its whole-crate snapshots and superseded definitions,
/// each checked, from the `api_snapshots/` of every directory whose `Cargo.toml` names the crate. Read in memory,
/// within [crate::limits]; an archive past them is an error, an entry past its file's limit is skipped and logged.
pub(crate) fn read_archive(gz: &[u8], crate_name: &str) -> Result<Fetched> {
    let decoder = flate2::read::GzDecoder::new(gz).take(ARCHIVE_UNPACKED_BYTES_MAX);
    let mut archive = tar::Archive::new(decoder);
    let mut manifests: HashMap<String, String> = HashMap::new();
    let mut kept: Vec<(String, Kept)> = vec![];
    for (n, entry) in archive.entries()?.enumerate() {
        if n >= ARCHIVE_ENTRIES_MAX {
            bail!("more than {ARCHIVE_ENTRIES_MAX} entries, refused");
        }
        let mut entry = entry.context("reading an entry")?;
        if entry.header().entry_type() != tar::EntryType::Regular {
            continue;
        }
        let path = entry.path().context("an entry's path")?.into_owned();
        let mut parts = vec![];
        for component in path.components() {
            if let Component::Normal(s) = component
                && let Some(s) = s.to_str()
                && !s.contains('\\')
            {
                parts.push(s.to_owned());
            } else {
                debug!("{}: not a plain relative path, skipped", path.display());
                parts.clear();
                break;
            }
        }
        let [dir @ .., last] = parts.as_slice() else {
            continue;
        };
        let (dir, limit) = match dir {
            _ if last == "Cargo.toml" => (dir, MANIFEST_BYTES_MAX),
            [dir @ .., snapshots]
                if snapshots == crate::dir::SNAPSHOTS_DIR
                    && file::parse_file_name(last).is_some_and(|v| v.crate_id == crate_name) =>
            {
                (dir, SNAPSHOT_FILE_BYTES_MAX)
            }
            [dir @ .., snapshots, superseded]
                if snapshots == crate::dir::SNAPSHOTS_DIR
                    && superseded == crate::dir::SUPERSEDED_DIR
                    && last.ends_with(".ww") =>
            {
                (dir, SUPERSEDED_FILE_BYTES_MAX)
            }
            _ => continue,
        };
        if entry.size() > limit {
            warn!(
                "{}: {} bytes, more than {limit}, skipped",
                path.display(),
                entry.size()
            );
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(limit)
            .read_to_end(&mut bytes)
            .with_context(|| path.display().to_string())?;
        let dir = dir.join("/");
        if last == "Cargo.toml" {
            if let Some(name) = package_name(&bytes) {
                manifests.insert(dir, name);
            }
            continue;
        }
        if kept.len() >= ARCHIVE_FILES_KEPT_MAX {
            bail!("more than {ARCHIVE_FILES_KEPT_MAX} snapshot files, refused");
        }
        let item = if last.ends_with(".ww") {
            Kept::Superseded {
                name: last.clone(),
                bytes,
            }
        } else {
            Kept::Snapshot(String::from_utf8(bytes).with_context(|| path.display().to_string())?)
        };
        kept.push((dir, item));
    }
    let mut fetched = Fetched::default();
    for (dir, item) in kept {
        if manifests.get(&dir).map(String::as_str) != Some(crate_name) {
            debug!("{dir}/api_snapshots: not the crate {crate_name}, skipped");
            continue;
        }
        match item {
            Kept::Snapshot(text) => match file::parse(&text) {
                Ok(snapshot) if snapshot.version().crate_id == crate_name => {
                    fetched.snapshots.push(snapshot);
                }
                Ok(snapshot) => warn!(
                    "{dir}: a snapshot of {} where {crate_name}'s were expected, skipped",
                    snapshot.version().crate_id
                ),
                Err(e) => warn!("{dir}: a snapshot file refused: {e:#}"),
            },
            Kept::Superseded { name, bytes } => match cache::check_superseded(&name, &bytes) {
                Ok(name) => fetched.superseded.push((name, bytes)),
                Err(e) => warn!("{dir}: a superseded definition refused: {e:#}"),
            },
        }
    }
    Ok(fetched)
}

/// `[package] name` of a manifest, `None` for anything else (a workspace root, a damaged file).
fn package_name(manifest: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(manifest).ok()?;
    let table: toml::Table = toml::from_str(text).ok()?;
    let name = table.get("package")?.get("name")?.as_str()?;
    file::is_plain_crate_name(name).then(|| name.to_owned())
}

/// The superseded definition (EVO-68) of `version` that `reference` names among `entries` (file name, bytes), the
/// way an embedded set and a directory answer: one with the same docs is preferred to one with the same shape only.
pub(crate) fn superseded_among<'a>(
    entries: impl Iterator<Item = (&'a str, &'a [u8])>,
    kind: Kind,
    version: &FullVersionOwned,
    name: &str,
    reference: &[u8],
) -> Option<ApiBundleOwned> {
    let prefix = definition_cache::file_name(kind, version, name, &[]);
    let prefix = prefix.strip_suffix(".ww").unwrap_or(&prefix);
    let mut same_shape = None;
    for (file_name, bytes) in entries {
        if !file_name
            .strip_prefix(prefix)
            .is_some_and(|rest| !rest.contains('-'))
        {
            continue;
        }
        let (bundle, signatures) = match definition_cache::decode_with_signatures(kind, name, bytes)
        {
            Ok(decoded) => decoded,
            Err(e) => {
                warn!("superseded definition {file_name}: {e:#}");
                continue;
            }
        };
        match signatures.matches(reference) {
            Match::Exact => return Some(bundle),
            Match::No => {}
            Match::DocsDiffer | Match::Shape => same_shape = same_shape.or(Some(bundle)),
        }
    }
    same_shape
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use ww_version::VersionOwned;

    fn bundle_of(crate_id: &str, version: VersionOwned) -> ApiBundleOwned {
        ApiBundleOwned {
            magic: ww_self::MAGIC,
            ww_self_version: ww_self::VERSION,
            root: ww_self::ApiLevelOwned {
                docs: vec![],
                crate_idx: 0.into(),
                compact_id: None,
                meta: None,
                trait_name: "Api".to_owned(),
                items: vec![],
            },
            types: vec![],
            traits: vec![],
            ext_crates: vec![FullVersionOwned::new(crate_id.to_owned(), version)],
            root_omitted: None,
            wire: None,
            ext_wire: None,
        }
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (path, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, path, *bytes).unwrap();
        }
        let tar = tar.into_inner().unwrap();
        let mut gz = GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn only_the_crates_checked_snapshots_are_kept() {
        let v1 = bundle_of("dep", VersionOwned::new(0, 1, 0));
        let v2 = bundle_of("dep", VersionOwned::new(0, 2, 0));
        let other = bundle_of("other", VersionOwned::new(0, 1, 0));
        let ron = |b| file::render_ron(b).unwrap();
        let (r1, r2, r_other) = (ron(&v1), ron(&v2), ron(&other));
        let damaged = r1.replace("trait_name: \"Api\"", "trait_name: \"Nope\"");
        let gz = archive(&[
            (
                "dep-0.2.0/Cargo.toml",
                b"[package]\nname = \"dep\"\nversion = \"0.2.0\"\n",
            ),
            ("dep-0.2.0/api_snapshots/dep_0_1_0.ron", r1.as_bytes()),
            ("dep-0.2.0/api_snapshots/dep_0_2_0.ron", r2.as_bytes()),
            // a snapshot of another crate under this crate's directory
            (
                "dep-0.2.0/api_snapshots/other_0_1_0.ron",
                r_other.as_bytes(),
            ),
            // the right name, bytes that don't hash
            ("dep-0.2.0/api_snapshots/dep_0_3_0.ron", damaged.as_bytes()),
            // the same crate name, another package: not kept
            ("fork/Cargo.toml", b"[package]\nname = \"fork\"\n"),
            ("fork/api_snapshots/dep_0_4_0.ron", r1.as_bytes()),
            // no manifest above it
            ("loose/api_snapshots/dep_0_5_0.ron", r1.as_bytes()),
            ("dep-0.2.0/src/lib.rs", b"// not a snapshot"),
        ]);
        let fetched = read_archive(&gz, "dep").unwrap();
        let versions: Vec<_> = fetched
            .snapshots
            .iter()
            .map(|s| file::triplet(&s.version().version))
            .collect();
        assert_eq!(versions, ["0.1.0", "0.2.0"]);
        assert!(fetched.superseded.is_empty());
        assert!(fetched.snapshot_of(&v2.ext_crates[0]).is_some());
        assert!(fetched.snapshot_of(&other.ext_crates[0]).is_none());
    }

    #[test]
    fn paths_out_of_the_tree_and_huge_entries_are_skipped() {
        let v1 = bundle_of("dep", VersionOwned::new(0, 1, 0));
        let r1 = file::render_ron(&v1).unwrap();
        let mut tar = tar::Builder::new(Vec::new());
        // a manifest, then an entry whose path climbs out
        let manifest = b"[package]\nname = \"dep\"\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, "dep/Cargo.toml", &manifest[..])
            .unwrap();
        // `set_path` refuses `..`, as the builder should: the bytes go in directly, as an attacker writes them
        let mut header = tar::Header::new_gnu();
        header.set_size(r1.len() as u64);
        let name = b"dep/api_snapshots/../../dep_0_1_0.ron";
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_cksum();
        tar.append(&header, r1.as_bytes()).unwrap();
        let mut gz = GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar.into_inner().unwrap()).unwrap();
        let fetched = read_archive(&gz.finish().unwrap(), "dep").unwrap();
        assert!(fetched.snapshots.is_empty());
    }

    #[test]
    fn github_urls_are_plain() {
        assert_eq!(
            github_archive_url("https://github.com/o/r", None).unwrap(),
            "https://github.com/o/r/archive/HEAD.tar.gz"
        );
        assert_eq!(
            github_archive_url("https://github.com/o/r.git/", Some("v0.1.0")).unwrap(),
            "https://github.com/o/r/archive/v0.1.0.tar.gz"
        );
        assert!(github_archive_url("https://gitlab.com/o/r", None).is_err());
        assert!(github_archive_url("https://github.com/o/r/../x", None).is_err());
        assert!(github_archive_url("https://github.com/o/r", Some("a?b")).is_err());
        assert!(github_archive_url("https://github.com/o", None).is_err());
    }

    #[test]
    fn an_offline_fetcher_never_goes_out() {
        let fetcher = Fetcher::new(
            Sources {
                offline: true,
                ..Default::default()
            },
            None,
        );
        let version = FullVersionOwned::new("dep".into(), VersionOwned::new(0, 1, 0));
        assert!(fetcher.snapshot(&version).is_none());
        assert!(fetcher.tried.locked().is_empty());
    }
}
