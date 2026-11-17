//! Assembles the WireWeaver book from the pages of the repositories it takes by git. `book_sources.toml` at the
//! repository root names each one either by a package `Cargo.lock` pins, its pages relative to that package's
//! directory, or by its git URL and branch, its pages relative to the repository's root in a checkout of that branch
//! (no revision is pinned: the branch's latest commit, as for a git dependency); and the directory under `docs/` they
//! go to. [assemble] copies them there with the repository's feature tracker as `features.md`, and writes a
//! `.gitignore` naming the copies, so they never enter this repository.

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use ww_snapshots::cargo::CargoSources;

/// The file at the repository root that lists the sources.
pub const SOURCES_FILE: &str = "book_sources.toml";

/// Most pages one source may bring.
const PAGES_MAX: usize = 256;

/// Environment variable naming the directory the checkouts of git sources are kept in.
pub const CHECKOUTS_ENV: &str = "WW_BOOK_CHECKOUTS";

/// One repository whose pages are part of the book.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// A package of that repository, as `Cargo.lock` names it; or [Source::git]
    pub package: Option<String>,
    /// The repository's git URL, taken at [Source::branch]; or [Source::package]
    pub git: Option<String>,
    /// The branch of [Source::git]
    pub branch: Option<String>,
    /// The repository's name as the book shows it; [Source::package], or the URL's last path segment, when left out
    pub name: Option<String>,
    /// Directory of its pages, relative to the package's directory (the repository root for a git source)
    pub pages: PathBuf,
    /// Directory under `docs/` the pages go to
    pub into: PathBuf,
    /// Its feature tracker, relative to the package's directory (the repository root for a git source)
    pub features: Option<PathBuf>,
    /// File name of the tracker's page under [Source::into]; [TRACKER_PAGE] when left out
    pub features_page: Option<String>,
}

/// File name of a tracker's page when its source names none.
pub const TRACKER_PAGE: &str = "features.md";

impl Source {
    /// File name of the tracker's page under [Source::into].
    pub fn tracker_page(&self) -> &str {
        self.features_page.as_deref().unwrap_or(TRACKER_PAGE)
    }

    /// The repository's name as the book shows it.
    pub fn name(&self) -> &str {
        if let Some(name) = self.name.as_deref().or(self.package.as_deref()) {
            return name;
        }
        let url = self.git.as_deref().unwrap_or_default().trim_end_matches('/');
        let last = url.rsplit(['/', ':']).next().unwrap_or(url);
        last.strip_suffix(".git").unwrap_or(last)
    }
}

/// Whether [locate] brings an existing checkout of a git source up to its branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    /// Fetch the branch; keep the checkout as it is when the fetch fails (offline)
    Fetch,
    /// Use the checkout as it is; clone only one that is missing
    Keep,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcesFile {
    #[serde(default)]
    source: Vec<Source>,
}

/// A [Source] with the directory of its package in the checkout `Cargo.lock` pins, or of its git checkout.
#[derive(Debug, Clone)]
pub struct Located {
    /// What `book_sources.toml` says
    pub source: Source,
    /// The package's directory, or the git checkout's root
    pub dir: PathBuf,
    /// The commit of a git source's checkout, and why it is not the branch's latest one when the fetch failed
    pub commit: Option<(String, Option<String>)>,
}

impl Located {
    /// The directory of its pages.
    pub fn pages_dir(&self) -> PathBuf {
        self.dir.join(&self.source.pages)
    }

    /// Its feature tracker's text, if it has one.
    pub fn tracker(&self) -> Result<Option<String>> {
        let Some(features) = &self.source.features else {
            return Ok(None);
        };
        let path = self.dir.join(features);
        fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))
            .map(Some)
    }
}

/// The sources `root/book_sources.toml` lists: packages located through `root`'s `Cargo.lock`, git sources in their
/// checkouts (see [checkouts_dir]), cloned when missing and fetched as `update` says.
pub fn locate(root: &Path, update: Update) -> Result<Vec<Located>> {
    let path = root.join(SOURCES_FILE);
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let file: SourcesFile =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let mut package_dirs = None;
    file.source
        .into_iter()
        .map(|source| {
            ensure!(
                source.into.is_relative() && !source.into.as_os_str().is_empty(),
                "{SOURCES_FILE}: `into` of {} must be a directory under docs/",
                source.name()
            );
            match (&source.package, &source.git, &source.branch) {
                (Some(package), None, None) => {
                    if package_dirs.is_none() {
                        let mut cargo = CargoSources::default();
                        cargo.entry_dir = Some(root.to_path_buf());
                        package_dirs = Some(cargo.locked_package_dirs().with_context(|| {
                            format!(
                                "finding the packages of {}",
                                root.join("Cargo.lock").display()
                            )
                        })?);
                    }
                    let dirs = package_dirs.as_ref().context("package dirs set above")?;
                    let Some((_, _, dir)) = dirs.iter().find(|(name, _, _)| name == package) else {
                        bail!(
                            "{SOURCES_FILE}: {package} is not in Cargo.lock, or Cargo has not fetched it yet (cargo fetch)"
                        );
                    };
                    Ok(Located {
                        dir: dir.clone(),
                        source,
                        commit: None,
                    })
                }
                (None, Some(url), Some(branch)) => {
                    let dir = checkouts_dir()?.join(source.name());
                    let commit = checkout(url, branch, &dir, update)?;
                    Ok(Located {
                        source,
                        dir,
                        commit: Some(commit),
                    })
                }
                _ => bail!(
                    "{SOURCES_FILE}: {} needs either `package`, or `git` with `branch`",
                    source.name()
                ),
            }
        })
        .collect()
}

/// Where the checkouts of git sources are kept: [CHECKOUTS_ENV], else `ww_book/` in the user's cache directory
/// (`$XDG_CACHE_HOME`, else `~/.cache`), shared by every checkout of this repository.
pub fn checkouts_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os(CHECKOUTS_ENV) {
        return Ok(PathBuf::from(dir));
    }
    let cache = match std::env::var_os("XDG_CACHE_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::home_dir()
            .context("no home directory for the checkouts of git sources")?
            .join(".cache"),
    };
    Ok(cache.join("ww_book"))
}

/// Runs `git` with `args` in `dir`, returns its trimmed output.
fn git(dir: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    if let Some(dir) = dir {
        command.arg("-C").arg(dir);
    }
    let output = command
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Brings the checkout of `url` at `dir` to `branch`: a clone when it is missing, else a fetch when `update` asks
/// for one, which leaves the checkout as it is when it fails. Returns the checkout's commit and, when a fetch failed,
/// why.
fn checkout(url: &str, branch: &str, dir: &Path, update: Update) -> Result<(String, Option<String>)> {
    let tracking = format!("refs/remotes/origin/{branch}");
    if !dir.join(".git").exists() {
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let target = dir.to_string_lossy();
        git(
            None,
            &["clone", "-q", "--single-branch", "--branch", branch, url, &target],
        )
        .with_context(|| format!("cloning {url} for the book"))?;
        return Ok((git(Some(dir), &["rev-parse", "HEAD"])?, None));
    }
    let origin = git(Some(dir), &["remote", "get-url", "origin"])?;
    ensure!(
        origin == url,
        "{} is a checkout of {origin}, not {url}: remove it to take the new URL",
        dir.display()
    );
    let mut stale = None;
    if update == Update::Fetch {
        let refspec = format!("+refs/heads/{branch}:{tracking}");
        match git(Some(dir), &["fetch", "-q", "origin", &refspec]) {
            Ok(_) => {
                git(Some(dir), &["checkout", "-q", "--detach", &tracking])?;
            }
            Err(e) => stale = Some(format!("{e:#}")),
        }
    }
    Ok((git(Some(dir), &["rev-parse", "HEAD"])?, stale))
}

/// The pages (`*.md`) of a source's pages directory, by file name, sorted.
fn pages(located: &Located) -> Result<Vec<String>> {
    let dir = located.pages_dir();
    let mut names = vec![];
    for entry in fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
        let entry = entry.with_context(|| format!("listing {}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") && entry.file_type()?.is_file() {
            names.push(name);
        }
        ensure!(
            names.len() <= PAGES_MAX,
            "{} has more than {PAGES_MAX} pages",
            dir.display()
        );
    }
    names.sort();
    Ok(names)
}

/// Where `page` (a path under `docs/`, as the book links it) is: in `root/docs/`, or in a source's pages. `None`
/// when neither has it.
pub fn find_page(root: &Path, sources: &[Located], page: &Path) -> Option<PathBuf> {
    let own = root.join("docs").join(page);
    if own.is_file() {
        return Some(own);
    }
    sources.iter().find_map(|located| {
        let rest = page.strip_prefix(&located.source.into).ok()?;
        let path = located.pages_dir().join(rest);
        path.is_file().then_some(path)
    })
}

/// Name of the `.gitignore` each target directory gets.
const GITIGNORE: &str = ".gitignore";

/// First line of a `.gitignore` [assemble] wrote: one it finds without it is this repository's own.
const GITIGNORE_HEAD: &str =
    "# Written by ww-book assemble: pages copied from other repositories (book_sources.toml)";

/// Copies every source's pages into `root/docs/<into>/`, its tracker as `features.md` there, and writes the
/// directory's `.gitignore`. Refuses a page this repository has itself, so a page lives in one place. Returns the
/// files written, relative to `root`.
pub fn assemble(root: &Path, sources: &[Located]) -> Result<Vec<PathBuf>> {
    let mut written = vec![];
    // per target directory: what an earlier run copied there, what this one does
    let mut by_dir: Vec<(PathBuf, Vec<String>, Vec<String>)> = vec![];
    for located in sources {
        let into = root.join("docs").join(&located.source.into);
        fs::create_dir_all(&into).with_context(|| format!("creating {}", into.display()))?;
        let previous = copied_before(&into)?;
        let mut names = pages(located)?;
        if located.source.features.is_some() {
            let page = located.source.tracker_page();
            ensure!(
                page.ends_with(".md") && !page.contains(['/', '\\']) && !names.iter().any(|n| n == page),
                "{SOURCES_FILE}: `features_page` of {} must be a file name ending in .md that is none of its pages",
                located.source.name()
            );
            names.push(page.to_owned());
        }
        for name in &names {
            let target = into.join(name);
            ensure!(
                !target.exists() || previous.contains(name),
                "{} is in this repository and in {}'s pages: a page lives in one place",
                target.display(),
                located.source.name()
            );
            let text = if located.source.features.is_some() && name == located.source.tracker_page() {
                tracker_page(located)?
            } else {
                let from = located.pages_dir().join(name);
                fs::read_to_string(&from).with_context(|| format!("reading {}", from.display()))?
            };
            fs::write(&target, text).with_context(|| format!("writing {}", target.display()))?;
            written.push(target.strip_prefix(root).unwrap_or(&target).to_path_buf());
        }
        match by_dir.iter_mut().find(|(dir, _, _)| *dir == into) {
            Some((_, _, all)) => all.extend(names),
            None => by_dir.push((into, previous, names)),
        }
    }
    for (dir, previous, names) in by_dir {
        // a page its repository no longer has goes from the book too
        for gone in previous
            .iter()
            .filter(|p| *p != GITIGNORE && !names.contains(p))
        {
            let path = dir.join(gone);
            if path.is_file() {
                fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            }
        }
        let mut text = format!("{GITIGNORE_HEAD}\n/{GITIGNORE}\n");
        for name in names {
            text.push_str(&format!("/{name}\n"));
        }
        let path = dir.join(GITIGNORE);
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(written)
}

/// The names a `.gitignore` of an earlier [assemble] lists in `dir`; none when there is no such file.
fn copied_before(dir: &Path) -> Result<Vec<String>> {
    let path = dir.join(GITIGNORE);
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(vec![]);
    };
    ensure!(
        text.starts_with(GITIGNORE_HEAD),
        "{} is this repository's own, not one ww-book wrote",
        path.display()
    );
    Ok(text
        .lines()
        .filter_map(|line| line.strip_prefix('/'))
        .map(str::to_owned)
        .collect())
}

/// A source's feature tracker as a page of the book. Its links into its own pages directory (`](docs/ww_si.md)` in a
/// tracker next to `docs/`) point beside it, where the book has those pages.
fn tracker_page(located: &Located) -> Result<String> {
    let tracker = located
        .tracker()?
        .context("tracker_page is only called for a source with a tracker")?;
    let tracker = match pages_from_tracker(&located.source) {
        Some(prefix) => tracker.replace(&format!("]({prefix}/"), "]("),
        None => tracker,
    };
    Ok(format!(
        "---\nicon: lucide/list-checks\nlevel: advanced\n---\n\n{tracker}"
    ))
}

/// The pages directory as the tracker's links write it, relative to the tracker's directory (`docs`), when it is
/// below that directory.
fn pages_from_tracker(source: &Source) -> Option<String> {
    let tracker_dir = source.features.as_ref()?.parent()?;
    let rest = source.pages.strip_prefix(tracker_dir).ok()?;
    let prefix = rest.to_str()?.trim_end_matches('/');
    (!prefix.is_empty()).then(|| prefix.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ww_book_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn located(dir: &Path) -> Located {
        Located {
            source: Source {
                package: Some("fmt".to_owned()),
                git: None,
                branch: None,
                name: None,
                pages: "../docs".into(),
                into: "serdes".into(),
                features: Some("../FEATURES.md".into()),
                features_page: None,
            },
            dir: dir.join("fmt"),
            commit: None,
        }
    }

    #[test]
    fn copies_pages_and_tracker_and_ignores_them() {
        let dir = scratch("copies");
        fs::create_dir_all(dir.join("other/docs")).unwrap();
        fs::create_dir_all(dir.join("other/fmt")).unwrap();
        fs::write(dir.join("other/docs/wire.md"), "# Wire\n").unwrap();
        fs::write(dir.join("other/docs/notes.txt"), "not a page").unwrap();
        fs::write(dir.join("other/FEATURES.md"), "# fmt features\n").unwrap();
        let root = dir.join("book");
        fs::create_dir_all(root.join("docs/serdes")).unwrap();
        fs::write(root.join("docs/serdes/own.md"), "# Own\n").unwrap();
        let sources = [located(&dir.join("other"))];

        let written = assemble(&root, &sources).unwrap();
        assert_eq!(
            written,
            [
                PathBuf::from("docs/serdes/wire.md"),
                PathBuf::from("docs/serdes/features.md")
            ]
        );
        assert_eq!(
            fs::read_to_string(root.join("docs/serdes/wire.md")).unwrap(),
            "# Wire\n"
        );
        assert!(
            fs::read_to_string(root.join("docs/serdes/features.md"))
                .unwrap()
                .ends_with("level: advanced\n---\n\n# fmt features\n")
        );
        let ignore = fs::read_to_string(root.join("docs/serdes/.gitignore")).unwrap();
        assert!(
            ignore.ends_with("/.gitignore\n/wire.md\n/features.md\n"),
            "{ignore}"
        );
        // a second run replaces its own copies, and drops a page the source no longer has
        fs::remove_file(dir.join("other/docs/wire.md")).unwrap();
        assemble(&root, &sources).unwrap();
        assert!(!root.join("docs/serdes/wire.md").exists());
        assert_eq!(
            find_page(&root, &sources, Path::new("serdes/own.md")),
            Some(root.join("docs/serdes/own.md"))
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tracker_links_into_its_pages_point_beside_it() {
        let dir = scratch("tracker_links");
        fs::create_dir_all(dir.join("other/docs")).unwrap();
        fs::create_dir_all(dir.join("other/fmt")).unwrap();
        fs::write(dir.join("other/docs/wire.md"), "# Wire\n").unwrap();
        fs::write(
            dir.join("other/FEATURES.md"),
            "[wire](docs/wire.md#bits), [site](https://x.org/docs/a)\n",
        )
        .unwrap();
        let root = dir.join("book");
        fs::create_dir_all(root.join("docs")).unwrap();
        assemble(&root, &[located(&dir.join("other"))]).unwrap();
        let page = fs::read_to_string(root.join("docs/serdes/features.md")).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert!(
            page.ends_with("[wire](wire.md#bits), [site](https://x.org/docs/a)\n"),
            "{page}"
        );
    }

    #[test]
    fn a_source_is_shown_by_its_name_or_package() {
        let mut source = located(Path::new("/nowhere")).source;
        assert_eq!(source.name(), "fmt");
        source.name = Some("fmt_repo".to_owned());
        assert_eq!(source.name(), "fmt_repo");
        source.name = None;
        source.package = None;
        source.git = Some("https://github.com/vhrdtech/ww_stdlib.git".to_owned());
        assert_eq!(source.name(), "ww_stdlib");
    }

    /// A git source is cloned at its branch, kept as it is without an update, fetched with one, and kept at its
    /// commit when the fetch fails (offline).
    #[test]
    fn a_git_source_follows_its_branch() {
        let dir = scratch("git_source");
        let upstream = dir.join("upstream");
        fs::create_dir_all(upstream.join("docs")).unwrap();
        fs::write(upstream.join("docs/page.md"), "# One\n").unwrap();
        let commit_all = |message: &str| {
            git(Some(&upstream), &["add", "-A"]).unwrap();
            git(
                Some(&upstream),
                &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-qm", message],
            )
            .unwrap();
            git(Some(&upstream), &["rev-parse", "HEAD"]).unwrap()
        };
        git(Some(&upstream), &["init", "-q", "-b", "main"]).unwrap();
        let first = commit_all("one");
        let url = upstream.to_string_lossy().into_owned();
        let checkout_dir = dir.join("checkouts/upstream");

        let cloned = checkout(&url, "main", &checkout_dir, Update::Keep).unwrap();
        assert_eq!(cloned, (first.clone(), None));
        fs::write(upstream.join("docs/page.md"), "# Two\n").unwrap();
        let second = commit_all("two");
        let kept = checkout(&url, "main", &checkout_dir, Update::Keep).unwrap();
        assert_eq!(kept, (first.clone(), None));
        let fetched = checkout(&url, "main", &checkout_dir, Update::Fetch).unwrap();
        assert_eq!(fetched, (second.clone(), None));
        assert_eq!(
            fs::read_to_string(checkout_dir.join("docs/page.md")).unwrap(),
            "# Two\n"
        );

        fs::remove_dir_all(&upstream).unwrap();
        let offline = checkout(&url, "main", &checkout_dir, Update::Fetch).unwrap();
        assert_eq!(offline.0, second);
        assert!(offline.1.is_some(), "{offline:?}");
        let other = checkout("https://example.org/other", "main", &checkout_dir, Update::Keep);
        fs::remove_dir_all(&dir).unwrap();
        assert!(format!("{:#}", other.unwrap_err()).contains("remove it"));
    }

    #[test]
    fn a_page_lives_in_one_place() {
        let dir = scratch("one_place");
        fs::create_dir_all(dir.join("other/docs")).unwrap();
        fs::create_dir_all(dir.join("other/fmt")).unwrap();
        fs::write(dir.join("other/docs/wire.md"), "# Wire\n").unwrap();
        fs::write(dir.join("other/FEATURES.md"), "# fmt features\n").unwrap();
        let root = dir.join("book");
        fs::create_dir_all(root.join("docs/serdes")).unwrap();
        fs::write(root.join("docs/serdes/wire.md"), "# Ours\n").unwrap();
        let err = assemble(&root, &[located(&dir.join("other"))]).unwrap_err();
        fs::remove_dir_all(&dir).unwrap();
        assert!(
            err.to_string().contains("a page lives in one place"),
            "{err}"
        );
    }

    #[test]
    fn finds_a_page_in_a_source_before_it_is_copied() {
        let dir = scratch("find");
        fs::create_dir_all(dir.join("other/docs")).unwrap();
        fs::create_dir_all(dir.join("other/fmt")).unwrap();
        fs::write(dir.join("other/docs/wire.md"), "# Wire\n").unwrap();
        let root = dir.join("book");
        fs::create_dir_all(root.join("docs")).unwrap();
        let sources = [located(&dir.join("other"))];
        let found = find_page(&root, &sources, Path::new("serdes/wire.md"));
        let missing = find_page(&root, &sources, Path::new("serdes/none.md"));
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found, Some(dir.join("other/fmt/../docs/wire.md")));
        assert_eq!(missing, None);
    }
}
