//! Renders `docs/features_today.md`, the docs site's short "what works today" page, from the feature tracker
//! `FEATURES.md` (DOC-7): per area the titles of the items that are done, then the ones in progress. The page is a
//! golden file: `tests/features_page.rs` fails when it no longer matches `FEATURES.md`, `just features-page` writes it.

use std::fmt::Write as _;

/// Areas of `FEATURES.md` that are not features.
const SKIPPED_AREAS: [&str; 2] = ["How to use this file", "Dropped and superseded"];

#[derive(Default)]
struct Area {
    /// The heading as written, e.g. ``Wire format (`shrink_wrap`, `SW`)``.
    heading: String,
    done: Vec<String>,
    /// Done, the check on hardware still open (🧪).
    done_untested: Vec<String>,
    in_progress: Vec<String>,
    other: usize,
}

/// The page for `features` (the contents of `FEATURES.md`).
pub fn render(features: &str) -> String {
    let areas = parse(features);
    let mut out = String::new();
    out.push_str(
        "---\nicon: lucide/list-checks\n---\n\n\
         <!-- Generated from FEATURES.md by `just features-page` (tests/features_page), don't edit by hand. -->\n\n\
         # What works today\n\n\
         What WireWeaver can do now, by area: the items of the [feature tracker](features.md) that are done, and the\n\
         ones being worked on. The tracker has the details, what is planned, and for which release.\n",
    );
    for area in &areas {
        let _ = write!(out, "\n## {}\n\n", area_title(&area.heading));
        out.push_str("<div class=\"ww-columns\" markdown>\n\n");
        for title in &area.done {
            let _ = writeln!(out, "- {title}");
        }
        for title in &area.done_untested {
            let _ = writeln!(out, "- {title} (hardware check open)");
        }
        out.push_str("\n</div>\n\n");
        if !area.in_progress.is_empty() {
            let _ = writeln!(out, "**In progress:** {}.\n", area.in_progress.join(", "));
        }
        let total = area.done.len() + area.done_untested.len() + area.in_progress.len() + area.other;
        let _ = writeln!(
            out,
            "[All {total} items of this area in the tracker](features.md#{})",
            slug(&area.heading)
        );
    }
    out
}

fn parse(features: &str) -> Vec<Area> {
    let mut areas: Vec<Area> = Vec::new();
    let mut skip = true;
    for line in features.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            skip = SKIPPED_AREAS.contains(&heading);
            if !skip {
                areas.push(Area {
                    heading: heading.to_string(),
                    ..Default::default()
                });
            }
            continue;
        }
        let (Some(area), false) = (areas.last_mut(), skip) else {
            continue;
        };
        // Top-level items only: `- ✅ **SW-1 Title**, rest`
        let Some(item) = line.strip_prefix("- ") else {
            continue;
        };
        let Some((status, rest)) = item.split_once(' ') else {
            continue;
        };
        let Some(title) = title(rest) else {
            continue;
        };
        match status {
            "✅" => area.done.push(title),
            "🧪" => area.done_untested.push(title),
            "🚧" => area.in_progress.push(title),
            _ => area.other += 1,
        }
    }
    areas
}

/// `**SW-1 Structs, plain enums**, rest` → `Structs, plain enums`: the bold part without the item's ID.
fn title(rest: &str) -> Option<String> {
    let bold = rest.strip_prefix("**")?;
    let (bold, _) = bold.split_once("**")?;
    let title = match bold.split_once(' ') {
        Some((id, title)) if is_id(id) => title,
        _ => bold,
    };
    Some(title.trim().to_string())
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

    #[test]
    fn parses_items_and_areas() {
        let src = "# T\n\n## How to use this file\n\n- ✅ done\n\n## Wire format (`shrink_wrap`, `SW`)\n\n\
                   - ✅ **SW-1 Structs and enums**, with `ww_repr`.\n  - ✅ nested, not an item\n\
                   - 🧪 **SW-2 On hardware**: text\n- 🚧 **SW-3 Half done** · `v0.5` — text\n\
                   - 📋 **SW-4 Planned**\n\n## Dropped and superseded\n\n- **Old thing** — gone\n";
        let page = render(src);
        assert!(page.contains("## Wire format\n"), "{page}");
        assert!(page.contains("- Structs and enums\n"), "{page}");
        assert!(page.contains("- On hardware (hardware check open)\n"), "{page}");
        assert!(page.contains("**In progress:** Half done.\n"), "{page}");
        assert!(page.contains("[All 4 items of this area in the tracker](features.md#wire-format-shrink_wrap-sw)"));
        assert!(!page.contains("How to use"), "{page}");
        assert!(!page.contains("Old thing"), "{page}");
    }

    #[test]
    fn ids() {
        assert!(is_id("SW-1"));
        assert!(is_id("TEST-17"));
        assert!(!is_id("Sub-type"));
        assert!(!is_id("SW-"));
    }
}
