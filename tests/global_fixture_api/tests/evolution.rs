//! The current source compared with `api_snapshots/global_fixture_api_0_1_0.ron`: fails when the crate changed without
//! the bump it needs. Hosts and tap decoders find the trait by this snapshot, as they find a real `ww_stdlib` trait.
//! `WW_API_SAVE=1 cargo test -p global_fixture_api --test evolution` saves a new version's snapshot.

#[test]
fn api_evolution() {
    wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR"))
        .require_current(true)
        .strict(true)
        .assert();
}
