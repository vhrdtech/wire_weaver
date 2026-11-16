//! The current source compared with `api_snapshots/wire_line_fixture_api_0_1_0.ron` (EVO-26): fails when the crate
//! changed without the bump it needs. `WW_API_SAVE=1 cargo test -p wire_line_fixture_api --test evolution` saves a
//! new version's snapshot.

#[test]
fn api_evolution() {
    wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR"))
        .require_current(true)
        .strict(true)
        .assert();
}
