//! Assembles the WireWeaver book from the pages of the repositories it takes by git. `book_sources.toml` at the
//! repository root names each one by a package `Cargo.lock` pins, where its pages are relative to that package's
//! directory, and the directory under `docs/` they go to. [assemble] copies them there with the repository's feature
//! tracker as `features.md`, and writes a `.gitignore` naming the copies, so they never enter this repository.

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use ww_snapshots::cargo::CargoSources;

/// The file at the repository root that lists the sources.
pub const SOURCES_FILE: &str = "book_sources.toml";

/// Most pages one source may bring.
const PAGES_MAX: usize = 256;

/// One repository whose pages are part of the book.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// A package of that repository, as `Cargo.lock` names it
    pub package: String,
    /// Directory of its pages, relative to the package's directory
    pub pages: PathBuf,
    /// Directory under `docs/` the pages go to
    pub into: PathBuf,
    /// Its feature tracker, relative to the package's directory
    pub features: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcesFile {
    #[serde(default)]
    source: Vec<Source>,
}

/// A [Source] with the directory of its package in the checkout `Cargo.lock` pins.
#[derive(Debug, Clone)]
pub struct Located {
    /// What `book_sources.toml` says
    pub source: Source,
    /// The package's directory
    pub dir: PathBuf,
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

/// The sources `root/book_sources.toml` lists, located through `root`'s `Cargo.lock`.
pub fn locate(root: &Path) -> Result<Vec<Located>> {
    let path = root.join(SOURCES_FILE);
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let file: SourcesFile =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let mut cargo = CargoSources::default();
    cargo.entry_dir = Some(root.to_path_buf());
    let dirs = cargo.locked_package_dirs().with_context(|| {
        format!(
            "finding the packages of {}",
            root.join("Cargo.lock").display()
        )
    })?;
    file.source
        .into_iter()
        .map(|source| {
            ensure!(
                source.into.is_relative() && !source.into.as_os_str().is_empty(),
                "{SOURCES_FILE}: `into` of {} must be a directory under docs/",
                source.package
            );
            let Some((_, _, dir)) = dirs.iter().find(|(name, _, _)| *name == source.package) else {
                bail!(
                    "{SOURCES_FILE}: {} is not in Cargo.lock, or Cargo has not fetched it yet (cargo fetch)",
                    source.package
                );
            };
            Ok(Located {
                source,
                dir: dir.clone(),
            })
        })
        .collect()
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
            names.push("features.md".to_owned());
        }
        for name in &names {
            let target = into.join(name);
            ensure!(
                !target.exists() || previous.contains(name),
                "{} is in this repository and in {}'s pages: a page lives in one place",
                target.display(),
                located.source.package
            );
            let text = if name == "features.md" {
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

/// A source's feature tracker as a page of the book.
fn tracker_page(located: &Located) -> Result<String> {
    let tracker = located
        .tracker()?
        .context("tracker_page is only called for a source with a tracker")?;
    Ok(format!(
        "---\nicon: lucide/list-checks\nlevel: advanced\n---\n\n{tracker}"
    ))
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
                package: "fmt".to_owned(),
                pages: "../docs".into(),
                into: "serdes".into(),
                features: Some("../FEATURES.md".into()),
            },
            dir: dir.join("fmt"),
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
