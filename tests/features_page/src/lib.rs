//! Renders `docs/features_today.md`, the docs site's "Features" page (DOC-7, DOC-9): per area a sentence on what
//! it is for, then its features by reading level, each with a short line and a link to its docs page. What the page
//! says is kept by hand in `entries.md` next to this crate, what is done comes from the feature tracker
//! `FEATURES.md`; [`render`] joins the two and fails when they disagree, so a feature that is done can't miss the
//! page. The page is a golden file: `tests/features_page.rs` fails when it is stale, `just features-page` writes it.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Areas of `FEATURES.md` that are not features.
const SKIPPED_AREAS: [&str; 2] = ["How to use this file", "Dropped and superseded"];

/// The `entries.md` area of items left off the page on purpose.
const OMITTED: &str = "Omitted";

/// Status of a `FEATURES.md` item.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Status {
    /// ✅
    Done,
    /// 🧪 done, the check on hardware still open
    HardwareCheckOpen,
    /// 🚧
    InProgress,
    /// Anything else: planned, idea, blocked, to check.
    NotDone,
}

impl Status {
    fn from_marker(marker: &str) -> Self {
        match marker {
            "✅" => Status::Done,
            "🧪" => Status::HardwareCheckOpen,
            "🚧" => Status::InProgress,
            _ => Status::NotDone,
        }
    }

    fn shown(self) -> bool {
        self != Status::NotDone
    }
}

/// A top-level item of `FEATURES.md`.
struct Feature {
    status: Status,
    title: String,
    /// Index into the list of areas.
    area: usize,
}

struct Tracker {
    /// The headings as written, e.g. ``Wire format (`shrink_wrap`, `SW`)``.
    areas: Vec<String>,
    /// By ID, in file order.
    features: Vec<(String, Feature)>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Level {
    Basic,
    Advanced,
    Nomicon,
}

impl Level {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "Basic" => Some(Level::Basic),
            "Advanced" => Some(Level::Advanced),
            "Nomicon" => Some(Level::Nomicon),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Level::Basic => "basic",
            Level::Advanced => "advanced",
            Level::Nomicon => "nomicon",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Level::Basic => "Basic",
            Level::Advanced => "Advanced",
            Level::Nomicon => "Nomicon",
        }
    }

    /// Filled bars of the badge icon.
    fn depth(self) -> u8 {
        self as u8 + 1
    }
}

/// One line of `entries.md`.
struct Entry {
    ids: Vec<String>,
    name: String,
    line: String,
    /// A page under `docs/`, maybe with an anchor.
    link: Option<String>,
    /// Line number in `entries.md`, for errors.
    at: usize,
}

struct PageArea {
    title: String,
    intro: String,
    levels: Vec<(Level, Vec<Entry>)>,
}

struct Entries {
    areas: Vec<PageArea>,
    /// IDs left off the page, with the line they are on.
    omitted: Vec<(String, usize)>,
}

/// The page for `features` (the contents of `FEATURES.md`) and `entries` (of `entries.md`), or what is wrong, one
/// problem per line.
pub fn render(features: &str, entries: &str) -> Result<String, String> {
    let tracker = parse_tracker(features);
    let entries = parse_entries(entries)?;
    check(&tracker, &entries)?;
    let by_id: BTreeMap<&str, &Feature> = tracker
        .features
        .iter()
        .map(|(id, f)| (id.as_str(), f))
        .collect();

    let mut out = String::new();
    out.push_str(
        "---\nicon: lucide/list-checks\nlevel: basic\n---\n\n\
         <!-- Generated from FEATURES.md and tests/features_page/entries.md by `just features-page`, don't edit by \
         hand. -->\n\n\
         # Features\n\n\
         WireWeaver lets you describe a device's API once, as a Rust trait with your own data types, and generates\n\
         both ends from it: the server on the device, `no_std` and without an allocator, and the client on the PC.\n\
         Calls, property reads and streams go over USB, RTT, UDP or WebSocket in `shrink_wrap`, a dense bit-packed\n\
         format that also works on its own. Newer firmware keeps working with older hosts and the other way round.\n\n\
         This page goes through what works today, area by area, in the order you are likely to meet them. Each area\n\
         starts with what it is for, then lists its features by [reading level](levels.md): Basic is what most\n\
         projects use, Advanced is for when the defaults are not enough, Nomicon is about internals and code size.\n\
         Features that work only in part are marked. What is planned, and the details of every item, are in the\n\
         [feature tracker](features.md).\n",
    );
    for area in &entries.areas {
        let _ = write!(out, "\n## {}\n\n{}\n", area.title, area.intro);
        let mut tracker_areas: Vec<usize> = Vec::new();
        for (level, items) in &area.levels {
            let _ = write!(out, "\n{}\n\n", badge(*level));
            for entry in items {
                let status = entry.ids.iter().map(|id| by_id[id.as_str()].status).max();
                for id in &entry.ids {
                    let area = by_id[id.as_str()].area;
                    if !tracker_areas.contains(&area) {
                        tracker_areas.push(area);
                    }
                }
                out.push_str("- ");
                match &entry.link {
                    Some(link) => {
                        let _ = write!(out, "**[{}]({link})**", entry.name);
                    }
                    None => {
                        let _ = write!(out, "**{}**", entry.name);
                    }
                }
                let _ = write!(out, ": {}", entry.line);
                match status {
                    Some(Status::InProgress) => {
                        out.push_str(" <span class=\"ww-status\">in progress</span>")
                    }
                    Some(Status::HardwareCheckOpen) => {
                        out.push_str(" <span class=\"ww-status\">hardware check open</span>")
                    }
                    _ => {}
                }
                out.push('\n');
            }
        }
        let links: Vec<String> = tracker_areas
            .iter()
            .map(|&i| {
                let heading = &tracker.areas[i];
                format!("[{}](features.md#{})", area_title(heading), slug(heading))
            })
            .collect();
        let _ = writeln!(
            out,
            "\n<p class=\"ww-features__more\" markdown>Details and what is planned: {} in the feature tracker.</p>",
            links.join(", ")
        );
    }
    Ok(out)
}

/// The doc pages `entries` links to, without anchors, relative to `docs/`.
pub fn links(entries: &str) -> Vec<String> {
    let Ok(entries) = parse_entries(entries) else {
        return Vec::new();
    };
    let mut links: Vec<String> = entries
        .areas
        .iter()
        .flat_map(|a| &a.levels)
        .flat_map(|(_, items)| items)
        .filter_map(|e| e.link.as_deref())
        .map(|l| l.split_once('#').map_or(l, |(page, _)| page).to_string())
        .collect();
    links.sort();
    links.dedup();
    links
}

/// The level badge of `docs/levels.md` and `overrides/partials/level.html`, as a paragraph.
fn badge(level: Level) -> String {
    let bar = |n: u8, rect: &str| {
        if level.depth() >= n {
            format!("<rect {rect}/>")
        } else {
            format!("<rect {rect} class=\"ww-level__off\"/>")
        }
    };
    format!(
        "<p class=\"ww-features__level\"><span class=\"ww-level ww-level--{}\"><svg class=\"ww-level__icon\" \
         viewBox=\"0 0 14 12\" aria-hidden=\"true\">{}{}{}</svg><span class=\"ww-level__label\">{}</span></span></p>",
        level.name(),
        bar(1, "x=\"0\" y=\"7\" width=\"3.2\" height=\"5\" rx=\"0.6\""),
        bar(
            2,
            "x=\"5.4\" y=\"3.5\" width=\"3.2\" height=\"8.5\" rx=\"0.6\""
        ),
        bar(
            3,
            "x=\"10.8\" y=\"0\" width=\"3.2\" height=\"12\" rx=\"0.6\""
        ),
        level.label(),
    )
}

/// Every problem between the tracker and the entries.
fn check(tracker: &Tracker, entries: &Entries) -> Result<(), String> {
    let mut problems = Vec::new();
    let by_id: BTreeMap<&str, &Feature> = tracker
        .features
        .iter()
        .map(|(id, f)| (id.as_str(), f))
        .collect();
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    let listed = entries
        .areas
        .iter()
        .flat_map(|a| &a.levels)
        .flat_map(|(_, items)| items)
        .flat_map(|e| e.ids.iter().map(move |id| (id.as_str(), e.at)))
        .chain(entries.omitted.iter().map(|(id, at)| (id.as_str(), *at)));
    for (id, at) in listed {
        if let Some(first) = seen.insert(id, at) {
            problems.push(format!("entries.md:{at}: {id} is already on line {first}"));
        }
        match by_id.get(id) {
            None => problems.push(format!("entries.md:{at}: {id} is not in FEATURES.md")),
            Some(f) if !f.status.shown() => problems.push(format!(
                "entries.md:{at}: {id} {} is not done or in progress in FEATURES.md: remove it until it is",
                f.title
            )),
            Some(_) => {}
        }
    }
    for (id, f) in &tracker.features {
        if f.status.shown() && !seen.contains_key(id.as_str()) {
            problems.push(format!(
                "FEATURES.md: {id} {} has no line in tests/features_page/entries.md: add one (or put it under \
                 `# {OMITTED}` with a reason)",
                f.title
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn parse_tracker(features: &str) -> Tracker {
    let mut tracker = Tracker {
        areas: Vec::new(),
        features: Vec::new(),
    };
    let mut skip = true;
    for line in features.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            skip = SKIPPED_AREAS.contains(&heading);
            if !skip {
                tracker.areas.push(heading.to_string());
            }
            continue;
        }
        if skip || tracker.areas.is_empty() {
            continue;
        }
        // Top-level items only: `- ✅ **SW-1 Title**, rest`
        let Some(item) = line.strip_prefix("- ") else {
            continue;
        };
        let Some((marker, rest)) = item.split_once(' ') else {
            continue;
        };
        let Some((id, title)) = id_and_title(rest) else {
            continue;
        };
        tracker.features.push((
            id,
            Feature {
                status: Status::from_marker(marker),
                title,
                area: tracker.areas.len() - 1,
            },
        ));
    }
    tracker
}

/// `**SW-1 Structs, plain enums**, rest` → (`SW-1`, `Structs, plain enums`).
fn id_and_title(rest: &str) -> Option<(String, String)> {
    let bold = rest.strip_prefix("**")?;
    let (bold, _) = bold.split_once("**")?;
    let (id, title) = bold.split_once(' ')?;
    is_id(id).then(|| (id.to_string(), title.trim().to_string()))
}

fn parse_entries(src: &str) -> Result<Entries, String> {
    let mut entries = Entries {
        areas: Vec::new(),
        omitted: Vec::new(),
    };
    let mut problems = Vec::new();
    let mut in_omitted = false;
    for (n, line) in src.lines().enumerate() {
        let at = n + 1;
        if let Some(title) = line.strip_prefix("# ") {
            in_omitted = title == OMITTED;
            if !in_omitted {
                entries.areas.push(PageArea {
                    title: title.trim().to_string(),
                    intro: String::new(),
                    levels: Vec::new(),
                });
            }
            continue;
        }
        if in_omitted {
            if let Some(item) = line.strip_prefix("- ") {
                match item.split_once(" | ") {
                    Some((ids, reason)) if !reason.trim().is_empty() => match parse_ids(ids) {
                        Some(ids) => entries.omitted.extend(ids.into_iter().map(|id| (id, at))),
                        None => {
                            problems.push(format!("entries.md:{at}: not a list of IDs: `{ids}`"))
                        }
                    },
                    _ => problems.push(format!("entries.md:{at}: expected `- IDS | reason`")),
                }
            } else if !line.trim().is_empty() {
                problems.push(format!("entries.md:{at}: expected `- IDS | reason`"));
            }
            continue;
        }
        // The description above the first area.
        let Some(area) = entries.areas.last_mut() else {
            continue;
        };
        if let Some(level) = line.strip_prefix("## ") {
            match Level::parse(level.trim()) {
                Some(level) if area.levels.last().is_none_or(|(last, _)| *last < level) => {
                    area.levels.push((level, Vec::new()))
                }
                Some(_) => problems.push(format!(
                    "entries.md:{at}: levels go Basic, Advanced, Nomicon, each at most once"
                )),
                None => problems.push(format!(
                    "entries.md:{at}: `{level}` is not a level (Basic, Advanced, Nomicon)"
                )),
            }
            continue;
        }
        if let Some(item) = line.strip_prefix("- ") {
            let Some((_, items)) = area.levels.last_mut() else {
                problems.push(format!(
                    "entries.md:{at}: an item before the area's first level"
                ));
                continue;
            };
            match parse_entry(item, at) {
                Ok(entry) => items.push(entry),
                Err(problem) => problems.push(problem),
            }
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        if !area.levels.is_empty() {
            problems.push(format!(
                "entries.md:{at}: text after the first level of an area: an item starts with `- `"
            ));
            continue;
        }
        if !area.intro.is_empty() {
            area.intro.push('\n');
        }
        area.intro.push_str(line.trim_end());
    }
    for area in &entries.areas {
        if area.intro.is_empty() {
            problems.push(format!(
                "entries.md: area `{}` has no opening sentence",
                area.title
            ));
        }
        if area.levels.iter().all(|(_, items)| items.is_empty()) {
            problems.push(format!("entries.md: area `{}` has no items", area.title));
        }
    }
    if problems.is_empty() {
        Ok(entries)
    } else {
        Err(problems.join("\n"))
    }
}

/// `SW-1, SW-2 | name | line | link` (the link may be empty).
fn parse_entry(item: &str, at: usize) -> Result<Entry, String> {
    let parts: Vec<&str> = item.split('|').map(str::trim).collect();
    let [ids, name, line, link] = parts[..] else {
        return Err(format!(
            "entries.md:{at}: expected `- IDS | name | line | link` (an empty link is fine), got {} parts",
            parts.len()
        ));
    };
    let ids =
        parse_ids(ids).ok_or_else(|| format!("entries.md:{at}: not a list of IDs: `{ids}`"))?;
    if name.is_empty() || line.is_empty() {
        return Err(format!(
            "entries.md:{at}: the name and the line can't be empty"
        ));
    }
    if link.contains("://") || link.starts_with('/') || link.contains(' ') {
        return Err(format!(
            "entries.md:{at}: the link is a page under docs/, like `api/streams.md#stream-functions`"
        ));
    }
    Ok(Entry {
        ids,
        name: name.to_string(),
        line: line.to_string(),
        link: (!link.is_empty()).then(|| link.to_string()),
        at,
    })
}

/// `SW-31, SW-32`
fn parse_ids(ids: &str) -> Option<Vec<String>> {
    let ids: Vec<String> = ids.split(',').map(|id| id.trim().to_string()).collect();
    ids.iter().all(|id| is_id(id)).then_some(ids)
}

/// `SW-1`, `TEST-17`
fn is_id(word: &str) -> bool {
    word.split_once('-').is_some_and(|(area, n)| {
        !area.is_empty()
            && area.chars().all(|c| c.is_ascii_uppercase())
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit())
    })
}

/// ``Wire format (`shrink_wrap`, `SW`)`` → `Wire format`
fn area_title(heading: &str) -> &str {
    heading.split_once(" (").map_or(heading, |(title, _)| title)
}

/// The anchor of a heading on the docs site: lower case, every run of other characters than letters, digits and
/// `_` collapsed to one `-`.
fn slug(heading: &str) -> String {
    let mut slug = String::new();
    for c in heading.to_lowercase().chars() {
        if c.is_alphanumeric() || c == '_' {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEATURES: &str = "# T\n\n## How to use this file\n\n- ✅ **X-1 not an item**\n\n\
                            ## Wire format (`shrink_wrap`, `SW`)\n\n\
                            - ✅ **SW-1 Structs and enums**, with `ww_repr`.\n  - ✅ **SW-9 nested, not an item**\n\
                            - 🧪 **SW-2 On hardware**: text\n- 🚧 **SW-3 Half done** · `v0.5` — text\n\
                            - 📋 **SW-4 Planned**\n\n## Documentation (`DOC`)\n\n- ✅ **DOC-1 Docs site**\n\n\
                            ## Dropped and superseded\n\n- **Old thing** — gone\n";

    const ENTRIES: &str = "How this file works.\n\n# Wire format\n\nWhat it is for.\nSecond line.\n\n\
                           ## Basic\n\n- SW-1 | Structs | Your types. | serdes/derive.md\n\n\
                           ## Advanced\n\n- SW-2, SW-3 | Two | Both. |\n\n# Omitted\n\n- DOC-1 | This site.\n";

    #[test]
    fn renders_areas_levels_and_status() {
        let page = render(FEATURES, ENTRIES).unwrap();
        assert!(
            page.contains("\n## Wire format\n\nWhat it is for.\nSecond line.\n"),
            "{page}"
        );
        assert!(page.contains("ww-level--basic"), "{page}");
        assert!(
            page.contains("- **[Structs](serdes/derive.md)**: Your types.\n"),
            "{page}"
        );
        // The worst status of an entry's items wins.
        assert!(
            page.contains("- **Two**: Both. <span class=\"ww-status\">in progress</span>\n"),
            "{page}"
        );
        assert!(page.contains(
            "[Wire format](features.md#wire-format-shrink_wrap-sw) in the feature tracker"
        ));
        assert!(!page.contains("Documentation"), "{page}");
        assert!(page.find("ww-level--basic") < page.find("ww-level--advanced"));
    }

    #[test]
    fn missing_and_stale_entries() {
        let entries = ENTRIES.replace("- SW-2, SW-3", "- SW-3, SW-4");
        let err = render(FEATURES, &entries).unwrap_err();
        assert!(err.contains("SW-2 On hardware has no line"), "{err}");
        assert!(
            err.contains("SW-4 Planned is not done or in progress"),
            "{err}"
        );

        let entries = ENTRIES.replace("- DOC-1 | This site.", "- DOC-1, SW-1 | This site.");
        let err = render(FEATURES, &entries).unwrap_err();
        assert!(err.contains("SW-1 is already on line 10"), "{err}");

        let entries = ENTRIES.replace("SW-1 |", "SW-7 |");
        let err = render(FEATURES, &entries).unwrap_err();
        assert!(err.contains("SW-7 is not in FEATURES.md"), "{err}");
    }

    #[test]
    fn malformed_entries() {
        let err = render(FEATURES, &ENTRIES.replace("## Advanced", "## Expert")).unwrap_err();
        assert!(err.contains("`Expert` is not a level"), "{err}");
        let err = render(FEATURES, &ENTRIES.replace(" | serdes/derive.md", "")).unwrap_err();
        assert!(err.contains("got 3 parts"), "{err}");
        let err = render(FEATURES, &ENTRIES.replace("## Advanced", "## Basic")).unwrap_err();
        assert!(err.contains("each at most once"), "{err}");
    }

    #[test]
    fn links_without_anchors() {
        let entries = ENTRIES.replace("serdes/derive.md", "serdes/derive.md#quick-start");
        assert_eq!(links(&entries), ["serdes/derive.md"]);
    }

    #[test]
    fn ids() {
        assert!(is_id("SW-1"));
        assert!(is_id("TEST-17"));
        assert!(!is_id("Sub-type"));
        assert!(!is_id("SW-"));
    }
}
