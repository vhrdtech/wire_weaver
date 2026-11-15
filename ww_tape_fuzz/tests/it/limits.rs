//! The case runner: an allocation over the cap and a panic are outcomes of the case, not of the test.

use ww_tape_fuzz::case::{ALLOC_MAX, Outcome, run};

fn allocates_over_the_cap(_: &[u8]) {
    let v: Vec<u8> = Vec::with_capacity(ALLOC_MAX + 1);
    std::hint::black_box(&v);
}

fn allocates_at_the_cap(_: &[u8]) {
    let v: Vec<u8> = Vec::with_capacity(ALLOC_MAX);
    std::hint::black_box(&v);
}

fn panics(data: &[u8]) {
    assert!(data.is_empty(), "a case that panics");
}

#[test]
fn an_allocation_over_the_cap_is_the_case_outcome() {
    assert_eq!(
        run(allocates_over_the_cap, &[]).0,
        Outcome::Alloc(ALLOC_MAX + 1)
    );
    assert_eq!(run(allocates_at_the_cap, &[]).0, Outcome::Ok);
    // the next case starts clean
    assert_eq!(run(panics, &[]).0, Outcome::Ok);
}

#[test]
fn a_panic_is_the_case_outcome_with_its_place() {
    let Outcome::Panic(at, msg) = run(panics, &[1]).0 else {
        panic!("the case panics");
    };
    assert!(at.starts_with("ww_tape_fuzz/tests/it/limits.rs:"), "{at}");
    assert_eq!(msg, "a case that panics");
}
