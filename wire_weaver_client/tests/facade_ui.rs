//! What `facade!` and `facade_line!` accept, and the compile errors for what they don't (CLT-64): every operation
//! a line doesn't write and no `#[unsupported]` names for it is a compile error.

/// trybuild builds `wire_weaver_client` again in its own target directory, minutes on a clean checkout: more than
/// nextest's hang limit, so this runs through `just test-facade-ui` (part of `just test`) with plain `cargo test`.
#[test]
#[ignore = "run by `just test-facade-ui`"]
fn facade_ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass.rs");
    t.compile_fail("tests/ui/[!p]*.rs");
}
