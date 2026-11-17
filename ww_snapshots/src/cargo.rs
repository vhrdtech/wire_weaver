//! Where Cargo keeps the source of a git or registry dependency: the exact package is pinned by the `Cargo.lock` the
//! crate the walk started from is built with (in a proc macro, that of the workspace being built, [crate::rustc]), its
//! source lies under `CARGO_HOME` once Cargo fetched it
//! (`git/checkouts/<repo>-<hash>/<short rev>/`, `registry/src/<index>/<name>-<version>/`). The same places answer
//! where a crate of a given name and version is on this machine ([CargoSources::locate_version], [scan_fetched]),
//! which is where its `api_snapshots/` are.

use crate::cargo_config;
use crate::limits::{
    CARGO_DIRS_SCANNED_MAX, CARGO_LOCK_BYTES_MAX, CARGO_TREE_DIRS_MAX, CHECKOUT_LOCKS_MAX,
};
use crate::rustc::RustcBuild;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// How a manifest asks for a dependency that isn't a path.
pub enum DepSpec<'a> {
    /// `name = "1.2"` or `name = { version = "1.2" }`, from crates.io or another registry.
    Registry {
        version_req: Option<&'a semver::VersionReq>,
    },
    /// `name = { git = "https://.." }`, with or without `branch`, `tag` or `rev`.
    Git { url: &'a str },
}

/// The package a dependency is looked up for.
pub struct Dependent<'a> {
    pub name: &'a str,
    pub version: &'a semver::Version,
    /// Directory with its `Cargo.toml`, canonical.
    pub dir: &'a Path,
}

/// Told every file read and directory listed or looked into, so that a caller caching what a walk depended on knows
/// when to walk again.
pub type ReadNote = Box<dyn Fn(&Path)>;

/// Cargo.lock and the fetched sources it points at, loaded once per walk.
#[derive(Default)]
pub struct CargoSources {
    /// Crate the walk started from: the `Cargo.lock` it is built with pins every git and registry dependency, also
    /// those of dependencies (a git checkout's own `Cargo.lock` isn't the one in use). That is the nearest one above
    /// it, except in a proc macro of a crate built as a dependency of another workspace ([Self::rustc]).
    pub entry_dir: Option<PathBuf>,
    /// `CARGO_HOME` to use instead of the environment's (tests).
    pub cargo_home: Option<PathBuf>,
    /// Called with every path read or looked into, `None` when nobody cares.
    pub reads: Option<ReadNote>,
    /// The rustc build the walk runs in, `None` for the one of this process, if any ([RustcBuild::from_process]).
    pub rustc: Option<RustcBuild>,
    lock: Option<Rc<CargoLock>>,
    /// [Self::lock] is the one of the rustc build the walk runs in, checked against what it compiled.
    lock_is_build: bool,
    /// Packages in a git checkout, by name, each with its directory.
    git_members: HashMap<PathBuf, Rc<HashMap<String, PathBuf>>>,
}

#[derive(Deserialize)]
struct CargoLock {
    #[serde(skip)]
    path: PathBuf,
    #[serde(default)]
    package: Vec<LockedPackage>,
}

#[derive(Deserialize)]
struct LockedPackage {
    name: String,
    version: String,
    /// `None` for path dependencies and workspace members.
    source: Option<String>,
    /// `name`, `name version` or `name version (source)`, as many as needed to tell same-named packages apart.
    #[serde(default)]
    dependencies: Vec<String>,
}

impl CargoLock {
    /// Indices of the packages the entries of `locked.dependencies` name.
    fn dependencies<'l>(&'l self, locked: &'l LockedPackage) -> impl Iterator<Item = usize> + 'l {
        locked.dependencies.iter().flat_map(move |entry| {
            let mut parts = entry.split_whitespace();
            let name = parts.next().unwrap_or_default();
            let version = parts.next();
            let source = parts
                .next()
                .map(|s| s.trim_start_matches('(').trim_end_matches(')'));
            (0..self.package.len()).filter(move |&i| {
                let p = &self.package[i];
                p.name == name
                    && version.is_none_or(|v| p.version == v)
                    && source.is_none_or(|s| p.is_from(s))
            })
        })
    }

    /// Indices of `roots` and every package they depend on, directly or not, sorted.
    fn tree(&self, roots: &[usize]) -> Vec<usize> {
        let mut seen = vec![false; self.package.len()];
        let mut queue = roots.to_vec();
        while let Some(i) = queue.pop() {
            if std::mem::replace(&mut seen[i], true) {
                continue;
            }
            queue.extend(self.dependencies(&self.package[i]));
        }
        (0..seen.len()).filter(|&i| seen[i]).collect()
    }
}

/// The URL Cargo identifies a git source by, as its `CanonicalUrl` makes it from `url` (a URL as `Cargo.lock` writes
/// it, already normalized): no query or fragment, no trailing `/`, no `.git` at the end, and a GitHub URL in `https`
/// and lower case. Two spellings of one repository with different canonical URLs are two sources to Cargo.
fn canonical_git_url(url: &str) -> String {
    let url = url.split(['?', '#']).next().unwrap_or_default();
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let path_start = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(path_start);
    let mut path = path.to_owned();
    if path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    let mut scheme = scheme.to_owned();
    if !scheme.contains('+') {
        let host = authority.rsplit('@').next().unwrap_or_default();
        let host = host.split(':').next().unwrap_or_default();
        if host == "github.com" {
            "https".clone_into(&mut scheme);
            path = path.to_lowercase();
        }
        if let Some(stripped) = path.strip_suffix(".git") {
            path = stripped.to_owned();
        }
    }
    format!("{scheme}://{authority}{path}")
}

/// The last path segment of a canonical git URL ([canonical_git_url]), `_empty` for none: the start of its checkout
/// directory's name.
fn git_repo_name(canonical: &str) -> &str {
    let path = canonical
        .split_once("://")
        .map_or(canonical, |(_, rest)| rest)
        .split_once('/')
        .map_or("", |(_, path)| path);
    match path.rsplit('/').next() {
        Some(repo) if !repo.is_empty() => repo,
        _ => "_empty",
    }
}

/// Whether `dir_name` is a directory in `git/checkouts/` of a repository named `repo` ([git_repo_name]), under any
/// URL: `<repo>-<16 hex digits>`, maybe with `-shallow`.
fn is_checkout_of(dir_name: &str, repo: &str) -> bool {
    let Some(hash) = dir_name
        .strip_prefix(repo)
        .and_then(|rest| rest.strip_prefix('-'))
    else {
        return false;
    };
    let hash = hash.strip_suffix("-shallow").unwrap_or(hash);
    hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The directory in `git/checkouts/` Cargo checks out the git source `url` into.
///
/// It is `<repo>-<hash>`: `<repo>` is the last path segment of Cargo's canonical URL (no query, fragment, trailing
/// `/` or `.git`; GitHub in `https` and lower case), `<hash>` Cargo's stable hash of it (SipHash-1-3 128 of
/// `rustc-stable-hash`, the low 64 bits in little-endian hex). Cargo before 1.85 hashed differently; edition 2024,
/// which the walk's crates need, needs Cargo 1.85.
pub fn git_checkout_dir_name(url: &str) -> String {
    let canonical = canonical_git_url(url);
    let repo = git_repo_name(&canonical);
    let mut hasher = rustc_stable_hash::StableSipHasher128::new();
    canonical.as_str().hash(&mut hasher);
    let hash: String = Hasher::finish(&hasher)
        .to_le_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{repo}-{hash}")
}

/// `[package] name` of the manifest in `dir`.
fn package_name(dir: &Path) -> Result<String> {
    let path = dir.join("Cargo.toml");
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let head: ManifestHead =
        toml::from_str(&contents).with_context(|| format!("Failed to parse {}", path.display()))?;
    head.package
        .map(|p| p.name)
        .ok_or_else(|| anyhow!("no [package] in {}", path.display()))
}

enum Source<'a> {
    Registry,
    Git { url: &'a str, rev: &'a str },
}

impl LockedPackage {
    fn source(&self) -> Result<Option<Source<'_>>> {
        let Some(source) = &self.source else {
            return Ok(None);
        };
        if source.starts_with("registry+") || source.starts_with("sparse+") {
            return Ok(Some(Source::Registry));
        }
        if let Some(git) = source.strip_prefix("git+") {
            let (url, rev) = git
                .split_once('#')
                .ok_or_else(|| anyhow!("git source without a revision: {source}"))?;
            // `?branch=..`, `?tag=..`, `?rev=..` say what was asked for, `rev` is what it resolved to
            let url = url.split_once('?').map_or(url, |(url, _)| url);
            return Ok(Some(Source::Git { url, rev }));
        }
        Err(anyhow!(
            "unsupported source `{source}` of {} {} in Cargo.lock: only crates.io / registry, git and path \
             dependencies are supported",
            self.name,
            self.version
        ))
    }

    /// Whether this package comes from `source` as a dependency entry names it: `git+URL` without the `#rev` the
    /// package's own `source` carries, a registry in full.
    fn is_from(&self, source: &str) -> bool {
        self.source.as_deref().is_some_and(|own| {
            own == source
                || own
                    .split_once('#')
                    .is_some_and(|(unpinned, _)| unpinned == source)
        })
    }

    fn version_is(&self, version: &semver::Version) -> bool {
        semver::Version::parse(&self.version).is_ok_and(|v| &v == version)
    }

    fn matches(&self, spec: &DepSpec) -> bool {
        match (spec, self.source()) {
            (DepSpec::Registry { version_req }, Ok(Some(Source::Registry))) => version_req
                .is_none_or(|req| {
                    semver::Version::parse(&self.version).is_ok_and(|v| req.matches(&v))
                }),
            (DepSpec::Git { url }, Ok(Some(Source::Git { url: locked, .. }))) => {
                same_git_url(url, locked)
            }
            _ => false,
        }
    }
}

fn same_git_url(a: &str, b: &str) -> bool {
    let normalize = |url: &str| {
        url.trim_end_matches('/')
            .trim_end_matches(".git")
            .to_ascii_lowercase()
    };
    normalize(a) == normalize(b)
}

impl CargoSources {
    /// Tells [Self::reads] that `path` was read, listed or looked for.
    fn note(&self, path: &Path) {
        if let Some(reads) = &self.reads {
            reads(path);
        }
    }

    /// [Self::note] of the directory `path` is in: whether a file is there.
    fn note_parent(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            self.note(dir);
        }
    }

    /// Directory of package `package` that `dependent` asks for with `spec`.
    pub fn locate(
        &mut self,
        dependent: &Dependent,
        package: &str,
        spec: &DepSpec,
    ) -> Result<PathBuf> {
        if let DepSpec::Registry { version_req } = spec
            && self.entry_is_fetched()?
        {
            return self.registry_newest(package, *version_req);
        }
        let lock = self.lock()?;
        let locked = self
            .resolve(&lock, dependent, package, spec)
            .with_context(|| format!("resolving {package} in {}", lock.path.display()))?;
        if let Some(dir) = self.package_dir(locked)? {
            return Ok(dir);
        }
        self.patched_dir(&lock.path, package)?.ok_or_else(|| {
            anyhow!(
                "{} {} has no source in {}: a path dependency there but not in the manifest, nor a [patch] of \
                 the workspace there",
                locked.name,
                locked.version,
                lock.path.display()
            )
        })
    }

    fn resolve<'l>(
        &mut self,
        lock: &'l CargoLock,
        dependent: &Dependent,
        package: &str,
        spec: &DepSpec,
    ) -> Result<&'l LockedPackage> {
        let mut candidates: Vec<&LockedPackage> = match self.locked_dependent(lock, dependent)? {
            // the dependent's own list says exactly which one it uses
            Some(locked) => lock
                .dependencies(locked)
                .filter(|&i| lock.package[i].name == package)
                .map(|i| &lock.package[i])
                .collect(),
            None => lock.package.iter().filter(|p| p.name == package).collect(),
        };
        if candidates.len() > 1 {
            candidates.retain(|p| p.matches(spec));
        }
        match candidates.as_slice() {
            [locked] => Ok(locked),
            [] => Err(anyhow!(
                "{package} is not in Cargo.lock as a dependency of {} {}: run `cargo fetch` (or `cargo update`) to \
                 bring Cargo.lock up to date",
                dependent.name,
                dependent.version
            )),
            _ => Err(anyhow!(
                "{package} is in Cargo.lock more than once ({}), and which one {} {} uses is ambiguous",
                candidates
                    .iter()
                    .map(|p| format!("{} {}", p.version, p.source.as_deref().unwrap_or("path")))
                    .collect::<Vec<_>>()
                    .join(", "),
                dependent.name,
                dependent.version
            )),
        }
    }

    /// Entry of `dependent` itself in Cargo.lock, `None` if it isn't there (the walk started outside of its
    /// workspace).
    fn locked_dependent<'l>(
        &mut self,
        lock: &'l CargoLock,
        dependent: &Dependent,
    ) -> Result<Option<&'l LockedPackage>> {
        let same: Vec<&LockedPackage> = lock
            .package
            .iter()
            .filter(|p| p.name == dependent.name && p.version_is(dependent.version))
            .collect();
        if same.len() <= 1 {
            return Ok(same.first().copied());
        }
        // the same name and version from more than one source: the one at the dependent's directory
        for locked in &same {
            if locked.source.is_some()
                && self
                    .package_dir(locked)
                    .ok()
                    .flatten()
                    .is_some_and(|dir| dir.canonicalize().is_ok_and(|dir| dir == dependent.dir))
            {
                return Ok(Some(locked));
            }
        }
        Ok(same.iter().find(|p| p.source.is_none()).copied())
    }

    /// Whether the walk started in a crate Cargo fetched (a dependency's `#[ww_trait]`, or `docs.rs`): the
    /// `Cargo.lock` next to it is its publisher's, and Cargo doesn't tell a crate which lock it is built with (rustc
    /// runs in the crate's own directory). Its registry dependencies are then the newest matching version Cargo has,
    /// which is what a fresh resolve picks; git ones still follow the lock next to it.
    fn entry_is_fetched(&mut self) -> Result<bool> {
        let Some(entry_dir) = self.entry_dir.clone() else {
            return Ok(false);
        };
        if !self.is_fetched(&entry_dir)? {
            return Ok(false);
        }
        // the lock of the build at hand pins them as well
        self.lock()?;
        Ok(!self.lock_is_build)
    }

    /// Newest `registry/src/<index>/<package>-<version>/` that matches `version_req`.
    fn registry_newest(
        &self,
        package: &str,
        version_req: Option<&semver::VersionReq>,
    ) -> Result<PathBuf> {
        let src = self.cargo_home()?.join("registry").join("src");
        let prefix = format!("{package}-");
        // a version fetched later is listed in an index directory
        self.note(&src);
        let newest = fs::read_dir(&src)
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|index| {
                self.note(&index.path());
                fs::read_dir(index.path()).into_iter().flatten().flatten()
            })
            .filter_map(|dir| {
                let name = dir.file_name();
                let version = semver::Version::parse(name.to_str()?.strip_prefix(&prefix)?).ok()?;
                let matches = version_req.is_none_or(|req| req.matches(&version));
                (matches && dir.path().join("Cargo.toml").exists()).then(|| (version, dir.path()))
            })
            .max_by(|a, b| a.0.cmp(&b.0));
        newest.map(|(_, dir)| dir).ok_or_else(|| {
            anyhow!(
                "{package} {} not found in {}/*/: run `cargo fetch` first",
                version_req.map_or("*".to_string(), |r| r.to_string()),
                src.display()
            )
        })
    }

    fn lock(&mut self) -> Result<Rc<CargoLock>> {
        if let Some(lock) = &self.lock {
            return Ok(lock.clone());
        }
        let entry_dir = self
            .entry_dir
            .clone()
            .ok_or_else(|| anyhow!("internal: crate walker entry crate not set"))?;
        let nearest = self.nearest_lock(&entry_dir);
        let entry = entry_dir
            .canonicalize()
            .unwrap_or_else(|_| entry_dir.clone());
        let build = match &self.rustc {
            Some(build) => Some(build.clone()),
            None => RustcBuild::from_process(),
        }
        .filter(|build| build.compiles(&entry));
        let lock = match build {
            Some(build) => self.build_lock(&entry, nearest, &build)?,
            None => {
                let path = nearest.ok_or_else(|| {
                    anyhow!(
                        "no Cargo.lock in {} or above it, needed to find git and registry dependencies: run `cargo \
                         fetch` there first",
                        entry_dir.display()
                    )
                })?;
                self.read_lock(&path)?
            }
        };
        let lock = Rc::new(lock);
        self.lock = Some(lock.clone());
        Ok(lock)
    }

    /// The nearest `Cargo.lock` in `dir` or above it.
    fn nearest_lock(&self, dir: &Path) -> Option<PathBuf> {
        dir.ancestors()
            .map(|dir| dir.join("Cargo.lock"))
            .find(|path| {
                self.note_parent(path);
                path.exists()
            })
    }

    fn read_lock(&self, path: &Path) -> Result<CargoLock> {
        self.note(path);
        let mut contents = String::new();
        fs::File::open(path)
            .and_then(|f| f.take(CARGO_LOCK_BYTES_MAX).read_to_string(&mut contents))
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut lock: CargoLock = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        lock.path = path.to_path_buf();
        Ok(lock)
    }

    /// The `Cargo.lock` the build `build` of the crate at `entry` (canonical) runs with. The one next to a crate
    /// isn't it when the crate is a dependency of another workspace (a git checkout, or a path dependency of a
    /// workspace around it: trybuild's, a `[patch]`), and Cargo doesn't tell rustc which one it is. So locks around
    /// the build are tried, in this order: above where Cargo was asked to build, `PWD`, the build's directories,
    /// above rustc's working directory (the workspace root for a member, the package itself for a fetched one), and
    /// the nearest one above `entry`. The
    /// first that lists `entry` and pins every git source Cargo compiled for this build ([Self::check_lock]) is
    /// taken. None for a crate from a git checkout: the locks of Cargo's git checkouts (`cargo install --git`).
    /// None at all: the nearest lock, unless one was ruled out by a git commit Cargo didn't compile, which is an
    /// error naming each lock looked at.
    fn build_lock(
        &mut self,
        entry: &Path,
        nearest: Option<PathBuf>,
        build: &RustcBuild,
    ) -> Result<CargoLock> {
        let fetched = self.is_fetched(entry)?;
        let entry_name =
            package_name(entry).with_context(|| format!("the package in {}", entry.display()))?;
        let externs = build.extern_dirs();
        let compiled = build.compiled();
        let candidates: Vec<PathBuf> = build
            .cargo_dirs
            .iter()
            .chain(&build.pwd)
            .chain(&build.dep_dirs)
            .chain(&build.cwd)
            .filter_map(|dir| self.nearest_lock(dir))
            .chain(nearest.clone())
            .collect();
        let mut rejected: Vec<(PathBuf, String)> = vec![];
        let mut looked: Vec<PathBuf> = vec![];
        for path in candidates {
            let path = path.canonicalize().unwrap_or(path);
            if looked.contains(&path) {
                continue;
            }
            looked.push(path.clone());
            match self.verified_lock(&path, entry, &entry_name, &externs, &compiled) {
                Ok(Some((lock, _))) => {
                    self.lock_is_build = true;
                    return Ok(lock);
                }
                Ok(None) => {}
                Err(reason) => rejected.push((path, reason)),
            }
        }
        if fetched {
            let mut groups: Vec<(Vec<String>, CargoLock)> = vec![];
            for path in self.checkout_locks(&entry_name)? {
                if looked.contains(&path) {
                    continue;
                }
                looked.push(path.clone());
                match self.verified_lock(&path, entry, &entry_name, &externs, &compiled) {
                    Ok(Some((lock, tree))) => {
                        if !groups.iter().any(|(t, _)| t == &tree) {
                            groups.push((tree, lock));
                        }
                    }
                    Ok(None) => {}
                    Err(reason) => rejected.push((path, reason)),
                }
            }
            match groups.len() {
                0 => {}
                1 => {
                    self.lock_is_build = true;
                    return Ok(groups.remove(0).1);
                }
                _ => {
                    return Err(anyhow!(
                        "{entry_name} at {} is built with one of {}, which pin different dependencies for it, and \
                         what Cargo compiled doesn't tell which: run cargo from the workspace being built",
                        entry.display(),
                        groups
                            .iter()
                            .map(|(_, l)| l.path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
        }
        if rejected.is_empty()
            && let Some(nearest) = nearest
        {
            return self.read_lock(&nearest);
        }
        let mut msg = format!(
            "{entry_name} at {} is built as a dependency of another workspace, and no Cargo.lock found pins the \
             git sources Cargo compiled for it: the workspace's Cargo.lock decides which commit of a git \
             dependency is used, not the one of {entry_name}. Run cargo from the workspace being built. \
             Cargo.locks looked at:",
            entry.display()
        );
        for (path, reason) in &rejected {
            msg.push_str(&format!("\n  {}: {reason}", path.display()));
        }
        if rejected.is_empty() {
            msg.push_str(" none lists it");
        }
        Err(anyhow!(msg))
    }

    /// The lock at `path` when [Self::check_lock] takes it, with what it pins for the tree of `entry`; `None` when
    /// it doesn't list `entry`.
    fn verified_lock(
        &mut self,
        path: &Path,
        entry: &Path,
        entry_name: &str,
        externs: &[PathBuf],
        compiled: &HashMap<String, Vec<PathBuf>>,
    ) -> std::result::Result<Option<(CargoLock, Vec<String>)>, String> {
        let lock = self.read_lock(path).map_err(|e| format!("{e:#}"))?;
        match self.check_lock(&lock, entry, entry_name, externs, compiled)? {
            Some(tree) => Ok(Some((lock, tree))),
            None => Ok(None),
        }
    }

    /// Whether `lock` is the one `entry` (named `entry_name`) is built with: it lists the package (a path package
    /// by name, a fetched one at `entry`), and pins every git source Cargo compiled: each of `externs` in a git
    /// checkout (as a path package when the lock is that checkout's own), and for each git package in the tree of
    /// `entry` but `entry` of which `compiled` has copies from the same repository, one of those copies. Registry crates aren't compared: a lock may pin several versions of one,
    /// and a build compiles only those it needs. `Ok(None)` when `lock` doesn't list `entry`, `Ok` with what it pins
    /// for the tree of `entry` to tell candidates apart, `Err` with why not.
    fn check_lock(
        &mut self,
        lock: &CargoLock,
        entry: &Path,
        entry_name: &str,
        externs: &[PathBuf],
        compiled: &HashMap<String, Vec<PathBuf>>,
    ) -> std::result::Result<Option<Vec<String>>, String> {
        let roots: Vec<usize> = (0..lock.package.len())
            .filter(|&i| {
                let p = &lock.package[i];
                p.name == entry_name && (p.source.is_none() || self.fetched_at(p, entry))
            })
            .collect();
        if roots.is_empty() {
            return Ok(None);
        }
        let checkouts = self
            .cargo_home()
            .map_err(|e| format!("{e:#}"))?
            .join("git")
            .join("checkouts");
        let checkouts = checkouts.canonicalize().unwrap_or(checkouts);
        // a checkout's own lock names the other packages of its repository as path packages
        let lock_root = lock.path.parent().and_then(|root| root.canonicalize().ok());
        for dir in externs.iter().filter(|dir| dir.starts_with(&checkouts)) {
            let Ok(name) = package_name(dir) else {
                continue;
            };
            let in_lock_tree = lock_root.as_ref().is_some_and(|root| dir.starts_with(root));
            let pinned = lock
                .package
                .iter()
                .filter(|p| p.name == name)
                .any(|p| (p.source.is_none() && in_lock_tree) || self.fetched_at(p, dir));
            if !pinned {
                return Err(format!(
                    "pins no {name} at {}, the source Cargo compiled it from",
                    dir.display()
                ));
            }
        }
        let tree = lock.tree(&roots);
        // `entry` itself is where the roots are: what `compiled` has of it is from earlier builds, this one is
        // running
        for &i in tree.iter().filter(|i| !roots.contains(i)) {
            let p = &lock.package[i];
            let Ok(Some(Source::Git { url, .. })) = p.source() else {
                continue;
            };
            let canonical = canonical_git_url(url);
            let repo = git_repo_name(&canonical);
            let copies: Vec<&PathBuf> = compiled
                .get(&p.name.replace('-', "_"))
                .into_iter()
                .flatten()
                .filter(|dir| {
                    dir.strip_prefix(&checkouts).is_ok_and(|rel| {
                        rel.components()
                            .next()
                            .is_some_and(|c| is_checkout_of(&c.as_os_str().to_string_lossy(), repo))
                    })
                })
                .collect();
            if !copies.is_empty() && !copies.iter().any(|dir| self.fetched_at(p, dir)) {
                return Err(format!(
                    "pins {} {} from {}, Cargo compiled it from {}",
                    p.name,
                    p.version,
                    p.source.as_deref().unwrap_or_default(),
                    copies
                        .iter()
                        .map(|d| d.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        let mut pinned: Vec<String> = tree
            .iter()
            .filter(|i| !roots.contains(i))
            .map(|&i| {
                let p = &lock.package[i];
                format!(
                    "{} {} {}",
                    p.name,
                    p.version,
                    p.source.as_deref().unwrap_or_default()
                )
            })
            .collect();
        pinned.sort();
        Ok(Some(pinned))
    }

    /// Whether `locked`, a fetched package, is at `dir` (canonical).
    fn fetched_at(&mut self, locked: &LockedPackage, dir: &Path) -> bool {
        locked.source.is_some()
            && self
                .package_dir(locked)
                .ok()
                .flatten()
                .and_then(|at| at.canonicalize().ok())
                .is_some_and(|at| at == dir)
    }

    /// Whether `dir` is a source Cargo fetched: a registry crate or a git checkout.
    fn is_fetched(&self, dir: &Path) -> Result<bool> {
        let home = self.cargo_home()?;
        let home = home.canonicalize().unwrap_or(home);
        Ok(dir.starts_with(home.join("registry")) || dir.starts_with(home.join("git")))
    }

    /// The `Cargo.lock` at the root of each of Cargo's git checkouts that names `package`: a workspace built from
    /// a checkout (`cargo install --git`) uses it.
    fn checkout_locks(&self, package: &str) -> Result<Vec<PathBuf>> {
        let checkouts = self.cargo_home()?.join("git").join("checkouts");
        let needle = format!("name = \"{package}\"");
        let mut budget = CHECKOUT_LOCKS_MAX;
        let mut locks = vec![];
        for repo in list_dirs(&checkouts, &mut budget) {
            for rev in list_dirs(&repo, &mut budget) {
                let path = rev.join("Cargo.lock");
                if fs::read_to_string(&path).is_ok_and(|c| c.contains(&needle)) {
                    locks.push(path);
                }
            }
        }
        Ok(locks)
    }

    fn cargo_home(&self) -> Result<PathBuf> {
        match &self.cargo_home {
            Some(home) => Ok(home.clone()),
            None => default_cargo_home(),
        }
    }

    /// Directories holding `name` at `version` as the `Cargo.lock` of [Self::entry_dir] pins and places them:
    /// Cargo's registry or git checkout for a fetched dependency, the workspace's own tree and its `[patch]`es for a
    /// path dependency. Empty when the lock doesn't list that version; a listed source Cargo hasn't fetched is
    /// skipped. An error without a lock. What this reads is not told to [Self::reads].
    pub fn locate_version(
        &mut self,
        name: &str,
        version: &semver::Version,
    ) -> Result<Vec<PathBuf>> {
        self.locate_locked(name, Some(version))
    }

    /// Directories holding `name` at whichever version the `Cargo.lock` of [Self::entry_dir] pins, as
    /// [Self::locate_version] places them: a crate's `api_snapshots/` keep its earlier versions too.
    pub fn locate_crate(&mut self, name: &str) -> Result<Vec<PathBuf>> {
        self.locate_locked(name, None)
    }

    fn locate_locked(
        &mut self,
        name: &str,
        version: Option<&semver::Version>,
    ) -> Result<Vec<PathBuf>> {
        let lock = self.lock()?;
        let mut dirs = vec![];
        for locked in lock
            .package
            .iter()
            .filter(|p| p.name == name && version.is_none_or(|v| p.version_is(v)))
        {
            match self.package_dir(locked) {
                Ok(Some(dir)) => dirs.push(dir),
                Ok(None) => {
                    if let Some(dir) = self.patched_dir(&lock.path, name)? {
                        dirs.push(dir);
                    }
                    let locked_version = semver::Version::parse(&locked.version).ok();
                    let root = lock.path.parent().unwrap_or(Path::new("."));
                    let mut budget = CARGO_TREE_DIRS_MAX;
                    dirs.extend(
                        packages_under(root, &mut budget)?
                            .into_iter()
                            .filter(|p| p.name == name && p.version == locked_version)
                            .map(|p| p.dir),
                    );
                }
                Err(e) => tracing::debug!("{name} {}: {e:#}", locked.version),
            }
        }
        dirs.dedup();
        Ok(dirs)
    }

    /// The `Cargo.lock` above [Self::entry_dir].
    pub fn lock_path(&mut self) -> Result<PathBuf> {
        Ok(self.lock()?.path.clone())
    }

    /// The directory of every package the `Cargo.lock` of [Self::entry_dir] lists and Cargo has on this machine:
    /// registry crates and git checkouts it fetched, the workspace's own tree and its `[patch]`es; one Cargo hasn't
    /// fetched is left out and named in the log. For a build script that embeds the snapshots of every crate a
    /// workspace uses ([crate::build]).
    pub fn locked_package_dirs(&mut self) -> Result<Vec<(String, semver::Version, PathBuf)>> {
        let lock = self.lock()?;
        let root = lock.path.parent().unwrap_or(Path::new("."));
        let mut tree: Option<Vec<Package>> = None;
        let mut dirs = vec![];
        for locked in &lock.package {
            let Ok(version) = semver::Version::parse(&locked.version) else {
                continue;
            };
            let dir = match self.package_dir(locked) {
                Ok(Some(dir)) => Some(dir),
                Ok(None) => match self.patched_dir(&lock.path, &locked.name)? {
                    Some(dir) => Some(dir),
                    None => tree
                        .get_or_insert_with(|| {
                            let mut budget = CARGO_TREE_DIRS_MAX;
                            packages_under(root, &mut budget).unwrap_or_default()
                        })
                        .iter()
                        .find(|p| p.name == locked.name && p.version.as_ref() == Some(&version))
                        .map(|p| p.dir.clone()),
                },
                Err(e) => {
                    tracing::debug!("{} {}: {e:#}", locked.name, locked.version);
                    None
                }
            };
            if let Some(dir) = dir {
                dirs.push((locked.name.clone(), version, dir));
            }
        }
        Ok(dirs)
    }

    /// The directory of the one package named `name` that [Self::locked_package_dirs] finds: for a git dependency,
    /// Cargo's checkout of the commit `Cargo.lock` pins. An error when there is none or more than one.
    pub fn locked_package_dir(&mut self, name: &str) -> Result<PathBuf> {
        let lock = self.lock_path()?;
        let mut found: Vec<PathBuf> = self
            .locked_package_dirs()?
            .into_iter()
            .filter(|(n, _, _)| n == name)
            .map(|(_, _, dir)| dir)
            .collect();
        found.dedup();
        match found.len() {
            1 => Ok(found.remove(0)),
            0 => Err(anyhow!(
                "{name} is not in {}, or Cargo has not fetched it yet (cargo fetch)",
                lock.display()
            )),
            n => Err(anyhow!(
                "{n} packages named {name} in {}: {found:?}",
                lock.display()
            )),
        }
    }

    /// Where Cargo put the source of `locked`, `None` for a path dependency.
    fn package_dir(&mut self, locked: &LockedPackage) -> Result<Option<PathBuf>> {
        match locked.source()? {
            None => Ok(None),
            Some(Source::Registry) => self.registry_dir(locked).map(Some),
            Some(Source::Git { url, rev }) => {
                let checkout = self.git_checkout(url, rev)?;
                let members = self.git_members(&checkout)?;
                members.get(&locked.name).cloned().map(Some).ok_or_else(|| {
                    anyhow!(
                        "no package {} in {url} at {rev} ({})",
                        locked.name,
                        checkout.display()
                    )
                })
            }
        }
    }

    /// `registry/src/<index>/<name>-<version>/`
    fn registry_dir(&self, locked: &LockedPackage) -> Result<PathBuf> {
        let src = self.cargo_home()?.join("registry").join("src");
        let dir_name = format!("{}-{}", locked.name, locked.version);
        self.note(&src);
        fs::read_dir(&src)
            .into_iter()
            .flatten()
            .flatten()
            .map(|index| index.path().join(&dir_name))
            .find(|dir| {
                self.note_parent(dir);
                dir.join("Cargo.toml").exists()
            })
            .ok_or_else(|| {
                anyhow!(
                    "{} {} not found in {}/*/: run `cargo fetch` first",
                    locked.name,
                    locked.version,
                    src.display()
                )
            })
    }

    /// `git/checkouts/<repo>-<hash>/<short rev>/` ([git_checkout_dir_name]), or its `-shallow` twin: the one checkout
    /// Cargo builds `url` at `rev` from. A repository fetched under two URLs has two checkouts, and Cargo compiles
    /// from the one of the URL the lock names.
    fn git_checkout(&self, url: &str, rev: &str) -> Result<PathBuf> {
        let checkouts = self.cargo_home()?.join("git").join("checkouts");
        let name = git_checkout_dir_name(url);
        let repo_dirs = [
            checkouts.join(&name),
            checkouts.join(format!("{name}-shallow")),
        ];
        // a checkout fetched later is a new directory in these; a checkout itself never changes
        self.note(&checkouts);
        for repo_dir in repo_dirs {
            self.note(&repo_dir);
            let mut revs: Vec<PathBuf> = fs::read_dir(&repo_dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| {
                    let name = e.file_name();
                    let name = name.to_string_lossy();
                    name.len() >= 7 && rev.starts_with(name.as_ref())
                })
                .map(|e| e.path())
                .collect();
            revs.sort();
            if let Some(checkout) = revs.into_iter().next() {
                return Ok(checkout);
            }
        }
        Err(anyhow!(
            "no checkout of {url} at {rev} in {}/{name}/: run `cargo fetch` first",
            checkouts.display()
        ))
    }

    /// Packages in a git checkout by name: Cargo finds a git dependency by its package name anywhere in the
    /// repository, not only among the members of its root workspace.
    fn git_members(&mut self, checkout: &Path) -> Result<Rc<HashMap<String, PathBuf>>> {
        if let Some(members) = self.git_members.get(checkout) {
            return Ok(members.clone());
        }
        let mut budget = CARGO_TREE_DIRS_MAX;
        let mut members = HashMap::new();
        for package in packages_under(checkout, &mut budget)? {
            members.entry(package.name).or_insert(package.dir);
        }
        let members = Rc::new(members);
        self.git_members
            .insert(checkout.to_path_buf(), members.clone());
        Ok(members)
    }
}

impl CargoSources {
    /// Directory of `package` as a `[patch.<source>]` of the workspace whose `Cargo.lock` is `lock_path` puts it: a
    /// git or registry dependency that Cargo resolves to a path there (a checkout of exact_units or shrink_wrap
    /// developed beside this one). Cargo's configuration first, as Cargo reads it ([crate::cargo_config]) from
    /// where the build's Cargo runs (the workspace root when that isn't known), then the manifest's `[patch]`
    /// tables: a config's patch wins over the manifest's.
    fn patched_dir(&self, lock_path: &Path, package: &str) -> Result<Option<PathBuf>> {
        let root = lock_path.parent().unwrap_or(Path::new("."));
        let build = match &self.rustc {
            Some(build) => Some(build.clone()),
            None => RustcBuild::from_process(),
        };
        // Cargo reads its configuration from where it runs; a lock that isn't that build's has its own
        let cargo_cwd = build
            .as_ref()
            .and_then(|b| b.cargo_cwd.clone())
            .filter(|cwd| self.lock_is_build || cwd.starts_with(root) || root.starts_with(cwd));
        let no_configs = vec![];
        let cli = match (&cargo_cwd, &build) {
            (Some(_), Some(build)) => &build.cargo_configs,
            _ => &no_configs,
        };
        let cargo_home = self.cargo_home().ok();
        let sources = cargo_config::ConfigSources {
            cli,
            cwd: cargo_cwd.as_deref().unwrap_or(root),
            cargo_home: cargo_home.as_deref(),
        };
        if let Some(dir) = cargo_config::patch_path(&sources, package, &|path| self.note(path))? {
            return Ok(Some(dir));
        }
        let manifest = root.join("Cargo.toml");
        self.note(&manifest);
        let Ok(contents) = fs::read_to_string(&manifest) else {
            return Ok(None);
        };
        let table: toml::Table = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse {}", manifest.display()))?;
        Ok(cargo_config::patch_in(&table, package).map(|dir| root.join(dir)))
    }
}

/// `CARGO_HOME`: the environment's, else `~/.cargo`.
pub fn default_cargo_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("CARGO_HOME") {
        return Ok(PathBuf::from(home));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".cargo"))
        .ok_or_else(|| anyhow!("neither CARGO_HOME nor HOME is set, can't find Cargo's sources"))
}

/// Every copy of `name` at `version` Cargo fetched under `cargo_home`, without a lock to say which one is in use:
/// `registry/src/<index>/<name>-<version>/`, and that package in every `git/checkouts/<repo>/<rev>/`. Looks into
/// at most [CARGO_DIRS_SCANNED_MAX] directories in all, [CARGO_TREE_DIRS_MAX] per checkout. [Fetched] scans once
/// for many lookups.
pub fn scan_fetched(cargo_home: &Path, name: &str, version: &semver::Version) -> Vec<PathBuf> {
    Fetched::scan(cargo_home).find(name, version)
}

/// What Cargo fetched under a `CARGO_HOME`, scanned once: the registry indexes and every package of every git
/// checkout, with the same bounds as [scan_fetched]. Its lookups read only the registry directory they name.
#[derive(Debug, Default)]
pub struct Fetched {
    registry_indexes: Vec<PathBuf>,
    checkout_packages: HashMap<(String, semver::Version), Vec<PathBuf>>,
}

impl Fetched {
    /// Lists `cargo_home`'s registry indexes and reads the manifests of its git checkouts.
    pub fn scan(cargo_home: &Path) -> Self {
        let mut budget = CARGO_DIRS_SCANNED_MAX;
        let registry_indexes = list_dirs(&cargo_home.join("registry").join("src"), &mut budget);
        let mut checkout_packages: HashMap<(String, semver::Version), Vec<PathBuf>> =
            HashMap::new();
        for repo in list_dirs(&cargo_home.join("git").join("checkouts"), &mut budget) {
            for checkout in list_dirs(&repo, &mut budget) {
                let mut tree = CARGO_TREE_DIRS_MAX.min(budget);
                let before = tree;
                let packages = packages_under(&checkout, &mut tree).unwrap_or_default();
                budget -= before - tree;
                for package in packages {
                    if let Some(version) = package.version {
                        checkout_packages
                            .entry((package.name, version))
                            .or_default()
                            .push(package.dir);
                    }
                }
            }
        }
        Fetched {
            registry_indexes,
            checkout_packages,
        }
    }

    /// The copies of `name` at `version`: registry ones first, then checkouts, each in sorted directory order.
    pub fn find(&self, name: &str, version: &semver::Version) -> Vec<PathBuf> {
        let dir_name = format!("{name}-{version}");
        let mut dirs: Vec<PathBuf> = self
            .registry_indexes
            .iter()
            .map(|index| index.join(&dir_name))
            .filter(|dir| dir.join("Cargo.toml").is_file())
            .collect();
        if let Some(found) = self
            .checkout_packages
            .get(&(name.to_owned(), version.clone()))
        {
            dirs.extend(found.iter().cloned());
        }
        dirs
    }
}

/// The directories in `dir`, sorted; nothing once `budget` is spent.
fn list_dirs(dir: &Path, budget: &mut usize) -> Vec<PathBuf> {
    if *budget == 0 {
        return vec![];
    }
    *budget -= 1;
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs
}

/// A `[package]` found under a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    /// `None` when the manifest has no plain version and no workspace above it in the same tree gives one.
    pub version: Option<semver::Version>,
    /// The directory with its `Cargo.toml`.
    pub dir: PathBuf,
}

#[derive(Deserialize)]
struct ManifestHead {
    package: Option<PackageHead>,
    workspace: Option<WorkspaceHead>,
}

#[derive(Deserialize)]
struct PackageHead {
    name: String,
    version: Option<toml::Value>,
}

#[derive(Deserialize)]
struct WorkspaceHead {
    package: Option<WorkspacePackageHead>,
}

#[derive(Deserialize)]
struct WorkspacePackageHead {
    version: Option<String>,
}

impl PackageHead {
    fn version(&self, workspace: Option<&semver::Version>) -> Option<semver::Version> {
        match &self.version {
            Some(toml::Value::String(v)) => semver::Version::parse(v).ok(),
            Some(toml::Value::Table(t))
                if t.get("workspace").and_then(|w| w.as_bool()) == Some(true) =>
            {
                workspace.cloned()
            }
            _ => None,
        }
    }
}

/// Every `[package]` under `dir`, nearest first, looking into at most `budget` directories (what is left is left in
/// `budget`); `.`-directories and `target` are skipped, a directory that can't be read is skipped unless it is `dir`.
/// A `version.workspace = true` takes the version of the nearest `[workspace.package]` above it in the tree.
pub fn packages_under(dir: &Path, budget: &mut usize) -> Result<Vec<Package>> {
    if !dir.is_dir() {
        return Err(anyhow!("{} is not a directory", dir.display()));
    }
    let mut packages = vec![];
    let mut queue: Vec<(PathBuf, Option<semver::Version>)> = vec![(dir.to_path_buf(), None)];
    while !queue.is_empty() {
        let mut next = vec![];
        for (dir, mut workspace_version) in queue {
            if *budget == 0 {
                return Ok(packages);
            }
            *budget -= 1;
            let head: Option<ManifestHead> = fs::read_to_string(dir.join("Cargo.toml"))
                .ok()
                .and_then(|contents| toml::from_str(&contents).ok());
            if let Some(head) = head {
                if let Some(workspace) = &head.workspace {
                    workspace_version = workspace
                        .package
                        .as_ref()
                        .and_then(|p| p.version.as_deref())
                        .and_then(|v| semver::Version::parse(v).ok());
                }
                if let Some(package) = head.package {
                    packages.push(Package {
                        version: package.version(workspace_version.as_ref()),
                        name: package.name,
                        dir: dir.clone(),
                    });
                }
            }
            let mut children: Vec<PathBuf> = fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|e| {
                    let name = e.file_name();
                    let name = name.to_string_lossy();
                    !name.starts_with('.') && name != "target"
                })
                .map(|e| e.path())
                .collect();
            children.sort();
            next.extend(children.into_iter().map(|c| (c, workspace_version.clone())));
        }
        queue = next;
    }
    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dependency_entry_names_a_git_source_without_its_revision() {
        let lock: CargoLock = toml::from_str(
            r#"
            [[package]]
            name = "units"
            version = "0.5.0"

            [[package]]
            name = "units"
            version = "0.5.0"
            source = "git+https://example.org/base#0123abcd"

            [[package]]
            name = "api"
            version = "0.1.0"
            dependencies = ["units 0.5.0 (git+https://example.org/base)"]
            "#,
        )
        .unwrap();
        let version = semver::Version::new(0, 1, 0);
        let dependent = Dependent {
            name: "api",
            version: &version,
            dir: Path::new("/nowhere/api"),
        };
        let spec = DepSpec::Git {
            url: "https://example.org/base",
        };
        let locked = CargoSources::default()
            .resolve(&lock, &dependent, "units", &spec)
            .unwrap();
        assert_eq!(
            locked.source.as_deref(),
            Some("git+https://example.org/base#0123abcd")
        );
    }

    /// Names of directories Cargo 1.100 made for these URLs.
    #[test]
    fn git_checkout_dir_name_is_the_one_cargo_makes() {
        for (url, dir) in [
            (
                "ssh://git@github.com/romixlab/pd_bench",
                "pd_bench-d39ac3f38ae5ecd4",
            ),
            (
                "ssh://git@github.com/romixlab/pd_bench.git",
                "pd_bench-d39ac3f38ae5ecd4",
            ),
            (
                "https://github.com/romixlab/pd_bench",
                "pd_bench-1cc6b6947041e225",
            ),
            (
                "https://github.com/romixlab/exact_units",
                "exact_units-0e04409aaab251a0",
            ),
            (
                "https://github.com/RomixLab/Exact_Units.git/?branch=main#0123abcd",
                "exact_units-0e04409aaab251a0",
            ),
            (
                "https://github.com/embassy-rs/embassy",
                "embassy-c08a80187403f815",
            ),
            (
                "https://github.com/romixlab/embassy",
                "embassy-977f720069a36616",
            ),
        ] {
            assert_eq!(git_checkout_dir_name(url), dir, "{url}");
        }
    }

    #[test]
    fn a_checkout_of_a_repository_is_told_from_one_of_a_longer_name() {
        assert!(is_checkout_of("base-d39ac3f38ae5ecd4", "base"));
        assert!(is_checkout_of("base-d39ac3f38ae5ecd4-shallow", "base"));
        assert!(!is_checkout_of("base-ext-d39ac3f38ae5ecd4", "base"));
        assert!(!is_checkout_of("base-1111", "base"));
    }

    #[test]
    fn locked_package_dir_finds_one_git_checkout() {
        let mut sources = CargoSources {
            entry_dir: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
            ..Default::default()
        };
        let exact_units = sources.locked_package_dir("exact_units").unwrap();
        assert!(exact_units.join("Cargo.toml").is_file(), "{exact_units:?}");
        let err = sources.locked_package_dir("no_such_package").unwrap_err();
        assert!(err.to_string().contains("not in"), "{err}");
    }

    #[test]
    fn patch_of_the_workspace_puts_a_git_dependency_at_a_path() {
        let root = std::env::temp_dir().join(format!("ww_patched_dir_ws_{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\n\n[patch.\"https://example.org/units\"]\nunits = { path = \"./units\" }\n",
        )
        .unwrap();
        let lock = root.join("Cargo.lock");
        let sources = CargoSources::default();
        let found = sources.patched_dir(&lock, "units").unwrap();
        let missing = sources.patched_dir(&lock, "no_such_package").unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, Some(root.join("./units")));
        assert_eq!(missing, None);
    }

    #[test]
    fn patch_of_the_cargo_config_puts_a_git_dependency_at_a_path() {
        let root = std::env::temp_dir().join(format!("ww_patched_dir_{}", std::process::id()));
        fs::create_dir_all(root.join(".cargo")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(
            root.join(".cargo/config.toml"),
            "[patch.\"https://example.org/units\"]\nunits = { path = \"../units\" }\n",
        )
        .unwrap();
        let lock = root.join("Cargo.lock");
        let sources = CargoSources::default();
        let found = sources.patched_dir(&lock, "units").unwrap();
        let missing = sources.patched_dir(&lock, "no_such_package").unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, Some(root.join("../units")));
        assert_eq!(missing, None);
    }

    #[test]
    fn patch_of_a_cargo_config_above_the_workspace_puts_a_git_dependency_at_a_path() {
        let top = std::env::temp_dir().join(format!("ww_patched_dir_above_{}", std::process::id()));
        let root = top.join("repo");
        fs::create_dir_all(top.join(".cargo")).unwrap();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(
            top.join(".cargo/config.toml"),
            "[patch.\"https://example.org/units\"]\nunits = { path = \"units\" }\n",
        )
        .unwrap();
        let lock = root.join("Cargo.lock");
        let sources = CargoSources::default();
        let found = sources.patched_dir(&lock, "units").unwrap();
        fs::remove_dir_all(&top).unwrap();
        assert_eq!(found, Some(top.join("units")));
    }

    fn ws_version(name: &str) -> semver::Version {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let manifest = fs::read_to_string(root.join(name).join("Cargo.toml")).unwrap();
        let head: ManifestHead = toml::from_str(&manifest).unwrap();
        let ws: ManifestHead =
            toml::from_str(&fs::read_to_string(root.join("Cargo.toml")).unwrap()).unwrap();
        let ws = ws.workspace.unwrap().package.unwrap().version.unwrap();
        head.package
            .unwrap()
            .version(Some(&semver::Version::parse(&ws).unwrap()))
            .unwrap()
    }

    #[test]
    fn packages_under_the_workspace_know_inherited_versions() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut budget = CARGO_TREE_DIRS_MAX;
        let packages = packages_under(&root, &mut budget).unwrap();
        assert!(
            budget > 0,
            "the workspace tree is larger than CARGO_TREE_DIRS_MAX"
        );
        let own = packages.iter().find(|p| p.name == "ww_snapshots").unwrap();
        assert_eq!(own.version, Some(ws_version("ww_snapshots")));
        assert_eq!(own.dir, root.join("ww_snapshots"));
        let plain = packages.iter().find(|p| p.name == "ww_self").unwrap();
        assert_eq!(plain.version, Some(ws_version("ww_self")));
        let mut spent = 1;
        assert_eq!(
            packages_under(&root, &mut spent).unwrap().len(),
            packages.iter().filter(|p| p.dir == root).count()
        );
    }

    #[test]
    fn locate_version_through_the_lock_finds_path_and_git_crates() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut sources = CargoSources {
            entry_dir: Some(root.join("ww_snapshots")),
            ..Default::default()
        };
        let own = ws_version("ww_snapshots");
        assert_eq!(
            sources.locate_version("ww_snapshots", &own).unwrap(),
            vec![root.join("ww_snapshots")]
        );
        assert!(
            sources
                .locate_version("ww_snapshots", &semver::Version::new(99, 0, 0))
                .unwrap()
                .is_empty()
        );
        // any locked version: the same directory, where the crate's earlier snapshots are kept
        assert_eq!(
            sources.locate_crate("ww_snapshots").unwrap(),
            vec![root.join("ww_snapshots")]
        );
        assert!(sources.locate_crate("no_such_crate").unwrap().is_empty());
        // exact_units comes by git: its checkout under CARGO_HOME, fetched for this workspace to build
        let lock: CargoLock =
            toml::from_str(&fs::read_to_string(root.join("Cargo.lock")).unwrap()).unwrap();
        let units = lock
            .package
            .iter()
            .find(|p| p.name == "exact_units")
            .unwrap();
        let dirs = sources
            .locate_version(
                "exact_units",
                &semver::Version::parse(&units.version).unwrap(),
            )
            .unwrap();
        assert_eq!(dirs.len(), 1, "{dirs:?}");
        assert!(dirs[0].join("Cargo.toml").is_file());
        // its checkout under CARGO_HOME, or the path a `[patch]` of this workspace puts it at while developing both
        let patched = sources
            .patched_dir(&root.join("Cargo.lock"), "exact_units")
            .unwrap();
        assert!(
            dirs[0].starts_with(default_cargo_home().unwrap().join("git").join("checkouts"))
                || Some(&dirs[0]) == patched.as_ref(),
            "{dirs:?}"
        );
    }

    #[test]
    fn scan_fetched_finds_registry_and_checkout_copies() {
        let home = std::env::temp_dir().join(format!("ww_scan_fetched_{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let write = |path: PathBuf, contents: &str| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        };
        let registry = home.join("registry/src/index.crates.io-abc/dep_api-0.2.0");
        write(
            registry.join("Cargo.toml"),
            "[package]\nname = \"dep_api\"\nversion = \"0.2.0\"\n",
        );
        write(
            home.join("registry/src/index.crates.io-abc/dep_api-0.3.0/Cargo.toml"),
            "[package]\nname = \"dep_api\"\nversion = \"0.3.0\"\n",
        );
        let checkout = home.join("git/checkouts/deps-1234/abcdef0");
        write(
            checkout.join("Cargo.toml"),
            "[workspace]\nmembers = [\"dep_api\"]\n[workspace.package]\nversion = \"0.2.0\"\n",
        );
        write(
            checkout.join("dep_api/Cargo.toml"),
            "[package]\nname = \"dep_api\"\nversion.workspace = true\n",
        );
        write(
            checkout.join("other/Cargo.toml"),
            "[package]\nname = \"other\"\nversion = \"0.2.0\"\n",
        );
        let found = scan_fetched(&home, "dep_api", &semver::Version::new(0, 2, 0));
        let none = scan_fetched(&home, "dep_api", &semver::Version::new(0, 9, 0));
        fs::remove_dir_all(&home).unwrap();
        assert_eq!(found, vec![registry, checkout.join("dep_api")]);
        assert!(none.is_empty());
    }
}
