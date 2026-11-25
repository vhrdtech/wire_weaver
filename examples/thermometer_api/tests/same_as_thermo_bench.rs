//! This API is line 1 at w1.0 of `tests/thermo_api`, the multi-line bench that tells the thermometer's later life,
//! so the book's two stories stay one device. The bench's line 1 at w1.0 was published as `thermo_api` 1.0.0; its
//! snapshot is compared with this crate's source under the bench's crate name. Doc comments may differ, nothing
//! else may.

use std::path::Path;

use wire_weaver_evolution::{Change, Difference, WireBump, compare, diff, snapshot_files};

#[test]
fn same_wire_as_the_bench_line_1() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bench = snapshot_files::parse(
        &here.join("../../tests/thermo_api/api_snapshots/thermo_api_1_0_0.ron"),
    )
    .expect("the bench's 1.0.0 snapshot");
    let mut example = wire_weaver_core::load_crate(here).expect("thermometer_api loads");
    // the same API under another crate name: the bench's name, so that the checker compares the two
    example.ext_crates[0].crate_id = bench.ext_crates[0].crate_id.clone();
    example.root.trait_name = bench.root.trait_name.clone();

    let report = compare(&bench, &example).expect("comparable");
    assert!(report.change <= Change::DocsOnly, "{report:#?}");
    assert_eq!(report.wire_bump, WireBump::None, "{report:#?}");

    let differences: Vec<_> = diff(&bench, &example)
        .expect("comparable")
        .into_iter()
        .filter(|d| !matches!(d, Difference::Docs { .. }))
        .collect();
    assert!(differences.is_empty(), "{differences:#?}");
}
