//! Where Cargo keeps the source of a git or registry dependency: the exact package is pinned by the `Cargo.lock` of
//! the crate the walk started from, its source lies under `CARGO_HOME` once Cargo fetched it
//! (`git/checkouts/<repo>-<hash>/<short rev>/`, `registry/src/<index>/<name>-<version>/`).

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// How a manifest asks for a dependency that isn't a path.
pub(crate) enum DepSpec<'a> {
    /// `name = "1.2"` or `name = { version = "1.2" }`, from crates.io or another registry.
    Registry {
        version_req: Option<&'a semver::VersionReq>,
    },
    /// `name = { git = "https://.." }`, with or without `branch`, `tag` or `rev`.
    Git { url: &'a str },
}

/// The package a dependency is looked up for.
pub(crate) struct Dependent<'a> {
    pub(crate) name: &'a str,
    pub(crate) version: &'a semver::Version,
    /// Directory with its `Cargo.toml`, canonical.
    pub(crate) dir: &'a Path,
}

/// Cargo.lock and the fetched sources it points at, loaded once per walk.
#[derive(Default)]
pub(crate) struct CargoSources {
    /// Crate the walk started from: the nearest `Cargo.lock` above it pins every git and registry dependency, also
    /// those of dependencies (a git checkout's own `Cargo.lock` isn't the one in use).
    pub(crate) entry_dir: Option<PathBuf>,
    /// `CARGO_HOME` to use instead of the environment's (tests).
    pub(crate) cargo_home: Option<PathBuf>,
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
    /// Directory of package `package` that `dependent` asks for with `spec`.
    pub(crate) fn locate(
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
        patched_dir(&lock.path, package)?.ok_or_else(|| {
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
        let newest = fs::read_dir(&src)
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|index| fs::read_dir(index.path()).into_iter().flatten().flatten())
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
            .find(|path| path.exists())
            .ok_or_else(|| {
                anyhow!(
                    "no Cargo.lock in {} or above it, needed to find git and registry dependencies: run `cargo \
                     fetch` there first",
                    entry_dir.display()
                )
            })?;
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
        if let Some(home) = &self.cargo_home {
            return Ok(home.clone());
        }
        if let Some(home) = std::env::var_os("CARGO_HOME") {
            return Ok(PathBuf::from(home));
        }
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(".cargo"))
            .ok_or_else(|| {
                anyhow!("neither CARGO_HOME nor HOME is set, can't find Cargo's sources")
            })
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
        fs::read_dir(&src)
            .into_iter()
            .flatten()
            .flatten()
            .map(|index| index.path().join(&dir_name))
            .find(|dir| dir.join("Cargo.toml").exists())
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
        let mut repo_dirs: Vec<PathBuf> = fs::read_dir(&checkouts)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .map(|e| e.path())
            .collect();
        repo_dirs.sort();
        for repo_dir in repo_dirs {
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
        let mut members = HashMap::new();
        collect_packages(checkout, &mut members)?;
        let members = Rc::new(members);
        self.git_members
            .insert(checkout.to_path_buf(), members.clone());
        Ok(members)
    }
}

/// Directory of `package` as a `[patch.<source>]` of the workspace whose `Cargo.lock` is `lock_path` puts it: a git
/// or registry dependency that Cargo resolves to a path there (wire_weaver patches the shrink_wrap that exact_units
/// takes by git to its own).
fn patched_dir(lock_path: &Path, package: &str) -> Result<Option<PathBuf>> {
    let root = lock_path.parent().unwrap_or(Path::new("."));
    let manifest_path = root.join("Cargo.toml");
    let Ok(contents) = fs::read_to_string(&manifest_path) else {
        return Ok(None);
    };
    let manifest: toml::Table = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
    let Some(patch) = manifest.get("patch").and_then(|p| p.as_table()) else {
        return Ok(None);
    };
    for deps in patch.values().filter_map(|deps| deps.as_table()) {
        for (name, dep) in deps {
            let Some(dep) = dep.as_table() else { continue };
            let renamed = dep.get("package").and_then(|p| p.as_str());
            if renamed.unwrap_or(name) != package {
                continue;
            }
            if let Some(path) = dep.get("path").and_then(|p| p.as_str()) {
                return Ok(Some(root.join(path)));
            }
        }
    }
    Ok(None)
}

#[derive(Deserialize)]
struct NameOnly {
    package: Option<PackageName>,
}

#[derive(Deserialize)]
struct PackageName {
    name: String,
}

/// Every `[package]` under `dir` by name, the one closest to `dir` when a name is there twice.
fn collect_packages(dir: &Path, packages: &mut HashMap<String, PathBuf>) -> Result<()> {
    let mut queue = vec![dir.to_path_buf()];
    while !queue.is_empty() {
        let mut next = vec![];
        for dir in queue {
            let manifest = dir.join("Cargo.toml");
            if let Ok(contents) = fs::read_to_string(&manifest)
                && let Ok(NameOnly {
                    package: Some(package),
                }) = toml::from_str(&contents)
            {
                packages.entry(package.name).or_insert_with(|| dir.clone());
            }
            let mut children: Vec<PathBuf> = fs::read_dir(&dir)
                .with_context(|| format!("Failed to read {}", dir.display()))?
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
            next.extend(children);
        }
        queue = next;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_of_the_workspace_puts_a_git_dependency_at_a_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let lock = root.join("Cargo.lock");
        assert_eq!(
            patched_dir(&lock, "shrink_wrap").unwrap(),
            Some(root.join("./shrink_wrap/shrink_wrap"))
        );
        assert_eq!(patched_dir(&lock, "no_such_package").unwrap(), None);
    }
}
