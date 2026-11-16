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
    /// An embedded set, by its position among the resolver's sets.
    Set(usize),
    /// A directory the caller named: a walked crate's `api_snapshots/`.
    Dir(PathBuf),
    /// The cache, `~/.wire_weaver/snapshots/`.
    Cache,
    /// The crate's directory in Cargo's sources (its `api_snapshots/` is copied into the cache).
    Cargo(PathBuf),
}

/// Sets registered with [embed], read by [Resolver::global].
static EMBEDDED: RwLock<Vec<&'static Set>> = RwLock::new(Vec::new());

static GLOBAL: OnceLock<Resolver> = OnceLock::new();

/// Registers a set for [Resolver::global] and every resolver built with [Resolver::with_embedded]: the application
/// calls it at start with the `static` its build script wrote ([crate::embed]).
pub fn embed(set: &'static Set) {
    EMBEDDED
        .write()
        .expect("the embedded sets lock is poisoned")
        .push(set);
    if let Some(global) = GLOBAL.get() {
        global.forget_misses();
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

enum Sets {
    Own(Vec<&'static Set>),
    Embedded,
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

/// Finds snapshots by crate name and version through the chain of sources it was built with, each found once per
/// version. Shared between threads: a host keeps one for the process, [Resolver::global].
pub struct Resolver {
    sets: Sets,
    dirs: Vec<Dir>,
    cache: Option<Dir>,
    definitions: Option<PathBuf>,
    cargo: Option<CargoScan>,
    memo: FrozenMap<FullVersionOwned, Box<ApiBundleOwned>>,
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
            sets: Sets::Own(vec![]),
            dirs: vec![],
            cache: None,
            definitions: None,
            cargo: None,
            memo: FrozenMap::new(),
            origins: Mutex::new(HashMap::new()),
            missing: Mutex::new(HashSet::new()),
        }
    }

    /// A host's resolver: the embedded sets, the cache and the definition cache under `~/.wire_weaver`, Cargo's
    /// sources through the current directory's workspace and `CARGO_HOME`.
    pub fn host() -> Self {
        let mut resolver = Self::new().with_embedded();
        if let Some(root) = cache::default_root() {
            resolver = resolver.with_cache(&root);
        }
        if let Some(dir) = definition_cache::default_dir() {
            resolver = resolver.with_definitions(dir);
        }
        resolver.with_cargo_scan(CargoScan {
            workspace: std::env::current_dir().ok(),
            cargo_home: cargo::default_cargo_home().ok(),
        })
    }

    /// The process's [Self::host] resolver.
    pub fn global() -> &'static Resolver {
        GLOBAL.get_or_init(Self::host)
    }

    /// Adds an embedded set, after those added before.
    pub fn with_set(mut self, set: &'static Set) -> Self {
        match &mut self.sets {
            Sets::Own(sets) => sets.push(set),
            Sets::Embedded => self.sets = Sets::Own(embedded().into_iter().chain([set]).collect()),
        }
        self
    }

    /// Uses the sets registered with [embed], as they are at each lookup.
    pub fn with_embedded(mut self) -> Self {
        self.sets = Sets::Embedded;
        self
    }

    /// Adds a directory in the `api_snapshots/` layout, after those added before.
    pub fn with_dir(mut self, dir: Dir) -> Self {
        self.dirs.push(dir);
        self
    }

    /// Adds the `api_snapshots/` of every crate directory that has one, in order.
    pub fn with_crate_dirs<P: AsRef<Path>>(
        mut self,
        crate_dirs: impl IntoIterator<Item = P>,
    ) -> Self {
        for crate_dir in crate_dirs {
            let dir = Dir::of_crate(crate_dir.as_ref());
            if dir.exists() && !self.dirs.contains(&dir) {
                self.dirs.push(dir);
            }
        }
        self
    }

    /// The cache rooted at `root` (`~/.wire_weaver`): read after the directories, written with what Cargo's sources
    /// yield.
    pub fn with_cache(mut self, root: &Path) -> Self {
        self.cache = Some(cache::snapshots_dir(root));
        self
    }

    /// The definition cache, [ww_self::definition_cache], looked up by signature.
    pub fn with_definitions(mut self, dir: PathBuf) -> Self {
        self.definitions = Some(dir);
        self
    }

    /// Cargo's sources on this machine, after the cache.
    pub fn with_cargo_scan(mut self, scan: CargoScan) -> Self {
        self.cargo = Some(scan);
        self
    }

    fn sets(&self) -> Vec<&'static Set> {
        match &self.sets {
            Sets::Own(sets) => sets.clone(),
            Sets::Embedded => embedded(),
        }
    }

    /// The directories named and the cache, in order.
    fn dirs(&self) -> impl Iterator<Item = &Dir> {
        self.dirs.iter().chain(self.cache.iter())
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
        match self.find(version) {
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

    fn find(&self, version: &FullVersionOwned) -> Option<(Origin, ApiBundleOwned)> {
        for (index, set) in self.sets().iter().enumerate() {
            if let Some(bundle) = set.get(version) {
                return Some((Origin::Set(index), bundle.clone()));
            }
        }
        for dir in self.dirs() {
            if let Some(snapshot) = read_snapshot(dir, version) {
                let origin = if Some(dir) == self.cache.as_ref() {
                    Origin::Cache
                } else {
                    Origin::Dir(dir.path().to_path_buf())
                };
                return Some((origin, snapshot.bundle));
            }
        }
        for crate_dir in self.cargo_dirs(version) {
            let dir = Dir::of_crate(&crate_dir);
            if let Some(snapshot) = read_snapshot(&dir, version) {
                self.store(&dir);
                return Some((Origin::Cargo(crate_dir), snapshot.bundle));
            }
        }
        None
    }

    /// Copies a crate's snapshots found outside the cache into it, so the next process finds them there.
    fn store(&self, from: &Dir) {
        let Some(cache) = &self.cache else { return };
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

    /// Directories of `version` in Cargo's sources that have an `api_snapshots/`.
    fn cargo_dirs(&self, version: &FullVersionOwned) -> Vec<PathBuf> {
        let Some(scan) = &self.cargo else {
            return vec![];
        };
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
        if let Some(home) = &scan.cargo_home {
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
            return Ok(Some(found));
        }
        if let Some(definitions) = &self.definitions
            && let Some(found) =
                definition_cache::load(definitions, kind, version, name, reference)?
        {
            return Ok(Some(found));
        }
        let origin = self.origin(version);
        for (candidate, bundle) in self.every_snapshot(version) {
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

    /// A superseded definition (EVO-68) of `version` that `reference` names, from the first source that keeps one.
    pub fn superseded(
        &self,
        kind: Kind,
        version: &FullVersionOwned,
        name: &str,
        reference: &[u8],
    ) -> Option<ApiBundleOwned> {
        for set in self.sets() {
            if let Some(found) = set.superseded(kind, version, name, reference) {
                return Some(found.clone());
            }
        }
        let cargo_dirs = self.cargo_dirs(version);
        for dir in self
            .dirs()
            .cloned()
            .chain(cargo_dirs.iter().map(|d| Dir::of_crate(d)))
        {
            match dir.superseded(kind, version, name, reference) {
                Ok(Some(found)) => return Some(found),
                Ok(None) => {}
                Err(e) => warn!("{}: {e:#}", dir.superseded_dir().display()),
            }
        }
        None
    }

    /// The snapshot of `version` in every source that has one, in chain order.
    fn every_snapshot(&self, version: &FullVersionOwned) -> Vec<(Origin, ApiBundleOwned)> {
        let mut found = vec![];
        for (index, set) in self.sets().iter().enumerate() {
            if let Some(bundle) = set.get(version) {
                found.push((Origin::Set(index), bundle.clone()));
            }
        }
        for dir in self.dirs() {
            if let Some(snapshot) = read_snapshot(dir, version) {
                let origin = if Some(dir) == self.cache.as_ref() {
                    Origin::Cache
                } else {
                    Origin::Dir(dir.path().to_path_buf())
                };
                found.push((origin, snapshot.bundle));
            }
        }
        for crate_dir in self.cargo_dirs(version) {
            if let Some(snapshot) = read_snapshot(&Dir::of_crate(&crate_dir), version) {
                found.push((Origin::Cargo(crate_dir), snapshot.bundle));
            }
        }
        found
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

impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver")
            .field("sets", &self.sets().len())
            .field("dirs", &self.dirs)
            .field("cache", &self.cache)
            .field("definitions", &self.definitions)
            .field("cargo", &self.cargo)
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
        let resolver =
            Resolver::new().with_crate_dirs([root.join("a"), root.join("b"), root.join("none")]);
        assert_eq!(resolver.dirs.len(), 2);
        let found = resolver.snapshot(&version).unwrap();
        assert_eq!(found.root.trait_name, "First");
        assert_eq!(
            resolver.origin(&version),
            Some(Origin::Dir(root.join("a").join("api_snapshots")))
        );
        fs::remove_dir_all(&root).unwrap();
        // from memory now
        assert_eq!(
            resolver.snapshot(&version).unwrap().root.trait_name,
            "First"
        );
        let other = FullVersionOwned::new("other".into(), VersionOwned::new(1, 0, 0));
        assert!(resolver.snapshot(&other).is_none());
        assert!(
            resolver
                .snapshot(&FullVersionOwned::new(
                    "../dep".into(),
                    VersionOwned::new(0, 1, 0)
                ))
                .is_none()
        );
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
        let snapshots: &'static [(&'static str, &'static [u8])] = Box::leak(
            vec![(
                &*Box::leak(file::file_name(&embedded.ext_crates[0]).into_boxed_str()),
                bytes,
            )]
            .into_boxed_slice(),
        );
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
        assert_eq!(resolver.every_snapshot(&version).len(), 2);
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
