//! Where Cargo keeps the source of a git or registry dependency: the exact package is pinned by the `Cargo.lock` of
//! the crate the walk started from, its source lies under `CARGO_HOME` once Cargo fetched it
//! (`git/checkouts/<repo>-<hash>/<short rev>/`, `registry/src/<index>/<name>-<version>/`). The same places answer
//! where a crate of a given name and version is on this machine ([CargoSources::locate_version], [scan_fetched]),
//! which is where its `api_snapshots/` are.

use crate::limits::{CARGO_DIRS_SCANNED_MAX, CARGO_TREE_DIRS_MAX};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
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
    /// Crate the walk started from: the nearest `Cargo.lock` above it pins every git and registry dependency, also
    /// those of dependencies (a git checkout's own `Cargo.lock` isn't the one in use).
    pub entry_dir: Option<PathBuf>,
    /// `CARGO_HOME` to use instead of the environment's (tests).
    pub cargo_home: Option<PathBuf>,
    /// Called with every path read or looked into, `None` when nobody cares.
    pub reads: Option<ReadNote>,
    lock: Option<Rc<CargoLock>>,
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
            Some(locked) => locked
                .dependencies
                .iter()
                .filter_map(|entry| {
                    let mut parts = entry.split_whitespace();
                    (parts.next() == Some(package)).then(|| (parts.next(), parts.next()))
                })
                .flat_map(|(version, source)| {
                    let source = source.map(|s| s.trim_start_matches('(').trim_end_matches(')'));
                    lock.package.iter().filter(move |p| {
                        p.name == package
                            && version.is_none_or(|v| p.version == v)
                            && source.is_none_or(|s| p.source.as_deref() == Some(s))
                    })
                })
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
    fn entry_is_fetched(&self) -> Result<bool> {
        let Some(entry_dir) = &self.entry_dir else {
            return Ok(false);
        };
        let home = self.cargo_home()?;
        let home = home.canonicalize().unwrap_or(home);
        Ok(entry_dir.starts_with(home.join("registry")) || entry_dir.starts_with(home.join("git")))
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
            .as_deref()
            .ok_or_else(|| anyhow!("internal: crate walker entry crate not set"))?;
        let path = entry_dir
            .ancestors()
            .map(|dir| dir.join("Cargo.lock"))
            .find(|path| {
                self.note_parent(path);
                path.exists()
            })
            .ok_or_else(|| {
                anyhow!(
                    "no Cargo.lock in {} or above it, needed to find git and registry dependencies: run `cargo \
                     fetch` there first",
                    entry_dir.display()
                )
            })?;
        self.note(&path);
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut lock: CargoLock = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        lock.path = path;
        let lock = Rc::new(lock);
        self.lock = Some(lock.clone());
        Ok(lock)
    }

    fn cargo_home(&self) -> Result<PathBuf> {
        match &self.cargo_home {
            Some(home) => Ok(home.clone()),
            None => default_cargo_home(),
        }
    }

    /// Directories holding `name` at `version` as the `Cargo.lock` above [Self::entry_dir] pins and places them:
    /// Cargo's registry or git checkout for a fetched dependency, the workspace's own tree and its `[patch]`es for a
    /// path dependency. Empty when the lock doesn't list that version; a listed source Cargo hasn't fetched is
    /// skipped. An error without a lock. What this reads is not told to [Self::reads].
    pub fn locate_version(
        &mut self,
        name: &str,
        version: &semver::Version,
    ) -> Result<Vec<PathBuf>> {
        let lock = self.lock()?;
        let mut dirs = vec![];
        for locked in lock
            .package
            .iter()
            .filter(|p| p.name == name && p.version_is(version))
        {
            match self.package_dir(locked) {
                Ok(Some(dir)) => dirs.push(dir),
                Ok(None) => {
                    if let Some(dir) = self.patched_dir(&lock.path, name)? {
                        dirs.push(dir);
                    }
                    let root = lock.path.parent().unwrap_or(Path::new("."));
                    let mut budget = CARGO_TREE_DIRS_MAX;
                    dirs.extend(
                        packages_under(root, &mut budget)?
                            .into_iter()
                            .filter(|p| p.name == name && p.version.as_ref() == Some(version))
                            .map(|p| p.dir),
                    );
                }
                Err(e) => tracing::debug!("{name} {version}: {e:#}"),
            }
        }
        dirs.dedup();
        Ok(dirs)
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

    /// `git/checkouts/<repo>-<hash>/<short rev>/`: `<hash>` is Cargo's hash of the URL, so every checkout of a
    /// repository with that name is tried; one at `rev` has the same files whichever URL it came from.
    fn git_checkout(&self, url: &str, rev: &str) -> Result<PathBuf> {
        let checkouts = self.cargo_home()?.join("git").join("checkouts");
        let repo = url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".git");
        let prefix = format!("{repo}-");
        // a checkout fetched later is a new directory in these; a checkout itself never changes
        self.note(&checkouts);
        let mut repo_dirs: Vec<PathBuf> = fs::read_dir(&checkouts)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .map(|e| e.path())
            .collect();
        repo_dirs.sort();
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
            "no checkout of {url} at {rev} in {}/{repo}-*/: run `cargo fetch` first",
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
    /// git or registry dependency that Cargo resolves to a path there (wire_weaver patches the shrink_wrap that
    /// exact_units takes by git to its own). The manifest's `[patch]` tables, then `.cargo/config.toml`'s, whose
    /// paths are relative to the workspace root as well.
    fn patched_dir(&self, lock_path: &Path, package: &str) -> Result<Option<PathBuf>> {
        let root = lock_path.parent().unwrap_or(Path::new("."));
        for file in [
            root.join("Cargo.toml"),
            root.join(".cargo").join("config.toml"),
        ] {
            if let Some(dir) = self.patch_in(&file, package)? {
                return Ok(Some(root.join(dir)));
            }
        }
        Ok(None)
    }

    /// The path the `[patch]` tables of the TOML file `path` give `package`, if any.
    fn patch_in(&self, path: &Path, package: &str) -> Result<Option<String>> {
        self.note(path);
        let Ok(contents) = fs::read_to_string(path) else {
            return Ok(None);
        };
        let file: toml::Table = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        let Some(patch) = file.get("patch").and_then(|p| p.as_table()) else {
            return Ok(None);
        };
        for deps in patch.values().filter_map(|deps| deps.as_table()) {
            for (name, dep) in deps {
                let Some(dep) = dep.as_table() else { continue };
                let renamed = dep.get("package").and_then(|p| p.as_str());
                if renamed.unwrap_or(name) != package {
                    continue;
                }
                if let Some(dir) = dep.get("path").and_then(|p| p.as_str()) {
                    return Ok(Some(dir.to_owned()));
                }
            }
        }
        Ok(None)
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
/// at most [CARGO_DIRS_SCANNED_MAX] directories in all, [CARGO_TREE_DIRS_MAX] per checkout.
pub fn scan_fetched(cargo_home: &Path, name: &str, version: &semver::Version) -> Vec<PathBuf> {
    let mut dirs = vec![];
    let mut budget = CARGO_DIRS_SCANNED_MAX;
    let dir_name = format!("{name}-{version}");
    for index in list_dirs(&cargo_home.join("registry").join("src"), &mut budget) {
        let dir = index.join(&dir_name);
        if dir.join("Cargo.toml").is_file() {
            dirs.push(dir);
        }
    }
    for repo in list_dirs(&cargo_home.join("git").join("checkouts"), &mut budget) {
        for checkout in list_dirs(&repo, &mut budget) {
            let mut tree = CARGO_TREE_DIRS_MAX.min(budget);
            let before = tree;
            let packages = packages_under(&checkout, &mut tree).unwrap_or_default();
            budget -= before - tree;
            dirs.extend(
                packages
                    .into_iter()
                    .filter(|p| p.name == name && p.version.as_ref() == Some(version))
                    .map(|p| p.dir),
            );
        }
    }
    dirs
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
    fn patch_of_the_workspace_puts_a_git_dependency_at_a_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let lock = root.join("Cargo.lock");
        let sources = CargoSources::default();
        assert_eq!(
            sources.patched_dir(&lock, "shrink_wrap").unwrap(),
            Some(root.join("./shrink_wrap/shrink_wrap"))
        );
        assert_eq!(sources.patched_dir(&lock, "no_such_package").unwrap(), None);
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
