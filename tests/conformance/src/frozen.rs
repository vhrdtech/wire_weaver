//! TEST-24: once a case's golden bytes land, they are sealed (`docs/serdes/conformance.md`). This reads the corpus
//! as it was at the baseline commit and fails when a case that existed there changed its `bytes`, `direction` or
//! `bytes_from`, or disappeared, in the corpus on disk now — a hand edit or a reset to `"?"` re-seals silently
//! otherwise, `CONFORMANCE=update` alone only refuses to overwrite a filled-in case.
//!
//! The baseline is `CONFORMANCE_BASE=<sha>` when set to a real commit (CI sets it to the pushed range's `before`
//! on a push to master, since `git merge-base HEAD origin/master` is `HEAD` itself right after the push, which
//! would compare the corpus with itself); otherwise `git merge-base HEAD origin/master`, falling back to local
//! `master` (a pull request, or a local run). `CONFORMANCE_BASE` unset or empty falls through to that default;
//! set to the all-zero sentinel (a push with no prior commit, a new branch) means there is nothing to compare
//! against at all.
//!
//! The baseline corpus is parsed with [`LenientCase`], not the current [`crate::corpus::Case`] schema: a field
//! added, renamed or made required since the baseline would otherwise make `serde_json` reject the whole file,
//! silently turning this guard off exactly when the corpus is being reshaped (and a bytes change could hide in
//! the same commit). Only `name`, `bytes`, `direction`, `bytes_from` and `wire_change` are read; anything else is
//! ignored, in either direction. A deliberate wire break still has a way through: mark the changed case with
//! `"wire_change": "EVO-n / reason"`, which the guard accepts once — the mark must be new or changed from what it
//! was at the baseline, or a reused mark would exempt the case from this guard forever instead of for one change.
//!
//! Depends only on the repository's own git history, no network: without git, or without `tests/conformance/
//! corpus.json` existing yet at the baseline (a shallow clone, or the corpus being introduced for the first
//! time), the guard says so and passes rather than failing a check it can't actually run. A baseline commit that
//! exists but whose `tests/conformance/corpus.json` isn't recognizable as a corpus at all (not JSON, or no
//! `cases` array) is different: that points at something broken in the repository's history, not at a guard with
//! nothing to do, so it fails loudly instead. CI must fetch enough history (`fetch-depth: 0`) for the check to
//! mean anything there.

use crate::corpus::{Corpus, Direction};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Relative to the repository root, same path [`crate::corpus::path`] reads from disk.
const REL_PATH: &str = "tests/conformance/corpus.json";

/// A corpus case read from a past commit: only the fields this guard compares, everything else ignored, so a
/// schema change doesn't make a past corpus unparsable (see the module doc).
#[derive(Debug, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct LenientCase {
    pub name: String,
    #[serde(default)]
    pub bytes: String,
    #[serde(default)]
    pub direction: Direction,
    #[serde(default)]
    pub bytes_from: Option<String>,
    #[serde(default)]
    pub wire_change: Option<String>,
}

/// A corpus read from a past commit. `cases` is required (unlike [`LenientCase`]'s own fields): a JSON object
/// without one isn't a corpus at all, see [`Baseline::Corrupt`].
#[derive(Debug, Deserialize, Default)]
pub struct LenientCorpus {
    pub cases: Vec<LenientCase>,
    #[serde(default)]
    pub requantize: Vec<LenientRequantize>,
}

/// A requantize case (STD-63) of a past commit: its result is as frozen as a case's golden bytes.
#[derive(Debug, Deserialize)]
pub struct LenientRequantize {
    pub name: String,
    #[serde(default)]
    pub result: serde_json::Value,
    #[serde(default)]
    pub wire_change: Option<String>,
}

/// What to compare the corpus on disk against, or why there's nothing to compare.
pub enum Baseline {
    At {
        sha: String,
        corpus: LenientCorpus,
    },
    /// No git, no base commit resolves, or the corpus doesn't exist yet there: nothing to compare, not a failure.
    Unavailable(String),
    /// The base commit exists but `tests/conformance/corpus.json` there isn't recognizable as a corpus at all: a
    /// failure, since this points at something broken rather than at nothing to check.
    Corrupt(String),
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("tests/conformance is two levels under the repository root")
        .to_path_buf()
}

/// `None` when git isn't there, the ref doesn't resolve, or the command otherwise fails; never a network call.
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo_root())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_string())
}

/// `CONFORMANCE_BASE`: `None` when unset or empty (not applicable to this CI event, fall through to the default
/// merge-base lookup), `Some(None)` when set to the all-zero sentinel or a commit that doesn't resolve (named but
/// unusable: this guard must not silently fall back to the default lookup, which is exactly the comparison this
/// variable exists to avoid on a push to master), `Some(Some(sha))` when it resolves.
fn explicit_base() -> Option<Option<String>> {
    let value = std::env::var("CONFORMANCE_BASE").ok()?;
    let sha = value.trim();
    if sha.is_empty() {
        return None;
    }
    if sha.chars().all(|c| c == '0') {
        return Some(None);
    }
    Some(git(&[
        "rev-parse",
        "--verify",
        &format!("{sha}^{{commit}}"),
    ]))
}

/// `Err` names why there is no baseline commit to use.
fn merge_base() -> Result<String, String> {
    if git(&["rev-parse", "--is-inside-work-tree"]).is_none() {
        return Err("no git".into());
    }
    if let Some(explicit) = explicit_base() {
        return explicit.ok_or_else(|| {
            "CONFORMANCE_BASE is set but names no usable commit (empty history, or it doesn't resolve)".into()
        });
    }
    ["origin/master", "master"]
        .into_iter()
        .find_map(|base| {
            git(&["rev-parse", "--verify", base])?;
            git(&["merge-base", "HEAD", base])
        })
        .ok_or_else(|| {
            "neither origin/master nor master resolves to a commit (a shallow clone?)".into()
        })
}

/// The corpus as it was at the baseline commit, read straight from git (never from the working tree).
pub fn baseline() -> Baseline {
    let sha = match merge_base() {
        Ok(sha) => sha,
        Err(reason) => {
            return Baseline::Unavailable(format!(
                "{reason}: skipping the frozen golden bytes guard (TEST-24)"
            ));
        }
    };
    let Some(text) = git(&["show", &format!("{sha}:{REL_PATH}")]) else {
        return Baseline::Unavailable(format!(
            "{REL_PATH} doesn't exist at {sha} (the baseline commit): skipping the frozen golden bytes guard \
             (TEST-24), nothing to compare yet"
        ));
    };
    match serde_json::from_str::<LenientCorpus>(&text) {
        Ok(corpus) => Baseline::At { sha, corpus },
        Err(e) => Baseline::Corrupt(format!(
            "{REL_PATH} at {sha} (the baseline commit) doesn't parse as a corpus at all ({e}): the frozen golden \
             bytes guard (TEST-24) can't tell whether it changed, which is a failure, not nothing to check"
        )),
    }
}

/// One failure line per sealed case that changed or disappeared since `old` (the baseline) in `now` (the corpus
/// on disk), empty when the guard is satisfied.
pub fn check(old: &LenientCorpus, now: &Corpus, sha: &str) -> Vec<String> {
    let mut failures = vec![];
    for was in &old.cases {
        let Some(is) = now.cases.iter().find(|c| c.name == was.name) else {
            failures.push(format!(
                "case '{}' existed at {sha} (baseline) and is gone now: golden bytes and cases don't change or \
                 disappear once in without the escape (docs/serdes/conformance.md)",
                was.name
            ));
            continue;
        };
        let fresh_escape = is
            .wire_change
            .as_deref()
            .is_some_and(|w| Some(w) != was.wire_change.as_deref());
        if fresh_escape {
            continue;
        }
        if is.bytes != was.bytes {
            failures.push(format!(
                "case '{}': bytes changed since {sha} (baseline): old '{}', new '{}'. Golden bytes don't change \
                 once in (docs/serdes/conformance.md); mark it \"wire_change\": \"EVO-n / reason\" (a new or \
                 changed reason, a reused one doesn't count) with the matching EVO item and a **Breaking:** \
                 CHANGELOG entry if this is a deliberate wire change",
                was.name, was.bytes, is.bytes
            ));
        }
        if is.direction != was.direction {
            failures.push(format!(
                "case '{}': direction changed since {sha} (baseline): old {:?}, new {:?}. Mark it \"wire_change\": \
                 \"EVO-n / reason\" (new or changed) if this is deliberate",
                was.name, was.direction, is.direction
            ));
        }
        if is.bytes_from != was.bytes_from {
            failures.push(format!(
                "case '{}': bytes_from changed since {sha} (baseline): old {:?}, new {:?}. Mark it \
                 \"wire_change\": \"EVO-n / reason\" (new or changed) if this is deliberate",
                was.name, was.bytes_from, is.bytes_from
            ));
        }
    }
    for was in &old.requantize {
        let Some(is) = now.requantize.iter().find(|c| c.name == was.name) else {
            failures.push(format!(
                "requantize case '{}' existed at {sha} (baseline) and is gone now: cases don't disappear once in",
                was.name
            ));
            continue;
        };
        let fresh_escape = is
            .wire_change
            .as_deref()
            .is_some_and(|w| Some(w) != was.wire_change.as_deref());
        if is.result != was.result && !fresh_escape {
            failures.push(format!(
                "requantize case '{}': result changed since {sha} (baseline): old {}, new {}. A conversion's \
                 result doesn't change once in; mark it \"wire_change\": \"STD-n / reason\" (new or changed) \
                 with a **Breaking:** CHANGELOG entry if this is deliberate",
                was.name, was.result, is.result
            ));
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::Case;

    fn lenient(name: &str, bytes: &str, wire_change: Option<&str>) -> LenientCase {
        LenientCase {
            name: name.into(),
            bytes: bytes.into(),
            direction: Direction::Both,
            bytes_from: None,
            wire_change: wire_change.map(String::from),
        }
    }

    fn case(name: &str, bytes: &str, wire_change: Option<&str>) -> Case {
        Case {
            name: name.into(),
            ty: "u8".into(),
            value: serde_json::Value::Null,
            bytes: bytes.into(),
            bytes_from: None,
            direction: Direction::Both,
            gaps: Default::default(),
            note: None,
            wire_change: wire_change.map(String::from),
        }
    }

    #[test]
    fn changed_bytes_fail() {
        let old = LenientCorpus {
            requantize: vec![],
            cases: vec![lenient("x", "00", None)],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "ff", None)],
            requantize: vec![],
        };
        let failures = check(&old, &now, "deadbeef");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("bytes changed"));
    }

    #[test]
    fn unchanged_bytes_pass() {
        let old = LenientCorpus {
            requantize: vec![],
            cases: vec![lenient("x", "00", None)],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "00", None)],
            requantize: vec![],
        };
        assert!(check(&old, &now, "deadbeef").is_empty());
    }

    #[test]
    fn disappeared_case_fails() {
        let old = LenientCorpus {
            requantize: vec![],
            cases: vec![lenient("gone", "00", None)],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![],
            requantize: vec![],
        };
        let failures = check(&old, &now, "deadbeef");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("gone") && failures[0].contains("is gone now"));
    }

    #[test]
    fn reused_wire_change_still_fails() {
        // the escape was already used for an earlier change (same mark at the baseline as now): a further bytes
        // change behind the same mark must not be exempt a second time.
        let old = LenientCorpus {
            requantize: vec![],
            cases: vec![lenient("x", "00", Some("EVO-1 / first change"))],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "ff", Some("EVO-1 / first change"))],
            requantize: vec![],
        };
        let failures = check(&old, &now, "deadbeef");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("bytes changed"));
    }

    #[test]
    fn new_wire_change_passes() {
        let old = LenientCorpus {
            requantize: vec![],
            cases: vec![lenient("x", "00", None)],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "ff", Some("EVO-2 / deliberate"))],
            requantize: vec![],
        };
        assert!(check(&old, &now, "deadbeef").is_empty());
    }

    #[test]
    fn changed_wire_change_reason_passes_again() {
        // a second, different deliberate change: a changed reason counts as fresh, same as a new one.
        let old = LenientCorpus {
            requantize: vec![],
            cases: vec![lenient("x", "ff", Some("EVO-2 / first change"))],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "00", Some("EVO-3 / second change"))],
            requantize: vec![],
        };
        assert!(check(&old, &now, "deadbeef").is_empty());
    }

    /// A past corpus whose schema differs from the current one (a field this guard doesn't know about, here
    /// standing in for one added or renamed since) still parses with `LenientCase` and still catches a bytes
    /// change: the guard doesn't switch itself off across a schema change.
    #[test]
    fn old_schema_baseline_still_checked() {
        let old_schema = r#"{
            "cases": [
                {"name": "x", "type": "u8", "value": 0, "bytes": "00", "obsolete_field_no_longer_read": 123}
            ]
        }"#;
        let old: LenientCorpus =
            serde_json::from_str(old_schema).expect("lenient schema parses old shape");
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "ff", None)],
            requantize: vec![],
        };
        let failures = check(&old, &now, "deadbeef");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("bytes changed"));
    }

    /// Not a corpus at all (no `cases`): the lenient parse itself must fail, so `baseline()` reports `Corrupt`
    /// (a loud failure) rather than treating it as nothing to compare against.
    #[test]
    fn not_a_corpus_fails_to_parse() {
        assert!(serde_json::from_str::<LenientCorpus>(r#"{"foo": 1}"#).is_err());
        assert!(serde_json::from_str::<LenientCorpus>("not json at all").is_err());
    }
}
