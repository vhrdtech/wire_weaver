//! TEST-24-style guard for this corpus: once a case's golden bytes land, they are sealed. This reads `corpus.json`
//! as it was at the baseline commit and fails when a step that existed there changed its `request`, `reply` or
//! `events`, or a case disappeared, in the corpus on disk now — a hand edit re-seals silently otherwise,
//! `DISPATCH_GOLDEN=update` alone only refuses to overwrite a filled-in step. See `tests/conformance/src/frozen.rs`,
//! which this mirrors one to one for its own corpus; `docs/serdes/conformance.md` has the baseline-resolution and
//! escape rules in full (`CONFORMANCE_BASE` there, `DISPATCH_GOLDEN_BASE` here).

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Relative to the repository root, same path [`crate::corpus::path`] reads from disk.
const REL_PATH: &str = "tests/dispatch_golden/corpus.json";

/// A step read from a past commit: only the fields this guard compares, everything else ignored, so a schema
/// change doesn't make a past corpus unparsable.
#[derive(Debug, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct LenientStep {
    #[serde(default)]
    pub request: String,
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub events: Vec<String>,
}

/// A corpus case read from a past commit.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct LenientCase {
    pub name: String,
    #[serde(default)]
    pub steps: Vec<LenientStep>,
    #[serde(default)]
    pub wire_change: Option<String>,
}

/// A corpus read from a past commit. `cases` is required, unlike its own fields: a JSON object without one isn't
/// a corpus at all, see [`Baseline::Corrupt`].
#[derive(Debug, Deserialize, Default)]
pub struct LenientCorpus {
    pub cases: Vec<LenientCase>,
}

/// What to compare the corpus on disk against, or why there's nothing to compare.
pub enum Baseline {
    At {
        sha: String,
        corpus: LenientCorpus,
    },
    /// No git, no base commit resolves, or the corpus doesn't exist yet there: nothing to compare, not a failure.
    Unavailable(String),
    /// The base commit exists but the corpus there isn't recognizable as one at all: a failure, since this points
    /// at something broken rather than at nothing to check.
    Corrupt(String),
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("tests/dispatch_golden is two levels under the repository root")
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

/// `DISPATCH_GOLDEN_BASE`: same rules as `CONFORMANCE_BASE` (see `tests/conformance/src/frozen.rs`).
fn explicit_base() -> Option<Option<String>> {
    let value = std::env::var("DISPATCH_GOLDEN_BASE").ok()?;
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

fn merge_base() -> Result<String, String> {
    if git(&["rev-parse", "--is-inside-work-tree"]).is_none() {
        return Err("no git".into());
    }
    if let Some(explicit) = explicit_base() {
        return explicit.ok_or_else(|| {
            "DISPATCH_GOLDEN_BASE is set but names no usable commit (empty history, or it doesn't resolve)".into()
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
                "{reason}: skipping the frozen golden bytes guard (TEST-24-style, dispatch_golden)"
            ));
        }
    };
    let Some(text) = git(&["show", &format!("{sha}:{REL_PATH}")]) else {
        return Baseline::Unavailable(format!(
            "{REL_PATH} doesn't exist at {sha} (the baseline commit): skipping the frozen golden bytes guard, \
             nothing to compare yet"
        ));
    };
    match serde_json::from_str::<LenientCorpus>(&text) {
        Ok(corpus) => Baseline::At { sha, corpus },
        Err(e) => Baseline::Corrupt(format!(
            "{REL_PATH} at {sha} (the baseline commit) doesn't parse as a corpus at all ({e}): the guard can't \
             tell whether it changed, which is a failure, not nothing to check"
        )),
    }
}

/// One failure line per sealed step that changed or disappeared since `old` (the baseline) in `now` (the corpus on
/// disk), empty when the guard is satisfied.
pub fn check(old: &LenientCorpus, now: &crate::corpus::Corpus, sha: &str) -> Vec<String> {
    let mut failures = vec![];
    for was in &old.cases {
        let Some(is) = now.cases.iter().find(|c| c.name == was.name) else {
            failures.push(format!(
                "case '{}' existed at {sha} (baseline) and is gone now: golden bytes and cases don't change or \
                 disappear once in without the escape (docs/serdes/conformance.md, \"wire_change\")",
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
        if was.steps.len() > is.steps.len() {
            failures.push(format!(
                "case '{}': had {} step(s) at {sha} (baseline), has {} now: steps don't disappear once in",
                was.name,
                was.steps.len(),
                is.steps.len()
            ));
            continue;
        }
        for (i, (was_step, is_step)) in was.steps.iter().zip(&is.steps).enumerate() {
            if was_step.request != is_step.request {
                failures.push(format!(
                    "case '{}' step {i}: request changed since {sha} (baseline): old '{}', new '{}'. Mark it \
                     \"wire_change\": \"EVO-n / reason\" (new or changed) if this is deliberate",
                    was.name, was_step.request, is_step.request
                ));
            }
            if was_step.reply != is_step.reply {
                failures.push(format!(
                    "case '{}' step {i}: reply changed since {sha} (baseline): old '{}', new '{}'. Golden bytes \
                     don't change once in; mark it \"wire_change\": \"EVO-n / reason\" (new or changed) with the \
                     matching EVO item and a **Breaking:** CHANGELOG entry if this is a deliberate wire change",
                    was.name, was_step.reply, is_step.reply
                ));
            }
            if was_step.events != is_step.events {
                failures.push(format!(
                    "case '{}' step {i}: events changed since {sha} (baseline): old {:?}, new {:?}. Mark it \
                     \"wire_change\": \"EVO-n / reason\" (new or changed) if this is deliberate",
                    was.name, was_step.events, is_step.events
                ));
            }
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{Case, Corpus, Step};

    fn lenient_step(request: &str, reply: &str) -> LenientStep {
        LenientStep {
            request: request.into(),
            reply: reply.into(),
            events: vec![],
        }
    }

    fn step(request: &str, reply: &str) -> Step {
        Step {
            request: request.into(),
            reply: reply.into(),
            events: vec![],
        }
    }

    fn lenient(name: &str, request: &str, reply: &str, wire_change: Option<&str>) -> LenientCase {
        LenientCase {
            name: name.into(),
            steps: vec![lenient_step(request, reply)],
            wire_change: wire_change.map(String::from),
        }
    }

    fn case(name: &str, request: &str, reply: &str, wire_change: Option<&str>) -> Case {
        Case {
            name: name.into(),
            steps: vec![step(request, reply)],
            note: None,
            wire_change: wire_change.map(String::from),
        }
    }

    #[test]
    fn changed_reply_fails() {
        let old = LenientCorpus {
            cases: vec![lenient("x", "aa", "00", None)],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "aa", "ff", None)],
        };
        let failures = check(&old, &now, "deadbeef");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("reply changed"));
    }

    #[test]
    fn unchanged_passes() {
        let old = LenientCorpus {
            cases: vec![lenient("x", "aa", "00", None)],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "aa", "00", None)],
        };
        assert!(check(&old, &now, "deadbeef").is_empty());
    }

    #[test]
    fn disappeared_case_fails() {
        let old = LenientCorpus {
            cases: vec![lenient("gone", "aa", "00", None)],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![],
        };
        let failures = check(&old, &now, "deadbeef");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("gone") && failures[0].contains("is gone now"));
    }

    #[test]
    fn reused_wire_change_still_fails() {
        let old = LenientCorpus {
            cases: vec![lenient("x", "aa", "00", Some("EVO-1 / first change"))],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "aa", "ff", Some("EVO-1 / first change"))],
        };
        let failures = check(&old, &now, "deadbeef");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("reply changed"));
    }

    #[test]
    fn new_wire_change_passes() {
        let old = LenientCorpus {
            cases: vec![lenient("x", "aa", "00", None)],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "aa", "ff", Some("EVO-2 / deliberate"))],
        };
        assert!(check(&old, &now, "deadbeef").is_empty());
    }

    #[test]
    fn fewer_steps_fails() {
        let old = LenientCorpus {
            cases: vec![LenientCase {
                name: "x".into(),
                steps: vec![lenient_step("aa", "00"), lenient_step("bb", "11")],
                wire_change: None,
            }],
        };
        let now = Corpus {
            about: vec![],
            cases: vec![case("x", "aa", "00", None)],
        };
        let failures = check(&old, &now, "deadbeef");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("step(s)"));
    }
}
