//! `ww-book check`: the book's frame. The nav in `zensical.toml` is read in click order (next/next), and each page it
//! lists is read from the repository that owns it. [check] fails:
//!
//! - a nav whose reading ramp (the parts [CHECKS_FILE] names, in order) ever steps down a level, or puts a page in
//!   a part of another level;
//! - a top-level nav entry that is neither a ramp part nor a shelf outside it;
//! - a page without a `level:` in its front matter, and a page the nav lists twice;
//! - a bare `rust` fence: outside Nomicon every `rust` fence is an include of a marked section of a compiled source
//!   (`--8<-- "path:section"`); a Nomicon page may also show a sketch, marked `title="sketch: ..."`. The pages not
//!   rewritten yet are waived in [CHECKS_FILE] with their count, which may only go down;
//! - a file under [GENERATED_DIR] that no page includes.

use crate::{Located, find_page};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// The file at the repository root that holds the ramp, the shelves outside it and the waived fences.
pub const CHECKS_FILE: &str = "book_checks.toml";

/// The book's configuration, holding its nav.
pub const ZENSICAL_FILE: &str = "zensical.toml";

/// Where golden tests write what a reader sees (transcripts, bytes, chart data), relative to the repository root.
pub const GENERATED_DIR: &str = "docs/generated";

/// Most entries one nav may hold, counting sections.
const NAV_ENTRIES_MAX: usize = 1024;

/// Deepest a nav section may nest.
const NAV_DEPTH_MAX: usize = 8;

/// How deep a page goes, from its front matter's `level:` (`docs/intro/reading.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// What a reader needs to use WireWeaver
    Basic,
    /// Every option and when to pick it
    Advanced,
    /// The internals
    Nomicon,
}

impl Level {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "basic" => Some(Self::Basic),
            "advanced" => Some(Self::Advanced),
            "nomicon" => Some(Self::Nomicon),
            _ => None,
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Basic => "basic",
            Self::Advanced => "advanced",
            Self::Nomicon => "nomicon",
        })
    }
}

/// One part of the reading ramp: a top-level nav section whose pages all have one level.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    /// The section's title in the nav
    pub title: String,
    /// The level of every page in it
    pub level: Level,
}

/// What [CHECKS_FILE] holds.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checks {
    /// The parts of the ramp, in reading order
    pub part: Vec<Part>,
    /// Titles of the top-level sections after the ramp (Reference, Articles), whose pages may have any level
    pub outside: Vec<String>,
    /// Pages still holding bare `rust` fences, as paths under `docs/`, with how many each may hold
    #[serde(default)]
    pub bare_fences: BTreeMap<String, usize>,
}

impl Checks {
    /// Reads `root/`[CHECKS_FILE].
    pub fn read(root: &Path) -> Result<Self> {
        let path = root.join(CHECKS_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// One page of the nav, in click order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavPage {
    /// Its path under `docs/`
    pub page: String,
    /// The top-level section it is in
    pub part: String,
}

/// What `zensical.toml`'s `project.nav` holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nav {
    /// The titles of its top-level sections, in order
    pub sections: Vec<String>,
    /// Its pages in click order; links to other sites are left out
    pub pages: Vec<NavPage>,
}

/// Reads the nav of `zensical`, the text of `zensical.toml`.
pub fn read_nav(zensical: &str) -> Result<Nav> {
    let value: toml::Table = toml::from_str(zensical).context("parsing the nav's file")?;
    let Some(nav) = value
        .get("project")
        .and_then(|project| project.get("nav"))
        .and_then(toml::Value::as_array)
    else {
        bail!("no `project.nav` array");
    };
    let mut sections = Vec::new();
    let mut pages = Vec::new();
    let mut entries = 0;
    for entry in nav {
        let Some((title, value)) = single_entry(entry) else {
            bail!("a top-level nav entry is not a section of its own: {entry:?}");
        };
        sections.push(title.to_owned());
        walk(value, title, 1, &mut pages, &mut entries)?;
    }
    Ok(Nav { sections, pages })
}

/// The title and value of a nav entry written `{ "Title" = value }`.
fn single_entry(entry: &toml::Value) -> Option<(&str, &toml::Value)> {
    let table = entry.as_table()?;
    let mut items = table.iter();
    let (title, value) = items.next()?;
    items.next().is_none().then_some((title.as_str(), value))
}

fn walk(
    value: &toml::Value,
    part: &str,
    depth: usize,
    pages: &mut Vec<NavPage>,
    entries: &mut usize,
) -> Result<()> {
    *entries += 1;
    if *entries > NAV_ENTRIES_MAX {
        bail!("the nav has more than {NAV_ENTRIES_MAX} entries");
    }
    if depth > NAV_DEPTH_MAX {
        bail!("the nav nests deeper than {NAV_DEPTH_MAX} sections in `{part}`");
    }
    match value {
        toml::Value::String(page) => {
            if !page.contains("://") {
                pages.push(NavPage {
                    page: page.clone(),
                    part: part.to_owned(),
                });
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                walk(item, part, depth + 1, pages, entries)?;
            }
        }
        toml::Value::Table(_) => {
            let Some((_, inner)) = single_entry(value) else {
                bail!("a nav entry in `{part}` has more than one title: {value:?}");
            };
            walk(inner, part, depth + 1, pages, entries)?;
        }
        other => bail!("a nav entry in `{part}` is neither a page nor a section: {other:?}"),
    }
    Ok(())
}

/// The `level:` of a page's front matter (the block between the first two `---` lines).
pub fn front_matter_level(text: &str) -> Option<Result<Level, String>> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    for line in lines {
        if line.trim_end() == "---" {
            return None;
        }
        if let Some(value) = line.strip_prefix("level:") {
            let value = value.trim();
            return Some(Level::parse(value).ok_or_else(|| value.to_owned()));
        }
    }
    None
}

/// A `rust` fence of a page that is not an include.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BareFence {
    /// Its opening line, counted from 1
    pub line: usize,
    /// Whether it is marked as a sketch (`title="sketch..."`), which a Nomicon page may show
    pub sketch: bool,
}

/// The `rust` fences of a page (`rs` too) whose body is not only `--8<--` include lines. A fence inside another
/// fence (a ````` ````text ````` showing Markdown) is the outer fence's text, not a fence of its own.
pub fn bare_rust_fences(text: &str) -> Vec<BareFence> {
    let mut bare = Vec::new();
    // the open fence: its character, length, whether it is `rust`, its line and whether it is a sketch, and
    // whether every body line so far is an include (`None`: no body line yet)
    let mut open: Option<(char, usize, bool, BareFence, Option<bool>)> = None;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if let Some((ch, len, rust, fence, includes)) = &mut open {
            if trimmed.len() >= *len && trimmed.chars().all(|c| c == *ch) {
                if *rust && *includes != Some(true) {
                    bare.push(*fence);
                }
                open = None;
            } else if !trimmed.is_empty() {
                let include = trimmed.starts_with("--8<--");
                *includes = Some(includes.unwrap_or(true) && include);
            }
            continue;
        }
        let Some(ch) = trimmed.chars().next().filter(|c| *c == '`' || *c == '~') else {
            continue;
        };
        let len = trimmed.chars().take_while(|c| *c == ch).count();
        if len < 3 {
            continue;
        }
        let info = trimmed[len..].trim();
        if ch == '`' && info.contains('`') {
            continue;
        }
        let lang = info
            .split(|c: char| c.is_whitespace() || c == '}')
            .next()
            .unwrap_or_default()
            .trim_start_matches(['{', '.']);
        let fence = BareFence {
            line: index + 1,
            sketch: is_sketch(info),
        };
        open = Some((ch, len, lang == "rust" || lang == "rs", fence, None));
    }
    if let Some((_, _, true, fence, includes)) = open
        && includes != Some(true)
    {
        bare.push(fence);
    }
    bare
}

/// Whether a fence's info string carries `title="sketch..."`.
fn is_sketch(info: &str) -> bool {
    info.split_once("title=")
        .is_some_and(|(_, rest)| rest.trim_start_matches(['"', '\'']).starts_with("sketch"))
}

/// The files a page includes as snippets (`--8<-- "path"`, `--8<-- "path:section"`), as written.
fn includes(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter_map(|line| {
        let rest = line.trim().strip_prefix("--8<--")?.trim();
        let quoted = rest.strip_prefix('"')?;
        let path = &quoted[..quoted.find('"')?];
        Some(path.split_once(':').map_or(path, |(path, _)| path))
    })
}

/// Where a page comes from, for the fence count: a source taken at a branch's latest commit is not pinned, so its
/// count may drop under this repository's feet, and a lower count than waived is not a failure there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Pinned,
    Branch,
}

/// The text and origin of `page`: from the repository that owns it (a source's pages before an assembled copy in
/// `docs/`), or this repository's `docs/`.
fn read_page(root: &Path, sources: &[Located], page: &str) -> Option<(String, Origin)> {
    let path = Path::new(page);
    let from_source = sources.iter().find_map(|located| {
        if located.source.book.is_some() {
            return None;
        }
        let rest = path.strip_prefix(&located.source.into).ok()?;
        let file = located.pages_dir().join(rest);
        let origin = if located.source.git.is_some() {
            Origin::Branch
        } else {
            Origin::Pinned
        };
        file.is_file().then_some((file, origin))
    });
    let (file, origin) = match from_source {
        Some(found) => found,
        None => (find_page(root, &[], path)?, Origin::Pinned),
    };
    Some((fs::read_to_string(file).ok()?, origin))
}

/// Everything [check] found wrong, one line each.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Problems(pub Vec<String>);

impl fmt::Display for Problems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for problem in &self.0 {
            writeln!(f, "{problem}")?;
        }
        Ok(())
    }
}

/// Checks the book at `root` (see the module's docs), its copied pages read from `sources`.
pub fn check(root: &Path, sources: &[Located]) -> Result<Problems> {
    let checks = Checks::read(root)?;
    let zensical_path = root.join(ZENSICAL_FILE);
    let zensical = fs::read_to_string(&zensical_path)
        .with_context(|| format!("reading {}", zensical_path.display()))?;
    let nav = read_nav(&zensical).with_context(|| format!("in {}", zensical_path.display()))?;
    let mut problems = Vec::new();

    let parts: BTreeMap<&str, Level> = checks
        .part
        .iter()
        .map(|part| (part.title.as_str(), part.level))
        .collect();
    let sections: Vec<&str> = nav.sections.iter().map(String::as_str).collect();
    let ramp: Vec<&str> = checks.part.iter().map(|part| part.title.as_str()).collect();
    let in_ramp: Vec<&str> = sections
        .iter()
        .copied()
        .filter(|s| parts.contains_key(s))
        .collect();
    if in_ramp != ramp {
        problems.push(format!(
            "the nav's ramp parts are {in_ramp:?}, {CHECKS_FILE} names {ramp:?} in that order"
        ));
    }
    if let Some(after) = sections
        .iter()
        .position(|s| checks.outside.iter().any(|o| o == s))
        && let Some(late) = sections[after..].iter().find(|s| parts.contains_key(*s))
    {
        problems.push(format!(
            "ramp part `{late}` comes after a shelf outside the ramp (`{}`)",
            sections[after]
        ));
    }
    for section in &sections {
        if !parts.contains_key(section) && !checks.outside.iter().any(|o| o == section) {
            problems.push(format!(
                "top-level section `{section}` is neither a ramp part nor a shelf outside it in {CHECKS_FILE}"
            ));
        }
    }

    let mut seen = BTreeSet::new();
    let mut previous: Option<(Level, &str)> = None;
    let mut included = BTreeSet::new();
    let mut waived_seen = BTreeSet::new();
    for NavPage { page, part } in &nav.pages {
        if !seen.insert(page.as_str()) {
            problems.push(format!("{page}: listed twice in the nav"));
            continue;
        }
        let Some((text, origin)) = read_page(root, sources, page) else {
            problems.push(format!("{page}: in the nav, but no such page"));
            continue;
        };
        included.extend(includes(&text).map(str::to_owned));
        let level = match front_matter_level(&text) {
            Some(Ok(level)) => level,
            Some(Err(value)) => {
                problems.push(format!(
                    "{page}: `level: {value}` is none of basic, advanced, nomicon"
                ));
                continue;
            }
            None => {
                problems.push(format!("{page}: no `level:` in its front matter"));
                continue;
            }
        };
        if let Some(&part_level) = parts.get(part.as_str()) {
            if level != part_level {
                problems.push(format!(
                    "{page}: a {level} page in part `{part}`, whose pages are {part_level}"
                ));
            }
            if let Some((before, at)) = previous
                && level < before
            {
                problems.push(format!(
                    "{page}: the nav steps down from {before} ({at}) to {level}"
                ));
            }
            previous = Some((level, page));
        }

        let bare: Vec<BareFence> = bare_rust_fences(&text)
            .into_iter()
            .filter(|fence| level != Level::Nomicon || !fence.sketch)
            .collect();
        let waived = checks.bare_fences.get(page).copied();
        if waived.is_some() {
            waived_seen.insert(page.as_str());
        }
        let allowed = waived.unwrap_or(0);
        if bare.len() > allowed {
            let lines: Vec<String> = bare.iter().map(|f| f.line.to_string()).collect();
            let what = if level == Level::Nomicon {
                "neither an include nor a sketch (`title=\"sketch: ...\"`)"
            } else {
                "not an include of a compiled source (`--8<-- \"path:section\"`)"
            };
            problems.push(format!(
                "{page}: {} `rust` fences {what}, {allowed} waived in {CHECKS_FILE}; lines {}",
                bare.len(),
                lines.join(", ")
            ));
        } else if bare.len() < allowed && origin == Origin::Pinned {
            problems.push(format!(
                "{page}: {} bare `rust` fences, {allowed} waived: lower its count in {CHECKS_FILE} to {}{}",
                bare.len(),
                bare.len(),
                if bare.is_empty() {
                    ", that is, drop the line"
                } else {
                    ""
                }
            ));
        }
    }
    for page in checks.bare_fences.keys() {
        if !waived_seen.contains(page.as_str()) {
            problems.push(format!(
                "{CHECKS_FILE}: `{page}` waives fences of a page the nav does not list"
            ));
        }
    }

    for file in generated_files(root)? {
        if !included.contains(&file) {
            problems.push(format!("{file}: no page in the nav includes it"));
        }
    }
    Ok(Problems(problems))
}

/// The files under `root/`[GENERATED_DIR], as paths from `root` with `/` separators.
fn generated_files(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let mut dirs = vec![PathBuf::from(GENERATED_DIR)];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(root.join(&dir)) else {
            continue;
        };
        for entry in entries {
            let entry = entry.with_context(|| format!("listing {}", dir.display()))?;
            let path = dir.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                dirs.push(path);
            } else {
                let parts: Vec<String> = path
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                files.push(parts.join("/"));
            }
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ww_book_check_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("docs")).unwrap();
        dir
    }

    const CHECKS: &str = r#"
outside = ["Reference"]
[[part]]
title = "Basic"
level = "basic"
[[part]]
title = "Advanced"
level = "advanced"
"#;

    fn page(root: &Path, name: &str, level: &str, body: &str) {
        let path = root.join("docs").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!("---\nlevel: {level}\n---\n\n# Page\n\n{body}"),
        )
        .unwrap();
    }

    fn book(name: &str, nav: &str, checks_extra: &str) -> PathBuf {
        let root = scratch(name);
        fs::write(root.join(CHECKS_FILE), format!("{CHECKS}\n{checks_extra}")).unwrap();
        fs::write(
            root.join(ZENSICAL_FILE),
            format!("[project]\nsite_name = \"t\"\nnav = {nav}\n"),
        )
        .unwrap();
        page(&root, "a.md", "basic", "");
        page(&root, "b.md", "advanced", "");
        page(&root, "c.md", "basic", "");
        root
    }

    fn problems(root: &Path) -> Vec<String> {
        check(root, &[]).unwrap().0
    }

    #[test]
    fn a_nav_in_level_order_passes() {
        let root = book(
            "ok",
            r#"[{ "Basic" = ["a.md", { "Sub" = [{ "C" = "c.md" }] }, { "Site" = "https://example.com/" }] },
               { "Advanced" = ["b.md"] }, { "Reference" = ["x.md"] }]"#,
            "",
        );
        page(&root, "x.md", "basic", "");
        assert_eq!(problems(&root), Vec::<String>::new());
    }

    #[test]
    fn a_step_down_fails() {
        let root = book(
            "step_down",
            r#"[{ "Basic" = ["a.md", "b.md", "c.md"] }, { "Advanced" = [] }]"#,
            "",
        );
        assert_eq!(
            problems(&root),
            [
                "b.md: a advanced page in part `Basic`, whose pages are basic",
                "c.md: the nav steps down from advanced (b.md) to basic",
            ]
        );
    }

    #[test]
    fn a_page_without_level_twice_or_missing_fails() {
        let root = book(
            "no_level",
            r#"[{ "Basic" = ["a.md", "plain.md", "a.md", "gone.md"] }, { "Advanced" = ["b.md"] }]"#,
            "",
        );
        fs::write(root.join("docs/plain.md"), "# Plain\n").unwrap();
        assert_eq!(
            problems(&root),
            [
                "plain.md: no `level:` in its front matter",
                "a.md: listed twice in the nav",
                "gone.md: in the nav, but no such page",
            ]
        );
    }

    #[test]
    fn parts_out_of_order_or_unknown_fail() {
        let root = book(
            "parts",
            r#"[{ "Advanced" = ["b.md"] }, { "Reference" = ["c.md"] }, { "Basic" = ["a.md"] }, { "Extra" = [] }]"#,
            "",
        );
        assert_eq!(
            problems(&root),
            [
                r#"the nav's ramp parts are ["Advanced", "Basic"], book_checks.toml names ["Basic", "Advanced"] in that order"#,
                "ramp part `Basic` comes after a shelf outside the ramp (`Reference`)",
                "top-level section `Extra` is neither a ramp part nor a shelf outside it in book_checks.toml",
                "a.md: the nav steps down from advanced (b.md) to basic",
            ]
        );
    }

    #[test]
    fn a_bare_fence_fails_and_an_include_passes() {
        let root = book(
            "fences",
            r#"[{ "Basic" = ["a.md", "c.md"] }, { "Advanced" = ["b.md"] }]"#,
            "",
        );
        page(
            &root,
            "a.md",
            "basic",
            "```rust\n--8<-- \"examples/x/src/lib.rs:part\"\n```\n",
        );
        page(
            &root,
            "c.md",
            "basic",
            "Text.\n\n```rust\nfn main() {}\n```\n",
        );
        assert_eq!(
            problems(&root),
            [
                "c.md: 1 `rust` fences not an include of a compiled source (`--8<-- \"path:section\"`), 0 waived in book_checks.toml; lines 9"
            ]
        );
    }

    #[test]
    fn waived_fences_may_only_go_down() {
        let nav = r#"[{ "Basic" = ["a.md", "c.md"] }, { "Advanced" = ["b.md"] }]"#;
        let root = book(
            "waived",
            nav,
            "[bare_fences]\n\"a.md\" = 1\n\"c.md\" = 2\n\"gone.md\" = 1\n",
        );
        page(&root, "a.md", "basic", "```rust\nfn a() {}\n```\n");
        page(&root, "c.md", "basic", "```rust\nfn c() {}\n```\n");
        assert_eq!(
            problems(&root),
            [
                "c.md: 1 bare `rust` fences, 2 waived: lower its count in book_checks.toml to 1",
                "book_checks.toml: `gone.md` waives fences of a page the nav does not list",
            ]
        );
    }

    #[test]
    fn a_nomicon_page_may_show_marked_sketches_only() {
        let root = scratch("nomicon");
        fs::write(
            root.join(CHECKS_FILE),
            "outside = []\n[[part]]\ntitle = \"Nomicon\"\nlevel = \"nomicon\"\n",
        )
        .unwrap();
        fs::write(
            root.join(ZENSICAL_FILE),
            "[project]\nnav = [{ \"Nomicon\" = [\"n.md\"] }]\n",
        )
        .unwrap();
        page(
            &root,
            "n.md",
            "nomicon",
            "```rust title=\"sketch: the loop\"\nloop {}\n```\n\n```rust\nfn bare() {}\n```\n",
        );
        assert_eq!(
            problems(&root),
            [
                "n.md: 1 `rust` fences neither an include nor a sketch (`title=\"sketch: ...\"`), 0 waived in book_checks.toml; lines 11"
            ]
        );
    }

    #[test]
    fn a_generated_file_no_page_includes_fails() {
        let root = book(
            "generated",
            r#"[{ "Basic" = ["a.md", "c.md"] }, { "Advanced" = ["b.md"] }]"#,
            "",
        );
        fs::create_dir_all(root.join("docs/generated/ex")).unwrap();
        fs::write(root.join("docs/generated/ex/used.txt"), "1\n").unwrap();
        fs::write(root.join("docs/generated/ex/orphan.txt"), "2\n").unwrap();
        page(
            &root,
            "a.md",
            "basic",
            "```text\n--8<-- \"docs/generated/ex/used.txt\"\n```\n",
        );
        assert_eq!(
            problems(&root),
            ["docs/generated/ex/orphan.txt: no page in the nav includes it"]
        );
    }

    #[test]
    fn fences_inside_a_longer_fence_are_its_text() {
        let text =
            "````text\n```rust\nfn shown() {}\n```\n````\n\n  ~~~ {.rust}\n  x\n  ~~~\n\n```rs\n";
        assert_eq!(
            bare_rust_fences(text),
            [
                BareFence {
                    line: 7,
                    sketch: false
                },
                BareFence {
                    line: 11,
                    sketch: false
                }
            ]
        );
    }

    #[test]
    fn front_matter_level_is_read_from_the_block_only() {
        assert_eq!(
            front_matter_level("---\nicon: x\nlevel: advanced\n---\n"),
            Some(Ok(Level::Advanced))
        );
        assert_eq!(front_matter_level("# Title\nlevel: basic\n"), None);
        assert_eq!(front_matter_level("---\n---\nlevel: basic\n"), None);
        assert_eq!(
            front_matter_level("---\nlevel: expert\n---\n"),
            Some(Err("expert".to_owned()))
        );
    }
}
