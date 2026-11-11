//! EVO-67: a dependency changed in place, without a version bump (a version not published, `ww_error` 0.1.0 in a
//! session that made `CommonError`'s `arg` a typed index). An API's snapshot names the dependency's type by crate
//! version and signature; the source now gives it another signature. With the definitions the source walk saw, the
//! two definitions are compared by the evolution rules, as the dependency's own check compares them.

use super::*;
use crate::compat::tests::add_known;

/// The dependency, `err_dep` 0.1.0, with `arg` of `InvalidArgument` as `arg`.
fn dep(arg: &str) -> String {
    format!(
        r#"
        use wire_weaver::prelude::*;

        #[derive_shrink_wrap(transparent)]
        pub struct ArgIndex(pub u8);

        #[derive_shrink_wrap(ww_repr = u4)]
        pub enum CommonError {{
            Busy,
            InvalidArgument {{ arg: {arg} }},
        }}
    "#
    )
}

const API: &str = r#"
    use err_dep::CommonError;

    #[ww_api_root(wire = "w1.0")]
    pub trait Api {
        fn read() -> Result<u32, ApiError>;
    }

    #[derive_shrink_wrap(ww_repr = u4)]
    pub enum ApiError {
        Common(CommonError),
        Empty,
    }
"#;

/// The API's snapshot saved against the dependency with `old_arg` (the embedded snapshot of the dependency), and the
/// API loaded from source against the dependency changed in place to `new_arg`, with the definitions its walk saw.
fn in_place(
    old_arg: &str,
    new_arg: &str,
) -> (
    ApiBundleOwned,
    ApiBundleOwned,
    wire_weaver_core::SourceNotes,
) {
    let (old_deps, old_dep) = dep_crate("err_dep", &dep(old_arg));
    add_known(old_dep);
    let (new_deps, _) = dep_crate("err_dep", &dep(new_arg));
    let old = load_with("0.1.0", API, &old_deps);
    let (new, notes) = load_with_notes("0.1.0", API, &new_deps);
    (old, new, notes)
}

fn compare_defined(
    old: &ApiBundleOwned,
    new: &ApiBundleOwned,
    definitions: &[&ApiBundleOwned],
) -> Result<Report, String> {
    let none = Recorded::default();
    compare_recorded(old, &none, new, &none, &[], definitions)
}

/// `arg: u8` -> `arg: ArgIndex` in place: compatible, the same bytes, in the method's return and the type, both ways.
/// It was a wire break ("signature .. on the new version and .. on the old version, and the other definition is not
/// known here"), on every crate embedding the type.
#[test]
fn compatible_change_in_place_is_compatible() {
    let (old, new, notes) = in_place("u8", "ArgIndex");
    let definitions = [&notes.definitions];
    for (a, b, from, to) in [
        (&old, &new, "u8", "ArgIndex"),
        (&new, &old, "ArgIndex", "u8"),
    ] {
        let report = compare_defined(a, b, &definitions).unwrap();
        assert_eq!(report.change, Change::Compatible, "{report:#?}");
        assert_eq!(report.wire_bump, WireBump::None, "{report:#?}");
        for path in [
            "Api::read: return type: variant Common: field 0",
            "type ApiError: variant Common: field 0",
        ] {
            let expected =
                format!("{path}: variant InvalidArgument: field `arg`: {from} -> {to}, same bytes");
            assert!(
                report.compatible.contains(&expected),
                "no '{expected}' in {:#?}",
                report.compatible
            );
        }
    }
    // without the source's definitions, the new one is known nowhere: the signatures differ, as before
    let report = compare_defined(&old, &new, &[]).unwrap();
    assert_eq!(report.change, Change::Breaking, "{report:#?}");
    assert!(
        report
            .breaking
            .iter()
            .any(|f| f.contains("the other definition is not known here")),
        "{:#?}",
        report.breaking
    );
}

/// `arg: u8` -> `arg: u16` in place: other bytes, breaking, named at the field rather than as two signatures.
#[test]
fn breaking_change_in_place_is_breaking_at_the_field() {
    let (old, new, notes) = in_place("u8", "u16");
    let report = compare_defined(&old, &new, &[&notes.definitions]).unwrap();
    assert_eq!(report.change, Change::Breaking, "{report:#?}");
    assert_eq!(report.wire_bump, WireBump::Line, "{report:#?}");
    assert!(
        report
            .breaking
            .iter()
            .any(|f| f.contains("variant InvalidArgument: field `arg`") && f.contains("u8")),
        "{:#?}",
        report.breaking
    );
    assert!(
        !report
            .breaking
            .iter()
            .any(|f| f.contains("the other definition is not known here")),
        "{:#?}",
        report.breaking
    );
}
