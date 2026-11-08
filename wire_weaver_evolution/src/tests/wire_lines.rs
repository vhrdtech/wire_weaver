//! EVO-26: a dependency crate's trait or type is compared by the crate's wire line, not its crate version, when both
//! sides declare one: a Rust-only break of a `ww_stdlib` crate (a new crate major, the same bytes) no longer breaks
//! the APIs using it.

use super::*;
use std::path::PathBuf;

/// A path-dependency crate with one `#[ww_trait]`, at crate version `version` and, when given, wire version `wire`
/// (`[package.metadata.wire_weaver]`): a stand-in for a `ww_stdlib` crate.
fn dep_dir(version: &str, wire: Option<&str>) -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/ww_evolution_tests_wire_dep")
        .join(format!(
            "{version}-{}-{}-{:?}",
            wire.unwrap_or("none"),
            std::process::id(),
            std::thread::current().id()
        ));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let metadata = wire
        .map(|w| format!("\n[package.metadata.wire_weaver]\nwire = \"{w}\"\n"))
        .unwrap_or_default();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"dep_api\"\nversion = \"{version}\"\nedition = \"2024\"\n{metadata}"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "use wire_weaver::prelude::*;\n\n#[ww_trait]\npub trait Dep {\n    fn get() -> u8;\n}\n",
    )
    .unwrap();
    dir
}

const API: &str = r#"
    #[ww_api_root(wire = "w1.0")]
    pub trait Api {
        ww_impl!(dep: dep_api::Dep);
    }
"#;

fn api_with_dep(version: &str, wire: Option<&str>) -> ApiBundleOwned {
    let dep = dep_dir(version, wire);
    let dependencies = format!("dep_api = {{ path = {:?} }}\n", dep.canonicalize().unwrap());
    load_with("0.1.0", API, &dependencies)
}

/// The dependency's crate major moves (0.1.0 -> 0.2.0) with the trait unchanged: without wire versions that is a
/// break (the old rule, kept for crates that declare none); on one wire line it is not; on two lines it is, named.
#[test]
fn dependency_compared_by_its_wire_line() {
    let report = compare(&api_with_dep("0.1.0", None), &api_with_dep("0.2.0", None)).unwrap();
    assert_eq!(report.change, Change::Breaking, "{report:#?}");
    assert!(
        format!("{:?}", report.breaking).contains("from incompatible crate versions"),
        "{report:#?}"
    );

    let old = api_with_dep("0.1.0", Some("w1.0"));
    let new = api_with_dep("0.2.0", Some("w1.1"));
    assert_eq!(
        new.crate_wire(1),
        Some(WireVersion::new(1, 1)),
        "{:?}",
        new.ext_crates
    );
    let report = compare(&old, &new).unwrap();
    assert!(report.breaking.is_empty(), "{report:#?}");

    let report = compare(&old, &api_with_dep("0.2.0", Some("w2.0"))).unwrap();
    assert_eq!(report.change, Change::Breaking, "{report:#?}");
    let breaking = format!("{:?}", report.breaking);
    assert!(
        breaking.contains("from dep_api on different wire lines: w1.0"),
        "{breaking}"
    );
}
