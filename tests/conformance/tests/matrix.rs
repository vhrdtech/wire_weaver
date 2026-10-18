//! The conformance matrix (TEST-23): every case of `corpus.json` through each consumer, one test per consumer.
//! Python runs the same corpus in `wire_weaver_py/tests/test_conformance.py`.

use conformance::corpus::{self, CONSUMERS, Direction};
use conformance::derive::Derive;
use conformance::dynamic::Dynamic;
use std::collections::BTreeSet;

fn updating() -> bool {
    std::env::var_os("CONFORMANCE").is_some_and(|v| v == "update")
}

/// The corpus is well formed and the derive has a value for every case (it is the reference: no gaps).
#[test]
fn corpus() {
    let derive = Derive::default();
    if updating() {
        corpus::fill_in_bytes(&derive);
    }
    let corpus = corpus::load();
    let mut names = BTreeSet::new();
    for case in &corpus.cases {
        assert!(
            names.insert(case.name.as_str()),
            "case '{}' twice",
            case.name
        );
        for consumer in case.gaps.keys() {
            assert!(
                CONSUMERS.contains(&consumer.as_str()) && consumer != "derive",
                "case '{}': gap of '{consumer}', not a consumer that can have gaps",
                case.name
            );
        }
        match case.direction {
            Direction::RejectValue => assert!(
                case.bytes.is_empty(),
                "case '{}': bytes of a value that doesn't encode",
                case.name
            ),
            _ => {
                case.golden();
            }
        }
        if case.direction == Direction::Reject {
            assert!(
                case.value.is_null(),
                "case '{}': a rejected case has value null",
                case.name
            );
        }
    }
    let typed: BTreeSet<&str> = derive.names().collect();
    let missing: Vec<_> = names.difference(&typed).collect();
    let extra: Vec<_> = typed.difference(&names).collect();
    assert!(
        missing.is_empty(),
        "cases without a value in derive.rs: {missing:?}"
    );
    assert!(
        extra.is_empty(),
        "derive.rs values without a case in corpus.json: {extra:?}"
    );
}

#[test]
fn derive() {
    conformance::run_all(&Derive::default());
}

#[test]
fn dynamic() {
    conformance::run_all(&Dynamic);
}

#[test]
fn c() {
    conformance::run_all(&conformance::c::C);
}

/// TEST-24: a case's `bytes`, `direction` and `bytes_from` don't change or disappear once in, since the baseline
/// commit (`conformance::frozen`); skips with a printed reason instead of failing when there's nothing to compare
/// against (no git, a shallow clone, or the corpus being new), but fails loudly when the baseline commit's corpus
/// isn't recognizable as one at all.
#[test]
fn frozen_bytes() {
    use conformance::frozen::Baseline;
    match conformance::frozen::baseline() {
        Baseline::Unavailable(reason) => println!("{reason}"),
        Baseline::Corrupt(reason) => panic!("{reason}"),
        Baseline::At { sha, corpus: old } => {
            let now = corpus::load();
            let failures = conformance::frozen::check(&old, &now, &sha);
            assert!(failures.is_empty(), "{}", failures.join("\n"));
        }
    }
}

/// TEST-25: every wire-relevant `ww_self::Type` variant and `shrink_wrap_derive` directive is exercised by some
/// case, or carries a gap mark naming why not (`conformance::coverage`).
#[test]
fn coverage() {
    use conformance::coverage::{self, ALL_TYPE_KINDS, DIRECTIVE_GAPS, TYPE_GAPS};
    use std::collections::BTreeSet;

    let corpus = corpus::load();
    let all_types: BTreeSet<&str> = ALL_TYPE_KINDS.iter().copied().collect();
    let mut failures = coverage::check(&all_types, &coverage::used_type_kinds(&corpus), TYPE_GAPS);
    failures.extend(coverage::check(
        &coverage::all_directive_kinds(),
        &coverage::used_directive_kinds(),
        DIRECTIVE_GAPS,
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `docs/serdes/conformance_matrix.md` is what the corpus says.
#[test]
fn report() {
    let report = conformance::report::render(&corpus::load());
    let path = conformance::report::path();
    if updating() {
        std::fs::write(&path, &report).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap_or_default(),
        report,
        "docs/serdes/conformance_matrix.md is not what the corpus says: just conformance-update"
    );
}
