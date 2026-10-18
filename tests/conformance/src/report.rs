//! The report, `docs/serdes/conformance_matrix.md`: every case and what each consumer does with it, from the corpus
//! and its gap marks (the consumer tests check the marks are true, Python's too).

use crate::corpus::{CONSUMERS, Corpus, Direction};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

pub fn path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/serdes/conformance_matrix.md")
}

/// The FEATURES.md id a gap reason names, the last `(XX-n)` in it.
fn gap_id(reason: &str) -> Option<&str> {
    let start = reason.rfind('(')?;
    let id = reason[start + 1..].strip_suffix(')')?;
    id.split_once('-')
        .filter(|(area, n)| {
            area.chars().all(|c| c.is_ascii_uppercase()) && n.chars().all(|c| c.is_ascii_digit())
        })
        .map(|_| id)
}

pub fn render(corpus: &Corpus) -> String {
    let mut out = String::new();
    let n = corpus.cases.len();
    let _ = writeln!(
        out,
        "---\nlevel: nomicon\n---\n\n# Conformance matrix\n\n<!-- Generated from tests/conformance/corpus.json by \
         `just conformance-update`, do not edit. -->\n\nEvery case of the [conformance corpus](conformance.md) and what \
         each consumer does with it: ✅ encodes to the golden bytes and decodes them back to the value (or only \
         decodes them, or rejects them, as the case's direction says); a gap names why the consumer can't run the case \
         yet and the item that fixes it, *reader only* that it decodes but can't encode, *by design* that it \
         behaves otherwise on purpose. The tests fail when a mark is missing or the consumer passes a marked case.\n"
    );
    let _ = writeln!(out, "| Consumer | Cases run | Gaps |\n|---|---|---|");
    for consumer in CONSUMERS {
        let gaps = corpus
            .cases
            .iter()
            .filter(|c| c.gap(consumer).is_some())
            .count();
        let _ = writeln!(out, "| {consumer} | {} of {n} | {gaps} |", n - gaps);
    }
    let _ = writeln!(
        out,
        "\n| Case | Type | Direction | {} |\n|---|---|---|{}",
        CONSUMERS.join(" | "),
        "---|".repeat(CONSUMERS.len())
    );
    for case in &corpus.cases {
        let direction = match case.direction {
            Direction::Both => "both",
            Direction::Decode => "decode",
            Direction::Reject => "reject",
            Direction::RejectValue => "reject value",
        };
        let cells: Vec<String> = CONSUMERS
            .iter()
            .map(|c| match case.gap(c) {
                None => "✅".into(),
                Some(reason) => {
                    let id = gap_id(reason).unwrap_or("-");
                    if reason.starts_with("by design") {
                        format!("by design ({id})")
                    } else if reason.starts_with("reader only") {
                        format!("reader only ({id})")
                    } else {
                        format!("gap {id}")
                    }
                }
            })
            .collect();
        let _ = writeln!(
            out,
            "| `{}` | `{}` | {direction} | {} |",
            case.name,
            case.ty,
            cells.join(" | ")
        );
    }
    // every reason once, with its cases
    let mut reasons: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for case in &corpus.cases {
        for (consumer, reason) in &case.gaps {
            reasons
                .entry((consumer.as_str(), reason.as_str()))
                .or_default()
                .push(&case.name);
        }
    }
    let _ = writeln!(out, "\n## Gaps\n");
    for ((consumer, reason), names) in reasons {
        let _ = writeln!(
            out,
            "- **{consumer}**: {reason}. {} case{}: {}.",
            names.len(),
            if names.len() == 1 { "" } else { "s" },
            names
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    out
}
