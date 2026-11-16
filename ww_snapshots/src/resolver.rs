//! The chain that finds a snapshot by crate name and version, and a definition by signature when the snapshot found
//! has it under another one; see the [crate docs](crate) for the order and the trust model.

use crate::cache;
use crate::cargo::{self, CargoSources};
use crate::dir::Dir;
use crate::file;
use crate::set::Set;
use anyhow::Result;
use elsa::sync::FrozenMap;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, RwLock};
use tracing::{debug, warn};
use ww_self::ApiBundleOwned;
use ww_self::definition_cache::{self, Kind};
use ww_self::inline::{self, Inlined};
use ww_self::signature;
use ww_version::FullVersionOwned;

/// Where a snapshot came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// An embedded set, by the position of its source in the resolver.
    Set(usize),
    /// A directory the caller named: a walked crate's `api_snapshots/`.
    Dir(PathBuf),
    /// The cache, `~/.wire_weaver/snapshots/`.
    Cache,
    /// The crate's directory in Cargo's sources (its `api_snapshots/` is copied into the cache).
    Cargo(PathBuf),
    /// Fetched from the network (and stored in the cache).
    Fetched,
}

/// Sets registered with [embed], read by [Resolver::global].
static EMBEDDED: RwLock<Vec<&'static Set>> = RwLock::new(Vec::new());

static GLOBAL: OnceLock<Resolver> = OnceLock::new();
static LOCAL: OnceLock<Resolver> = OnceLock::new();

/// Registers a set for [Resolver::global] and every resolver built with [Resolver::with_embedded]: the application
/// calls it at start with the `static` its build script wrote ([crate::embed]).
pub fn embed(set: &'static Set) {
    EMBEDDED
        .write()
        .expect("the embedded sets lock is poisoned")
        .push(set);
    for resolver in [GLOBAL.get(), LOCAL.get()].into_iter().flatten() {
        resolver.forget_misses();
    }
}

fn embedded() -> Vec<&'static Set> {
    EMBEDDED
        .read()
        .expect("the embedded sets lock is poisoned")
        .clone()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("a resolver lock is poisoned")
}

/// One source of the chain, in the order the resolver was built.
enum Source {
    Set(&'static Set),
    /// The sets registered with [embed], as they are at each lookup.
    Embedded,
    Dir(Dir),
    Cache(Dir),
    Cargo(CargoScan),
    #[cfg(feature = "fetch")]
    Fetch(crate::fetch::Fetcher),
    /// Another resolver asked after the sources before it, with what it remembers.
    Fallback(&'static Resolver),
}

/// A source expanded for one lookup.
enum Place<'s> {
    Set(&'static Set),
    /// A directory, and whether it is the cache.
    Dir(&'s Dir, bool),
    Cargo(&'s CargoScan),
    #[cfg(feature = "fetch")]
    Fetch(&'s crate::fetch::Fetcher),
    Fallback(&'static Resolver),
}

/// Where the resolver looks for crates in Cargo's sources on this machine.
#[derive(Clone, Debug, Default)]
pub struct CargoScan {
    /// A directory of the workspace whose `Cargo.lock` says which sources are in use (the current directory for a
    /// host tool); `None` to go by `cargo_home` alone.
    pub workspace: Option<PathBuf>,
    /// `CARGO_HOME`, scanned for fetched registry crates and git checkouts.
    pub cargo_home: Option<PathBuf>,
}

/// Lookup key of a superseded definition: kind, crate version, name, reference.
type SupersededKey = (Kind, FullVersionOwned, String, Vec<u8>);

/// Finds snapshots by crate name and version through its sources, in the order they were added, each found once per
/// version. Shared between threads: a host keeps one for the process, [Resolver::global].
pub struct Resolver {
    sources: Vec<Source>,
    definitions: Option<PathBuf>,
    memo: FrozenMap<FullVersionOwned, Box<ApiBundleOwned>>,
    superseded: FrozenMap<SupersededKey, Box<Option<ApiBundleOwned>>>,
    origins: Mutex<HashMap<FullVersionOwned, Origin>>,
    missing: Mutex<HashSet<FullVersionOwned>>,
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Resolver {
    /// A resolver with no source: codegen and the checker add the walked crates' directories with
    /// [Self::with_crate_dirs] and nothing else, so a build sees only its sources.
    pub fn new() -> Self {
        Resolver {
            sources: vec![],
            definitions: None,
            memo: FrozenMap::new(),
            superseded: FrozenMap::new(),
            origins: Mutex::new(HashMap::new()),
            missing: Mutex::new(HashSet::new()),
        }
    }

    /// A host's resolver: [Self::with_host_sources], then the fetch (feature `fetch`) as `~/.wire_weaver/sources.toml`
    /// allows it ([cache::Sources]).
    pub fn host() -> Self {
        let resolver = Self::new().with_host_sources();
        #[cfg(feature = "fetch")]
        let resolver = {
            let sources = match cache::default_root().map(|root| cache::Sources::read(&root)) {
                Some(Ok(sources)) => sources,
                Some(Err(e)) => {
                    warn!("{e:#}: fetching as if sources.toml were absent");
                    cache::Sources::read_env(cache::Sources::default())
                }
                None => cache::Sources::read_env(cache::Sources::default()),
            };
            resolver.with_fetch(sources)
        };
        resolver
    }

    /// Adds what a host on this machine has, after the sources added before: the embedded sets, the cache and the
    /// definition cache under `~/.wire_weaver`, Cargo's sources through the current directory's workspace and
    /// `CARGO_HOME`. The checker adds them after the walked crates' directories, for a frozen snapshot that names a
    /// crate the walk didn't reach.
    pub fn with_host_sources(mut self) -> Self {
        self = self.with_embedded();
        if let Some(root) = cache::default_root() {
            self = self.with_cache(&root);
        }
        if let Some(dir) = definition_cache::default_dir() {
            self = self.with_definitions(dir);
        }
        self.with_cargo_scan(CargoScan {
            workspace: std::env::current_dir().ok(),
            cargo_home: cargo::default_cargo_home().ok(),
        })
    }

    /// The process's [Self::host] resolver: what hosts (clients, `ww`, Python) resolve through.
    pub fn global() -> &'static Resolver {
        GLOBAL.get_or_init(Self::host)
    }

    /// The process's resolver over what this machine has ([Self::with_host_sources]), never the network: the
    /// evolution checker's fallback after the walked crates' directories, so a check is the same offline.
    pub fn local() -> &'static Resolver {
        LOCAL.get_or_init(|| Self::new().with_host_sources())
    }

    /// Adds an embedded set.
    pub fn with_set(mut self, set: &'static Set) -> Self {
        self.sources.push(Source::Set(set));
        self
    }

    /// Adds the sets registered with [embed], as they are at each lookup.
    pub fn with_embedded(mut self) -> Self {
        self.sources.push(Source::Embedded);
        self
    }

    /// Adds a directory in the `api_snapshots/` layout.
    pub fn with_dir(mut self, dir: Dir) -> Self {
        self.sources.push(Source::Dir(dir));
        self
    }

    /// Adds the `api_snapshots/` of every crate directory that has one, in order, each once.
    pub fn with_crate_dirs<P: AsRef<Path>>(
        mut self,
        crate_dirs: impl IntoIterator<Item = P>,
    ) -> Self {
        for crate_dir in crate_dirs {
            let dir = Dir::of_crate(crate_dir.as_ref());
            let known = self
                .sources
                .iter()
                .any(|s| matches!(s, Source::Dir(d) if *d == dir));
            if dir.exists() && !known {
                self.sources.push(Source::Dir(dir));
            }
        }
        self
    }

    /// Adds the cache rooted at `root` (`~/.wire_weaver`), which a hit in a source after it is copied into.
    pub fn with_cache(mut self, root: &Path) -> Self {
        self.sources.push(Source::Cache(cache::snapshots_dir(root)));
        self
    }

    /// The definition cache, [ww_self::definition_cache], looked up by signature after every source's `superseded/`.
    pub fn with_definitions(mut self, dir: PathBuf) -> Self {
        self.definitions = Some(dir);
        self
    }

    /// Adds Cargo's sources on this machine.
    pub fn with_cargo_scan(mut self, scan: CargoScan) -> Self {
        self.sources.push(Source::Cargo(scan));
        self
    }

    /// Adds the network, after every source before it: crates.io, then the repository `sources` names for the crate
    /// ([crate::fetch]). Nothing is added when `sources` say offline. What a fetch finds goes into the cache added
    /// with [Self::with_cache], when there is one.
    #[cfg(feature = "fetch")]
    pub fn with_fetch(mut self, sources: cache::Sources) -> Self {
        if sources.offline {
            debug!("offline: no fetch");
            return self;
        }
        let cache = self.cache().cloned();
        self.sources
            .push(Source::Fetch(crate::fetch::Fetcher::new(sources, cache)));
        self
    }

    /// Adds another resolver, asked after the sources before it: the checker puts the walked crates' directories
    /// before [Self::local], so what the process already found or looked for in vain is not looked for again.
    pub fn with_fallback(mut self, resolver: &'static Resolver) -> Self {
        self.sources.push(Source::Fallback(resolver));
        self
    }

    /// The sources with their position, the embedded sets expanded.
    fn places(&self) -> Vec<(usize, Place<'_>)> {
        let mut places = vec![];
        for (index, source) in self.sources.iter().enumerate() {
            match source {
                Source::Set(set) => places.push((index, Place::Set(set))),
                Source::Embedded => {
                    places.extend(embedded().into_iter().map(|s| (index, Place::Set(s))));
                }
                Source::Dir(dir) => places.push((index, Place::Dir(dir, false))),
                Source::Cache(dir) => places.push((index, Place::Dir(dir, true))),
                Source::Cargo(scan) => places.push((index, Place::Cargo(scan))),
                #[cfg(feature = "fetch")]
                Source::Fetch(fetcher) => places.push((index, Place::Fetch(fetcher))),
                Source::Fallback(resolver) => places.push((index, Place::Fallback(resolver))),
            }
        }
        places
    }

    /// The directories added with [Self::with_dir] and [Self::with_crate_dirs], in order.
    pub fn dirs(&self) -> impl Iterator<Item = &Dir> {
        self.sources.iter().filter_map(|s| match s {
            Source::Dir(dir) => Some(dir),
            _ => None,
        })
    }

    /// The cache, when there is one.
    fn cache(&self) -> Option<&Dir> {
        self.sources.iter().find_map(|s| match s {
            Source::Cache(dir) => Some(dir),
            _ => None,
        })
    }

    /// The snapshot of `version`, the first source that has one. Found once: later calls answer from memory.
    pub fn snapshot(&self, version: &FullVersionOwned) -> Option<&ApiBundleOwned> {
        if let Some(bundle) = self.memo.get(version) {
            return Some(bundle);
        }
        if !file::is_plain_crate_name(&version.crate_id) {
            warn!(
                "`{}` is not a crate name: no snapshot looked for",
                version.crate_id
            );
            return None;
        }
        if lock(&self.missing).contains(version) {
            return None;
        }
        match self.every_snapshot(version, true).into_iter().next() {
            Some((origin, bundle)) => {
                debug!(
                    "snapshot of {} {} from {origin:?}",
                    version.crate_id,
                    file::triplet(&version.version)
                );
                lock(&self.origins).insert(version.clone(), origin);
                Some(self.memo.insert(version.clone(), Box::new(bundle)))
            }
            None => {
                lock(&self.missing).insert(version.clone());
                None
            }
        }
    }

    /// Where the snapshot of `version` came from, once [Self::snapshot] found it.
    pub fn origin(&self, version: &FullVersionOwned) -> Option<Origin> {
        lock(&self.origins).get(version).cloned()
    }

    /// Forgets which versions had no snapshot, so the next lookup tries the sources again (a set was embedded, a
    /// file arrived).
    pub fn forget_misses(&self) {
        lock(&self.missing).clear();
    }

    /// The snapshot of `version` in every source that has one, in chain order; only the first with `first`. A hit
    /// in a source after the cache is copied into the cache.
    fn every_snapshot(
        &self,
        version: &FullVersionOwned,
        first: bool,
    ) -> Vec<(Origin, ApiBundleOwned)> {
        let mut found = vec![];
        let mut past_cache = false;
        for (index, place) in self.places() {
            match place {
                Place::Set(set) => {
                    if let Some(bundle) = set.get(version) {
                        found.push((Origin::Set(index), bundle.clone()));
                    }
                }
                Place::Dir(dir, is_cache) => {
                    past_cache |= is_cache;
                    if let Some(snapshot) = read_snapshot(dir, version) {
                        let origin = if is_cache {
                            Origin::Cache
                        } else {
                            Origin::Dir(dir.path().to_path_buf())
                        };
                        found.push((origin, snapshot.bundle));
                    }
                }
                Place::Cargo(scan) => {
                    for crate_dir in self.cargo_dirs(scan, version) {
                        let dir = Dir::of_crate(&crate_dir);
                        if let Some(snapshot) = read_snapshot(&dir, version) {
                            if past_cache {
                                self.store(&dir);
                            }
                            found.push((Origin::Cargo(crate_dir), snapshot.bundle));
                            if first {
                                return found;
                            }
                        }
                    }
                }
                #[cfg(feature = "fetch")]
                Place::Fetch(fetcher) => {
                    if let Some(snapshot) = fetcher.snapshot(version) {
                        found.push((Origin::Fetched, snapshot.bundle));
                    }
                }
                Place::Fallback(resolver) => {
                    if first {
                        if let Some(bundle) = resolver.snapshot(version) {
                            let origin = resolver.origin(version).unwrap_or(Origin::Set(index));
                            found.push((origin, bundle.clone()));
                        }
                    } else {
                        found.extend(resolver.every_snapshot(version, false));
                    }
                }
            }
            if first && !found.is_empty() {
                return found;
            }
        }
        found
    }

    /// Copies a crate's snapshots found outside the cache into it, so the next process finds them there.
    fn store(&self, from: &Dir) {
        let Some(cache) = self.cache() else { return };
        match cache::store_crate(cache, from) {
            Ok(stored) => {
                for (path, why) in &stored.refused {
                    warn!("{}: {why}", path.display());
                }
                debug!(
                    "{} copied into {}: {stored:?}",
                    from.path().display(),
                    cache.path().display()
                );
            }
            Err(e) => warn!("copying {} into the cache: {e:#}", from.path().display()),
        }
    }

    /// Directories of `version` in Cargo's sources (`scan`) that have an `api_snapshots/`: what the workspace's
    /// `Cargo.lock` places, else what a scan of `CARGO_HOME` finds.
    fn cargo_dirs(&self, scan: &CargoScan, version: &FullVersionOwned) -> Vec<PathBuf> {
        let v = &version.version;
        let semver = semver::Version::new(v.major.0.into(), v.minor.0.into(), v.patch.0.into());
        let mut dirs = vec![];
        if let Some(workspace) = &scan.workspace {
            let mut sources = CargoSources::default();
            sources.entry_dir = Some(workspace.clone());
            sources.cargo_home = scan.cargo_home.clone();
            match sources.locate_version(&version.crate_id, &semver) {
                Ok(found) => dirs.extend(found),
                Err(e) => debug!("no Cargo.lock to go by: {e:#}"),
            }
        }
        if dirs.is_empty()
            && let Some(home) = &scan.cargo_home
        {
            dirs.extend(cargo::scan_fetched(home, &version.crate_id, &semver));
        }
        dirs.dedup();
        dirs.retain(|dir| Dir::of_crate(dir).exists());
        dirs
    }

    /// The canonical bundle of the `kind` named `name` of `version` that `reference` (a `Skipped*` location's
    /// signature bytes) names, when the snapshot found has the definition under another signature: each source's
    /// `superseded/` (EVO-68), the definition cache, then the same version's snapshot in the other sources. `Ok(None)`
    /// when no source has it; a damaged file is named in the log and skipped.
    pub fn by_signature(
        &self,
        kind: Kind,
        version: &FullVersionOwned,
        name: &str,
        reference: &[u8],
    ) -> Result<Option<ApiBundleOwned>> {
        if !file::is_plain_crate_name(&version.crate_id)
            || !definition_cache::is_plain_definition_name(name)
        {
            warn!(
                "`{}` `{name}` is not a crate and definition name: not looked up",
                version.crate_id
            );
            return Ok(None);
        }
        if let Some(found) = self.superseded(kind, version, name, reference) {
            return Ok(Some(found.clone()));
        }
        if let Some(definitions) = &self.definitions
            && let Some(found) =
                definition_cache::load(definitions, kind, version, name, reference)?
        {
            return Ok(Some(found));
        }
        let origin = self.origin(version);
        for (candidate, bundle) in self.every_snapshot(version, false) {
            if origin.as_ref() == Some(&candidate) {
                continue;
            }
            if let Some(found) = self.definition_in(&bundle, kind, version, name, reference) {
                debug!(
                    "{} {name} found by signature in the snapshot from {candidate:?}",
                    kind.name()
                );
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    /// A superseded definition (EVO-68) of `version` that `reference` names, from the first source that keeps one;
    /// looked up once per key, a miss remembered too.
    pub fn superseded(
        &self,
        kind: Kind,
        version: &FullVersionOwned,
        name: &str,
        reference: &[u8],
    ) -> Option<&ApiBundleOwned> {
        let key = (kind, version.clone(), name.to_owned(), reference.to_vec());
        if let Some(found) = self.superseded.get(&key) {
            return found.as_ref();
        }
        if !file::is_plain_crate_name(&version.crate_id)
            || !definition_cache::is_plain_definition_name(name)
        {
            return None;
        }
        let found = self.find_superseded(kind, version, name, reference);
        self.superseded.insert(key, Box::new(found)).as_ref()
    }

    fn find_superseded(
        &self,
        kind: Kind,
        version: &FullVersionOwned,
        name: &str,
        reference: &[u8],
    ) -> Option<ApiBundleOwned> {
        for (_, place) in self.places() {
            let dirs = match place {
                Place::Set(set) => {
                    if let Some(found) = set.superseded(kind, version, name, reference) {
                        return Some(found.clone());
                    }
                    continue;
                }
                Place::Dir(dir, _) => vec![dir.clone()],
                Place::Cargo(scan) => self
                    .cargo_dirs(scan, version)
                    .iter()
                    .map(|d| Dir::of_crate(d))
                    .collect(),
                #[cfg(feature = "fetch")]
                Place::Fetch(fetcher) => {
                    if let Some(found) = fetcher.superseded(kind, version, name, reference) {
                        return Some(found);
                    }
                    continue;
                }
                Place::Fallback(resolver) => {
                    if let Some(found) = resolver.superseded(kind, version, name, reference) {
                        return Some(found.clone());
                    }
                    continue;
                }
            };
            for dir in dirs {
                match dir.superseded(kind, version, name, reference) {
                    Ok(Some(found)) => return Some(found),
                    Ok(None) => {}
                    Err(e) => warn!("{}: {e:#}", dir.superseded_dir().display()),
                }
            }
        }
        None
    }

    /// The canonical bundle of the `kind` named `name` in `bundle`, when its signature matches `reference`.
    fn definition_in(
        &self,
        bundle: &ApiBundleOwned,
        kind: Kind,
        version: &FullVersionOwned,
        name: &str,
        reference: &[u8],
    ) -> Option<ApiBundleOwned> {
        let resolve = &|v: &FullVersionOwned| self.snapshot(v);
        let bytes = match kind {
            Kind::Type => {
                let idx = inline::find_type(bundle, version, name)?;
                signature::canonical_type_bytes(bundle, idx, resolve)
            }
            Kind::Trait => {
                let idx = inline::find_trait(bundle, version, name)?;
                signature::canonical_trait_bytes(bundle, idx, resolve)
            }
        };
        match bytes.and_then(|bytes| definition_cache::decode(kind, name, reference, &bytes)) {
            Ok(found) => Some(found),
            Err(e) => {
                debug!("{} {name} of {}: {e:#}", kind.name(), version.crate_id);
                None
            }
        }
    }

    /// Puts the skipped definitions of `bundle` back from the snapshots this resolver finds, and by signature where a
    /// snapshot has a definition under another one ([ww_self::inline::inline_skipped_by_signature]).
    pub fn inline_skipped(&self, bundle: &mut ApiBundleOwned) -> Inlined {
        inline::inline_skipped_by_signature(
            bundle,
            &|v| self.snapshot(v),
            Some(&|kind, version, name, reference| {
                self.by_signature(kind, version, name, reference)
            }),
        )
    }
}

/// A directory's snapshot of `version`; a damaged file is named in the log and counts as none.
fn read_snapshot(dir: &Dir, version: &FullVersionOwned) -> Option<file::Snapshot> {
    match dir.snapshot(version) {
        Ok(found) => found,
        Err(e) => {
            warn!("{e:#}");
            None
        }
    }
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Set(set) => write!(f, "Set({set:?})"),
            Source::Embedded => write!(f, "Embedded({})", embedded().len()),
            Source::Dir(dir) => write!(f, "Dir({})", dir.path().display()),
            Source::Cache(dir) => write!(f, "Cache({})", dir.path().display()),
            Source::Cargo(scan) => write!(f, "Cargo({scan:?})"),
            #[cfg(feature = "fetch")]
            Source::Fetch(_) => write!(f, "Fetch"),
            Source::Fallback(_) => write!(f, "Fallback"),
        }
    }
}

impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver")
            .field("sources", &self.sources)
            .field("definitions", &self.definitions)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use ww_version::VersionOwned;

    fn bundle_of(crate_id: &str, trait_name: &str) -> ApiBundleOwned {
        ApiBundleOwned {
            magic: ww_self::MAGIC,
            ww_self_version: ww_self::VERSION,
            root: ww_self::ApiLevelOwned {
                docs: vec![],
                crate_idx: 0.into(),
                compact_id: None,
                meta: None,
                trait_name: trait_name.to_owned(),
                items: vec![],
            },
            types: vec![],
            traits: vec![],
            ext_crates: vec![FullVersionOwned::new(
                crate_id.to_owned(),
                VersionOwned::new(0, 1, 0),
            )],
            root_omitted: None,
            wire: None,
            ext_wire: None,
        }
    }

    fn write_snapshot(crate_dir: &Path, bundle: &ApiBundleOwned) {
        let dir = Dir::of_crate(crate_dir);
        fs::create_dir_all(dir.path()).unwrap();
        fs::write(
            dir.file_path(&bundle.ext_crates[0]),
            file::render_ron(bundle).unwrap(),
        )
        .unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ww_resolver_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_first_source_wins_and_is_remembered() {
        let root = scratch("order");
        let first = bundle_of("dep", "First");
        let second = bundle_of("dep", "Second");
        write_snapshot(&root.join("a"), &first);
        write_snapshot(&root.join("b"), &second);
        let version = first.ext_crates[0].clone();
        let resolver = Resolver::new().with_crate_dirs([
            root.join("a"),
            root.join("b"),
            root.join("none"),
            root.join("a"),
        ]);
        assert_eq!(resolver.sources.len(), 2);
        let found = resolver.snapshot(&version).unwrap();
        assert_eq!(found.root.trait_name, "First");
        assert_eq!(
            resolver.origin(&version),
            Some(Origin::Dir(root.join("a").join("api_snapshots")))
        );
        assert_eq!(resolver.every_snapshot(&version, false).len(), 2);
        fs::remove_dir_all(&root).unwrap();
        // from memory now
        assert_eq!(
            resolver.snapshot(&version).unwrap().root.trait_name,
            "First"
        );
        let other = FullVersionOwned::new("other".into(), VersionOwned::new(1, 0, 0));
        assert!(resolver.snapshot(&other).is_none());
        let bad = FullVersionOwned::new("../dep".into(), VersionOwned::new(0, 1, 0));
        assert!(resolver.snapshot(&bad).is_none());
    }

    #[test]
    fn a_set_comes_before_directories() {
        let root = scratch("set");
        let in_file = bundle_of("dep", "FromFile");
        write_snapshot(&root.join("a"), &in_file);
        let embedded = bundle_of("dep", "FromSet");
        let bytes: &'static [u8] = Box::leak(
            ww_self::snapshot::encode(&embedded)
                .unwrap()
                .into_boxed_slice(),
        );
        let name: &'static str =
            Box::leak(file::file_name(&embedded.ext_crates[0]).into_boxed_str());
        let snapshots: &'static [(&'static str, &'static [u8])] =
            Box::leak(vec![(name, bytes)].into_boxed_slice());
        let set: &'static Set = Box::leak(Box::new(Set::new(snapshots, &[])));
        let resolver = Resolver::new()
            .with_set(set)
            .with_crate_dirs([root.join("a")]);
        let version = embedded.ext_crates[0].clone();
        assert_eq!(
            resolver.snapshot(&version).unwrap().root.trait_name,
            "FromSet"
        );
        assert_eq!(resolver.origin(&version), Some(Origin::Set(0)));
        assert_eq!(resolver.every_snapshot(&version, false).len(), 2);
        // added after the directory, the set comes second
        let resolver = Resolver::new()
            .with_crate_dirs([root.join("a")])
            .with_set(set);
        assert_eq!(
            resolver.snapshot(&version).unwrap().root.trait_name,
            "FromFile"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_crate_in_cargos_sources_is_copied_into_the_cache() {
        let root = scratch("cargo");
        let home = root.join("cargo_home");
        let bundle = bundle_of("dep", "Fetched");
        let crate_dir = home.join("registry/src/index-abc/dep-0.1.0");
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"dep\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        write_snapshot(&crate_dir, &bundle);
        let cache_root = root.join("wire_weaver");
        let version = bundle.ext_crates[0].clone();
        let resolver = Resolver::new()
            .with_cache(&cache_root)
            .with_cargo_scan(CargoScan {
                workspace: None,
                cargo_home: Some(home.clone()),
            });
        assert_eq!(
            resolver.snapshot(&version).unwrap().root.trait_name,
            "Fetched"
        );
        assert_eq!(resolver.origin(&version), Some(Origin::Cargo(crate_dir)));
        let cached = cache::snapshots_dir(&cache_root);
        assert!(
            cached.file_path(&version).is_file(),
            "copied into the cache"
        );
        // the next resolver finds it in the cache without Cargo's sources
        let next = Resolver::new().with_cache(&cache_root);
        assert_eq!(next.snapshot(&version).unwrap().root.trait_name, "Fetched");
        assert_eq!(next.origin(&version), Some(Origin::Cache));
        fs::remove_dir_all(&root).unwrap();
    }
}
