//! Each root compared with its own line's snapshots in `api_snapshots/` (EVO-39): the lines' history from when each
//! was a crate of its own (1.0.0 to 3.0.0), then one snapshot per root of this crate. Fails when a root's snapshot of
//! the current version is missing or differs from the source, and when a root changed without the bump it needs.
//! `WW_API_SAVE=1 cargo test -p thermo_api@4.0.0 --test evolution` saves the current version's snapshots (the old
//! firmware crates of `tests/thermo_api_old` are named `thermo_api` too, hence the version).

#[test]
fn api_evolution() {
    wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR"))
        .require_current(true)
        .assert();
}
