//! EVO-15: a transparent wrapper defined in another crate whose snapshot is known (`ww_block::Lease` in `ww_sd`). An
//! API's bundle leaves it out by its signature, so the checker reads its definition from that crate's snapshot:
//! `u32` -> `Lease` is the same bytes in every position, as for a wrapper of the crate's own.

use super::*;
use crate::compat::tests::add_known;

/// The other crate: `Lease`, a wrapper of a plain number, and `Timeout`, a wrapper of a wrapper of that crate (its
/// field refers to that crate's snapshot, not to the API's bundle).
const DEP: &str = r#"
    use wire_weaver::prelude::*;

    #[derive_shrink_wrap(transparent)]
    pub struct Lease(pub u32);

    #[derive_shrink_wrap(transparent)]
    pub struct Ticks(pub u16);

    #[derive_shrink_wrap(transparent)]
    pub struct Timeout(pub Ticks);
"#;

fn dep(lib_rs: &str) -> (String, ApiBundleOwned) {
    dep_crate("lease_dep", lib_rs)
}

/// An API using `lease` for a method argument, a method's return, a read-only property and a struct field, and
/// `timeout` for an argument.
fn api(lease: &str, timeout: &str) -> String {
    format!(
        r#"
        use lease_dep::{{Lease, Timeout}};

        #[ww_api_root(wire = "w1.0")]
        pub trait Api {{
            fn release(lease: {lease}, timeout: {timeout});
            fn claim() -> {lease};
            property!(ro held: {lease});
        }}

        #[derive_shrink_wrap]
        pub struct Held {{
            pub lease: {lease},
        }}

        #[ww_trait]
        pub trait Uses {{
            property!(ro last: Held);
        }}
    "#
    )
}

/// `u32` <-> `lease_dep::Lease` (and `u16` <-> `Timeout`): compatible both ways in every position, reported as the
/// same bytes. It was "type definition is not available on one side", a wire break.
#[test]
fn plain_to_wrapper_of_another_crate_and_back_is_compatible() {
    let (deps, snapshot) = dep(DEP);
    add_known(snapshot);
    let plain = load_with("0.1.0", &api("u32", "u16"), &deps);
    let wrapped = load_with("0.1.0", &api("Lease", "Timeout"), &deps);
    assert!(
        wrapped.types.iter().any(|t| matches!(
            t,
            TypeLocationOwned::SkippedFullVersion { type_name, .. } if type_name == "Lease"
        )),
        "Lease is not left out of the API's bundle"
    );
    for (old, new, from, to) in [
        (&plain, &wrapped, "u32", "Lease"),
        (&wrapped, &plain, "Lease", "u32"),
    ] {
        let report = compare(old, new).unwrap();
        assert_eq!(report.change, Change::Compatible, "{report:#?}");
        assert_eq!(report.wire_bump, WireBump::None, "{report:#?}");
        for path in [
            "Api::release: argument `lease`",
            "Api::claim: return type",
            "Api::held",
            "type Held: field `lease`",
        ] {
            let expected = format!("{path}: {from} -> {to}, same bytes");
            assert!(
                report.compatible.contains(&expected),
                "no '{expected}' in {:#?}",
                report.compatible
            );
        }
    }
    let report = compare(&wrapped, &wrapped).unwrap();
    assert_eq!(report.change, Change::None, "{report:#?}");
}

/// The wrapper's field is checked, not taken as compatible: `Lease` wraps a `u32`, a `u16` or a `u8` is other bytes.
#[test]
fn wrapper_of_another_crate_over_other_bytes_is_breaking() {
    let (deps, snapshot) = dep(DEP);
    add_known(snapshot);
    let narrower = load_with("0.1.0", &api("u8", "u8"), &deps);
    let wrapped = load_with("0.1.0", &api("Lease", "Timeout"), &deps);
    for (old, new) in [(&narrower, &wrapped), (&wrapped, &narrower)] {
        let report = compare(old, new).unwrap();
        assert_eq!(report.change, Change::Breaking, "{report:#?}");
        assert!(
            report
                .breaking
                .iter()
                .any(|f| f.contains("Api::release") && f.contains("Lease")),
            "{:#?}",
            report.breaking
        );
        assert!(
            report
                .breaking
                .iter()
                .any(|f| f.contains("Api::release") && f.contains("Timeout")),
            "{:#?}",
            report.breaking
        );
    }
}

/// The other crate's snapshot was saved before `Lease` was added (a version not published, changed without a bump,
/// as `ww_block` in the session that found this): the definition is known nowhere, which is an error naming it, not
/// a wire break.
#[test]
fn wrapper_missing_from_the_other_crates_snapshot_is_an_error_naming_it() {
    let (deps, _) = dep(DEP);
    let (_, stale) = dep(&DEP.replace("pub struct Lease(pub u32);", ""));
    add_known(stale);
    let plain = load_with("0.1.0", &api("u32", "u16"), &deps);
    let wrapped = load_with("0.1.0", &api("Lease", "Timeout"), &deps);
    for (old, new) in [(&plain, &wrapped), (&wrapped, &plain)] {
        let err = compare(old, new).unwrap_err();
        assert!(
            err.contains("no definition of the type lease_dep::Lease 0.1.0 (signature")
                && err.contains("save the snapshot that is out of date again"),
            "{err}"
        );
        assert!(!err.contains("not available on one side"), "{err}");
    }
}
