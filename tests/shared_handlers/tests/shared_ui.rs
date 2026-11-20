//! A shared item whose handler takes `&mut self` doesn't compile (SRV-65): the `&self` dispatch can't call it.

/// trybuild builds the crates again in its own target directory, minutes on a clean checkout: more than nextest's
/// hang limit, so this runs through `just test-shared-ui` (part of `just test`) with plain `cargo test`.
#[test]
#[ignore = "run by `just test-shared-ui`"]
fn shared_ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
