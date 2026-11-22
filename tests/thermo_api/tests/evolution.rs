//! Each root compared with its own line's snapshots in `api_snapshots/` (EVO-39): 1.0.0 with line 1's root at the
//! crate root, then one snapshot per root from 2.0.0 on, and the record of line 1's retirement in 4.0.0. Fails when a
//! root's snapshot of the current version is missing or differs from the source, and when a root changed without the
//! bump it needs. `WW_API_SAVE=1 cargo test -p thermo_api@4.0.0 --test evolution` saves the current version's snapshots
//! (the old firmware crate of `tests/thermo_api_old` is named `thermo_api` too, hence the version).

#[test]
fn api_evolution() {
    // `strict(true)` (TOOL-35): our own repo wants a reason on every `#[no_unit_hint]`, opted in explicitly
    // here rather than implied by `require_current` (the documented `tests/evolution.rs` template,
    // docs/evolution/checker_tool.md, doesn't force external users copying it into writing one).
    wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR"))
        .require_current(true)
        .strict(true)
        .assert();
}
