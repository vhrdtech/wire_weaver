//! Wire-format conformance matrix (TEST-23, `docs/serdes/conformance.md`): one corpus of cases (`corpus.json`: a
//! type, a value, the golden bytes), sent through every consumer of the wire format: the Rust derive, the `ww_self`
//! dynamic client, Python (`wire_weaver_py/tests/test_conformance.py`) and C (the runtime and the generated
//! fixed-layout readers). A case a consumer can't express yet carries a gap mark with the reason; the tests fail
//! when a mark is missing or no longer needed, so nothing is skipped silently.
//!
//! `just conformance-update` (`CONFORMANCE=update` for the `corpus` and `report` tests) fills in the golden bytes of
//! new cases (from the derive) and writes the report, `docs/serdes/conformance_matrix.md`.

pub mod c;
pub mod corpus;
pub mod derive;
pub mod dynamic;
pub mod json;
pub mod report;

use corpus::{Case, Direction};

/// What a consumer made of a case.
#[derive(Debug)]
pub enum Outcome {
    Pass,
    /// Marked as a gap, and not passing.
    Gap(String),
}

/// A decode of the golden bytes: `Ok(Ok(()))` equal to the case's value, `Ok(Err(..))` another value, `Err(..)` a
/// decode error.
pub type DecodeResult = Result<Result<(), String>, String>;

/// Encoding and decoding as one consumer does it, or `None` for a case it can't run at all.
pub trait Consumer {
    const NAME: &'static str;
    /// The case's value encoded, or why not.
    fn encode(&self, case: &Case) -> Option<Result<Vec<u8>, String>>;
    /// The golden bytes decoded.
    fn decode(&self, case: &Case) -> Option<DecodeResult>;
    /// Further checks of a passing `both` case (the derive's borrowed types).
    fn extra(&self, _case: &Case) -> Result<(), String> {
        Ok(())
    }
}

/// What a consumer could do with a case.
enum Attempt {
    /// Nothing: it has no form for the type.
    None,
    /// Every half the direction asks for.
    Full(Result<(), String>),
    /// Some of them (a C reader without a writer): the gap mark says what is missing.
    Partial(Result<(), String>),
}

/// The case through `consumer`, by its direction: `Err` names what is wrong, a missing or stale gap mark too.
pub fn run<C: Consumer>(consumer: &C, case: &Case) -> Result<Outcome, String> {
    let gap = case.gap(C::NAME);
    match (gap, attempt(consumer, case)) {
        (Some(gap), Attempt::None | Attempt::Full(Err(_)) | Attempt::Partial(Ok(()))) => {
            Ok(Outcome::Gap(gap.into()))
        }
        (Some(gap), Attempt::Full(Ok(()))) => Err(format!(
            "passes now: remove its gap mark ({gap}) and close what it names"
        )),
        (None, Attempt::None) => Err("can't be run: give it a gap mark with the reason".into()),
        (None, Attempt::Partial(Ok(()))) => {
            Err("runs only in part: give it a gap mark naming what is missing".into())
        }
        (None, Attempt::Full(Ok(()))) => Ok(Outcome::Pass),
        (_, Attempt::Partial(Err(e))) => Err(format!("(the part that runs) {e}")),
        (None, Attempt::Full(Err(e))) => Err(e),
    }
}

fn attempt<C: Consumer>(consumer: &C, case: &Case) -> Attempt {
    let golden = case.golden();
    let encode = || -> Option<Result<(), String>> {
        Some(match consumer.encode(case)? {
            Ok(bytes) if bytes == golden => Ok(()),
            Ok(bytes) => Err(format!(
                "encode: {}",
                corpus::bytes_mismatch(&bytes, &golden)
            )),
            Err(e) => Err(format!("encode failed: {e}")),
        })
    };
    let decode = || -> Option<Result<(), String>> {
        Some(match consumer.decode(case)? {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(format!("decode: {e}")),
            Err(e) => Err(format!("decode failed: {e}")),
        })
    };
    let full = |r: Option<Result<(), String>>| r.map_or(Attempt::None, Attempt::Full);
    match case.direction {
        Direction::Both => match (encode(), decode()) {
            (Some(e), Some(d)) => Attempt::Full(e.and(d).and_then(|()| consumer.extra(case))),
            (Some(half), None) | (None, Some(half)) => Attempt::Partial(half),
            (None, None) => Attempt::None,
        },
        Direction::Decode => full(decode()),
        Direction::Reject => full(consumer.decode(case).map(|r| match r {
            Err(_) => Ok(()),
            Ok(_) => Err("decoded bytes that must be rejected".into()),
        })),
        Direction::RejectValue => full(consumer.encode(case).map(|r| match r {
            Err(_) => Ok(()),
            Ok(bytes) => Err(format!(
                "encoded a value that must be rejected: {}",
                hex::encode(bytes)
            )),
        })),
    }
}

/// Every case through `consumer`; panics with every failure, one line each.
pub fn run_all<C: Consumer>(consumer: &C) -> Vec<(String, Outcome)> {
    let corpus = corpus::load();
    let mut failures = vec![];
    let mut outcomes = vec![];
    for case in &corpus.cases {
        match run(consumer, case) {
            Ok(o) => outcomes.push((case.name.clone(), o)),
            Err(e) => failures.push(format!("case '{}', consumer {}: {e}", case.name, C::NAME)),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases fail:\n{}",
        failures.len(),
        corpus.cases.len(),
        failures.join("\n")
    );
    outcomes
}
