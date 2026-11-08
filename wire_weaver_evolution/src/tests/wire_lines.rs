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

/// A path-dependency crate `shared_api` at crate version `version` and wire version `wire`, with one struct whose
/// `id` is of type `id_ty`: a stand-in for a `ww_stdlib` crate with a type several API items use.
fn shared_dep_dir(version: &str, wire: &str, id_ty: &str) -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/ww_evolution_tests_shared_dep")
        .join(format!(
            "{version}-{wire}-{id_ty}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"shared_api\"\nversion = \"{version}\"\nedition = \"2024\"\n\n\
             [package.metadata.wire_weaver]\nwire = \"{wire}\"\n"
        ),
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

/// Two methods using the same dependency type, at the API's wire version `api_wire`.
fn api_with_shared(api_wire: &str, version: &str, wire: &str, id_ty: &str) -> ApiBundleOwned {
    let src = format!(
        r#"
        use shared_api::Shared;

        #[ww_api_root(wire = "{api_wire}")]
        pub trait Api {{
            fn one(s: Shared);
            fn two(s: Shared);
        }}
    "#
    );
    let dep = shared_dep_dir(version, wire, id_ty);
    let dependencies = format!(
        "shared_api = {{ path = {:?} }}\n",
        dep.canonicalize().unwrap()
    );
    load_with("0.1.0", &src, &dependencies)
}

/// EVO-27: a `ww_stdlib` crate's wire break (a new line, its type changed) forces a new wire line on every API using
/// it, reported as one cause: the dependency's new line, with the breaks it caused named once (EVO-21).
#[test]
fn dependency_wire_break_forces_a_new_line() {
    let old = api_with_shared("w1.0", "0.1.0", "w1.0", "u8");
    let new = api_with_shared("w1.0", "0.2.0", "w2.0", "u16");
    let report = compare(&old, &new).unwrap();
    assert_eq!(report.wire_bump, WireBump::Line, "{report:#?}");
    assert_eq!(
        report.wire_reasons,
        [
            "shared_api on a new wire line, w1.0 -> w2.0 (2 wire breaks because type shared_api::Shared changed)"
        ],
        "{report:#?}"
    );
    let e = report.check_wire().unwrap_err();
    assert_eq!(
        e,
        "test_api wire w1.0 -> w1.0 has shared_api on a new wire line, w1.0 -> w2.0 (2 wire breaks because type \
         shared_api::Shared changed), which needs a new wire line: w2.0 or later"
    );
    let bumped = api_with_shared("w2.0", "0.2.0", "w2.0", "u16");
    compare(&old, &bumped).unwrap().check_wire().unwrap();

    // the same type on a new line: still a new line of the API, as nothing on the device tells the lines apart
    let report = compare(&old, &api_with_shared("w1.0", "0.1.0", "w2.0", "u8")).unwrap();
    assert_eq!(report.wire_bump, WireBump::Line, "{report:#?}");
    assert!(
        report.wire_reasons[0].starts_with("shared_api on a new wire line, w1.0 -> w2.0"),
        "{report:#?}"
    );
}

/// EVO-27: a dependency at a newer revision of its line is a new revision of the API (what its devices carry grew),
/// and a Rust-only crate break of the dependency on the same revision needs no wire bump at all.
#[test]
fn dependency_revision_is_a_revision() {
    let old = api_with_shared("w1.0", "0.1.0", "w1.0", "u8");
    let report = compare(&old, &api_with_shared("w1.0", "0.1.1", "w1.1", "u8")).unwrap();
    assert_eq!(report.wire_bump, WireBump::Revision, "{report:#?}");
    assert_eq!(report.wire_reasons, ["shared_api wire w1.0 -> w1.1"]);
    assert!(report.check_wire().is_err());
    let report = compare(&old, &api_with_shared("w1.1", "0.1.1", "w1.1", "u8")).unwrap();
    assert_eq!(report.check_wire(), Ok(()));

    let report = compare(&old, &api_with_shared("w1.0", "0.2.0", "w1.0", "u8")).unwrap();
    assert_eq!(report.wire_bump, WireBump::None, "{report:#?}");
    assert!(report.breaking.is_empty(), "{report:#?}");
    assert_eq!(report.check_wire(), Ok(()));
}
