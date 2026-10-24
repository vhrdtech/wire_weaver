//! The API compared with its snapshots in `api_snapshots/` (docs/evolution/checker_tool.md): fails when the snapshot
//! of the current version is missing or differs from the source, and when it changed without the version bump the
//! change needs. `WW_API_SAVE=1 cargo test -p blinky_api --test evolution` saves the current version's snapshot.

#[test]
fn api_evolution() {
    wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR"))
        .require_current(true)
        .assert();
}
