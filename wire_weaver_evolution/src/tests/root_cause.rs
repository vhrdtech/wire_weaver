//! EVO-21: several API items broken by the same dependency type are reported once, under that type, instead of
//! looking like unrelated breaks.

use super::*;
use std::path::PathBuf;

/// A tiny path-dependency crate with one struct, `id`'s type and the crate's own version set by `version`: a
/// stand-in for a `ww_stdlib` crate that makes a breaking change of its own (as `ww_board_info`'s `Rework` did,
/// STD-98).
fn dep_dir(version: &str, id_ty: &str) -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/ww_evolution_tests_dep")
        .join(format!(
            "{version}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"dep_api\"\nversion = \"{version}\"\nedition = \"2024\"\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        format!(
            "use wire_weaver::prelude::*;\n\n#[derive_shrink_wrap]\npub struct Shared {{\n    pub id: {id_ty},\n}}\n"
        ),
    )
    .unwrap();
    dir
}

/// Two methods using the same dependency type, as two different `ww_impl!`-reached traits would in the real
/// incident (`BoardInfo`, `Indication`, `Clock`, `Can`, `LastReset`, `Counters` all reaching `Rework`).
const API: &str = r#"
    use dep_api::Shared;

    #[ww_api_root(wire = "w1.0")]
    pub trait Api {
        fn one(s: Shared);
        fn two(s: Shared);
    }
"#;

fn api_with_dep(version: &str, id_ty: &str) -> ApiBundleOwned {
    let dep = dep_dir(version, id_ty);
    let dependencies = format!("dep_api = {{ path = {:?} }}\n", dep.canonicalize().unwrap());
    load_with("0.1.0", API, &dependencies)
}

/// `dep_api` bumps its own breaking position (`u8` -> `u16`, an incompatible 0.1.0 -> 0.2.0): both `Api::one` and
/// `Api::two` break, but the checker finds the same cause for both, so they group under it instead of reading as
/// two unrelated breaks.
#[test]
fn shared_dependency_type_groups_every_item_it_reaches() {
    let old = api_with_dep("0.1.0", "u8");
    let new = api_with_dep("0.2.0", "u16");
    let report = compare(&old, &new).unwrap();
    assert_eq!(report.change, Change::Breaking, "{report:#?}");
    assert_eq!(report.breaking.len(), 2, "{:#?}", report.breaking);
    assert!(
        report.breaking.iter().all(|f| f.root.is_some()),
        "{:#?}",
        report.breaking
    );

    let (groups, rest) = report.breaking_groups();
    assert!(rest.is_empty(), "{rest:#?}");
    assert_eq!(groups.len(), 1, "{groups:#?}");
    let (root, findings) = &groups[0];
    assert_eq!(root.crate_id, "dep_api");
    assert_eq!(root.name, "Shared");
    assert_eq!(findings.len(), 2, "{findings:#?}");
}

/// A single item reaching a changed dependency type is not grouped with itself: it reads as any other breaking
/// change, root cause named but not pulled into a one-item group.
#[test]
fn a_lone_item_is_not_grouped() {
    let one_item = API.replace("fn two(s: Shared);\n", "");
    let old = {
        let dep = dep_dir("0.1.0", "u8");
        let dependencies = format!("dep_api = {{ path = {:?} }}\n", dep.canonicalize().unwrap());
        load_with("0.1.0", &one_item, &dependencies)
    };
    let new = {
        let dep = dep_dir("0.2.0", "u16");
        let dependencies = format!("dep_api = {{ path = {:?} }}\n", dep.canonicalize().unwrap());
        load_with("0.1.0", &one_item, &dependencies)
    };

    let report = compare(&old, &new).unwrap();
    assert_eq!(report.change, Change::Breaking, "{report:#?}");
    assert_eq!(report.breaking.len(), 1, "{:#?}", report.breaking);
    assert!(report.breaking[0].root.is_some(), "{:#?}", report.breaking);

    let (groups, rest) = report.breaking_groups();
    assert!(groups.is_empty(), "{groups:#?}");
    assert_eq!(rest.len(), 1);
}
