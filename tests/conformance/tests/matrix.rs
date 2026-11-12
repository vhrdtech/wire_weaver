//! The conformance matrix (TEST-23): every case of `corpus.json` through each consumer, one test per consumer.
//! Python runs the same corpus in `wire_weaver_py/tests/test_conformance.py`.

use conformance::corpus::{self, CONSUMERS, Direction, REQUANTIZE_CONSUMERS};
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

/// The requantize cases (STD-63) are well formed: unique names, gap marks only of consumers that can have them
/// (Python runs its own in `test_conformance.py`).
#[test]
fn requantize_corpus() {
    let corpus = corpus::load();
    let mut names = BTreeSet::new();
    for case in &corpus.requantize {
        assert!(
            names.insert(case.name.as_str()),
            "requantize case '{}' twice",
            case.name
        );
        for consumer in case.gaps.keys() {
            assert!(
                REQUANTIZE_CONSUMERS.contains(&consumer.as_str()) && consumer != "derive",
                "requantize case '{}': gap of '{consumer}', not a consumer that can have gaps",
                case.name
            );
        }
    }
}

/// The signature cases (EVO-69): unique names, both signatures as `ww_self` calculates them.
#[test]
fn signatures() {
    let corpus = corpus::load();
    let mut names = BTreeSet::new();
    let mut failures = vec![];
    for case in &corpus.signatures {
        assert!(
            names.insert(case.name.as_str()),
            "signature case '{}' twice",
            case.name
        );
        if let Err(e) = conformance::signature::check(case) {
            failures.push(format!("signature case '{}': {e}", case.name));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn requantize_derive() {
    conformance::run_requantize("derive", conformance::requantize::derive);
}

#[test]
fn requantize_dynamic() {
    conformance::run_requantize("dynamic", conformance::requantize::dynamic);
}

#[test]
fn requantize_c() {
    conformance::run_requantize("c", conformance::c::requantize);
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
fn explain() {
    conformance::run_all(&conformance::explain::Explain);
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

/// LVL-6: every codec case's code means its `decoded` number, by the typed codec and by the codec's definition in the
/// model, and that number encodes back to the case's code (or its `encodes_to`).
#[test]
fn codec_cases() {
    let corpus = conformance::corpus::load();
    let mut failures = vec![];
    let mut checked = 0;
    for case in corpus.cases.iter().filter(|c| c.decoded.is_some()) {
        checked += 1;
        for (consumer, run) in [
            ("derive", conformance::codec::derive as fn(&_) -> _),
            ("dynamic", conformance::codec::dynamic),
        ] {
            if let Err(e) = run(case) {
                failures.push(format!("case '{}', {consumer}: {e}", case.name));
            }
        }
    }
    assert!(checked > 0, "no codec cases");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A spare code read from the wire is refused with the code and the codec named, by the definition in the model.
#[test]
fn spare_code_refusal_names_the_code() {
    let ty = conformance::dynamic::property_ty("coded_mu_law_single_zero").unwrap();
    let err = ww_self::ValueOwned::des_shrink_wrap_dyn(&[0x7F], ty, conformance::dynamic::bundle())
        .unwrap_err();
    assert_eq!(
        format!("{err:#}"),
        "127 is a spare code of MuLawSingleZero, no value (exact_units::Coded<MuLawSingleZero, Fixed<Ratio, Pow2<-13>, i16, Unbounded>, u8>)"
    );
}
