//! TEST-24: once a case's golden bytes land, they are sealed (`docs/serdes/conformance.md`). This reads the corpus
//! as it was at the merge base with main (`git merge-base HEAD origin/master`, falling back to local `master`) and
//! fails when a case that existed there changed its `bytes`, `direction` or `bytes_from`, or disappeared, in the
//! corpus on disk now — a hand edit or a reset to `"?"` re-seals silently otherwise, `CONFORMANCE=update` alone
//! only refuses to overwrite a filled-in case.
//!
//! A deliberate wire break still has a way through: mark the case with `"wire_change": "EVO-n / reason"`
//! ([`crate::corpus::Case::wire_change`]), which the guard accepts and the matrix report shows.
//!
//! Depends only on the repository's own git history, no network: without git, or without `tests/conformance/
//! corpus.json` at the merge base (a shallow clone, or the corpus being introduced for the first time), the guard
//! says so and passes rather than failing a check it can't actually run. CI must fetch enough history
//! (`fetch-depth: 0`, or at least the base ref) for the check to mean anything there.

use crate::corpus::Corpus;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Relative to the repository root, same path [`crate::corpus::path`] reads from disk.
const REL_PATH: &str = "tests/conformance/corpus.json";

/// The corpus at the merge base with main, or why there's nothing to compare against.
pub enum Baseline {
    At { sha: String, corpus: Corpus },
    Unavailable(String),
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

fn merge_base() -> Option<String> {
    git(&["rev-parse", "--is-inside-work-tree"])?;
    ["origin/master", "master"].into_iter().find_map(|base| {
        git(&["rev-parse", "--verify", base])?;
        git(&["merge-base", "HEAD", base])
    })
}

/// The corpus as it was at the merge base, read straight from git (never from the working tree).
pub fn baseline() -> Baseline {
    let Some(sha) = merge_base() else {
        return Baseline::Unavailable(
            "no git, or neither origin/master nor master resolves to a commit (a shallow clone?): skipping the \
             frozen golden bytes guard (TEST-24)"
                .into(),
        );
    };
    let Some(text) = git(&["show", &format!("{sha}:{REL_PATH}")]) else {
        return Baseline::Unavailable(format!(
            "{REL_PATH} doesn't exist at {sha} (the merge base with main): skipping the frozen golden bytes guard \
             (TEST-24), nothing to compare yet"
        ));
    };
    match serde_json::from_str::<Corpus>(&text) {
        Ok(corpus) => Baseline::At { sha, corpus },
        Err(e) => Baseline::Unavailable(format!(
            "{REL_PATH} at {sha} doesn't parse with the current corpus schema ({e}): skipping the frozen golden \
             bytes guard (TEST-24)"
        )),
    }
}

/// One failure line per sealed case that changed or disappeared since `old` (the merge base) in `now` (the corpus
/// on disk), empty when the guard is satisfied.
pub fn check(old: &Corpus, now: &Corpus, sha: &str) -> Vec<String> {
    let mut failures = vec![];
    for was in &old.cases {
        let Some(is) = now.cases.iter().find(|c| c.name == was.name) else {
            failures.push(format!(
                "case '{}' existed at {sha} (merge base) and is gone now: golden bytes and cases don't change or \
                 disappear once in without the escape (docs/serdes/conformance.md)",
                was.name
            ));
            continue;
        };
        if is.wire_change.is_some() {
            continue;
        }
        if is.bytes != was.bytes {
            failures.push(format!(
                "case '{}': bytes changed since {sha} (merge base): old '{}', new '{}'. Golden bytes don't change \
                 once in (docs/serdes/conformance.md); mark it \"wire_change\": \"EVO-n / reason\" with the \
                 matching EVO item and a **Breaking:** CHANGELOG entry if this is a deliberate wire change",
                was.name, was.bytes, is.bytes
            ));
        }
        if is.direction != was.direction {
            failures.push(format!(
                "case '{}': direction changed since {sha} (merge base): old {:?}, new {:?}. Mark it \"wire_change\": \
                 \"EVO-n / reason\" if this is deliberate",
                was.name, was.direction, is.direction
            ));
        }
        if is.bytes_from != was.bytes_from {
            failures.push(format!(
                "case '{}': bytes_from changed since {sha} (merge base): old {:?}, new {:?}. Mark it \
                 \"wire_change\": \"EVO-n / reason\" if this is deliberate",
                was.name, was.bytes_from, is.bytes_from
            ));
        }
    }
    failures
}
