//! The API compared with its snapshots in `api_snapshots/` (docs/evolution/checker_tool.md): fails when it changed
//! without the version bump the change needs, or when there is no snapshot to compare with.

#[test]
fn api_evolution() {
    wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR"))
        .strict(true)
        .assert();
}
