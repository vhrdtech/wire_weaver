//! EVO-68: a dependency's trait whose definition changed in place, and whose snapshot was refreshed in place. The
//! shape of `tests/thermo_api`'s frozen API lines after `ww_error` 0.1.0 (not published) made `CommonError`'s `arg` a
//! typed index, the same bytes: the lines embed `ww_indication::Indication`, whose method returns a `CommonError`, and
//! name it by its signature. Once both snapshots were refreshed in place, the old definitions were known nowhere, and
//! a same-bytes change read as a wire break needing a new wire line. `ww api save` keeps what a refresh replaces, and
//! the checker compares a trait changed in place by its two definitions, as EVO-67 does a type.

use super::*;
use crate::compat::tests::{add_known, add_superseded};
use crate::superseded::{Superseded, superseded};
use std::path::Path;

/// `err_dep` 0.1.0, with `arg` of `InvalidArgument` as `arg`.
fn err_dep(arg: &str) -> String {
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

/// `ind_dep` 0.1.0: a trait of its own whose method returns `err_dep`'s error.
const IND_DEP: &str = r#"
    use wire_weaver::prelude::*;
    use err_dep::CommonError;

    #[ww_trait]
    pub trait Indication {
        fn identify(times: u8) -> Result<(), CommonError>;
    }
"#;

/// An API line embedding the dependency's trait.
const API: &str = r#"
    #[ww_api_root(wire = "w1.0")]
    pub trait Api {
        fn read() -> u32;
        ww_impl!(indication: ind_dep::Indication);
    }
"#;

/// The two dependencies at `arg`: the `[dependencies]` line of `ind_dep`, its snapshot and the definitions its walk
/// saw, and `err_dep`'s snapshot and definitions.
struct Deps {
    line: String,
    ind: (ApiBundleOwned, wire_weaver_core::SourceNotes),
    err: (ApiBundleOwned, wire_weaver_core::SourceNotes),
}

fn deps(arg: &str) -> Deps {
    let (err_line, err, err_notes) = dep_crate_with("err_dep", &err_dep(arg), "");
    let (line, ind, ind_notes) = dep_crate_with("ind_dep", IND_DEP, &err_line);
    Deps {
        line: format!("{line}{err_line}"),
        ind: (ind, ind_notes),
        err: (err, err_notes),
    }
}

/// The API's line snapshot saved against the dependencies with `u8`, and the API from source against them changed in
/// place to `ArgIndex`, with the definitions its walk saw. The embedded snapshots are the refreshed ones, as after
/// `just save-snapshots`; `keep` says whether what the refreshes replaced was kept, as `ww api save` does now.
fn refreshed_in_place(
    keep: bool,
) -> (
    ApiBundleOwned,
    ApiBundleOwned,
    wire_weaver_core::SourceNotes,
    Vec<Superseded>,
) {
    let old = deps("u8");
    let new = deps("ArgIndex");
    let dir = Path::new("api_snapshots/superseded");
    let mut kept = vec![];
    // ww_error's refresh first, as `just save-snapshots` saves in name order: ww_indication's old trait refers to
    // its old error, which only the kept definition has once ww_error's snapshot is refreshed
    for ((old_snapshot, _), (new_snapshot, new_notes)) in [(old.err, new.err), (old.ind, new.ind)] {
        let (found, warnings) = superseded(dir, &old_snapshot, &new_notes.definitions);
        // without err_dep's kept error, ind_dep's old trait can't be made whole, and says so
        match keep || found.iter().any(|f| f.name == "CommonError") {
            true => assert!(warnings.is_empty(), "{warnings:#?}"),
            false => assert!(
                warnings.len() == 1 && warnings[0].contains("trait ind_dep::Indication"),
                "{warnings:#?}"
            ),
        }
        if keep {
            for one in &found {
                add_superseded(one.kind, &one.name, &one.bytes);
            }
        }
        kept.extend(found);
        add_known(new_snapshot);
    }
    let api_old = load_with("0.1.0", API, &old.line);
    let (api_new, notes) = load_with_notes("0.1.0", API, &new.line);
    (api_old, api_new, notes, kept)
}

fn compare_defined(
    old: &ApiBundleOwned,
    new: &ApiBundleOwned,
    definitions: &[&ApiBundleOwned],
) -> Result<Report, String> {
    let none = Recorded::default();
    compare_recorded(old, &none, new, &none, &[], definitions)
}

/// A refresh in place keeps each own definition whose signature it changes, under the old signature: the error
/// type that changed, and the trait whose method returns it, which didn't change in its own source.
#[test]
fn refresh_in_place_keeps_what_it_replaces() {
    let (_, _, _, kept) = refreshed_in_place(true);
    let names: Vec<String> = kept
        .iter()
        .map(|k| k.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(kept.len(), 2, "{names:#?}");
    assert!(
        names[0].starts_with("err_dep_0_1_0-type-CommonError-"),
        "{names:#?}"
    );
    assert!(
        names[1].starts_with("ind_dep_0_1_0-trait-Indication-"),
        "{names:#?}"
    );
    for one in &kept {
        // the definition cache's entry: the canonical bytes, under the signature they hash to
        let bundle =
            ww_self::definition_cache::decode(one.kind, &one.name, &one.signature, &one.bytes)
                .unwrap();
        let name = ww_self::definition_cache::file_name(
            one.kind,
            bundle.crate_version(0).unwrap(),
            &one.name,
            &one.signature,
        );
        assert_eq!(one.path, Path::new("api_snapshots/superseded").join(name));
    }
}

/// The API line's snapshot, which names the trait by its old signature, against the source, which names it by the
/// new one: compatible, the same bytes, both ways, with no wire bump.
#[test]
fn trait_changed_in_place_after_a_refresh_is_compatible() {
    let (old, new, notes, _) = refreshed_in_place(true);
    let definitions = [&notes.definitions];
    for (a, b, from, to) in [
        (&old, &new, "u8", "ArgIndex"),
        (&new, &old, "ArgIndex", "u8"),
    ] {
        let report = compare_defined(a, b, &definitions).unwrap();
        assert_eq!(report.change, Change::Compatible, "{report:#?}");
        assert_eq!(report.wire_bump, WireBump::None, "{report:#?}");
        let expected = format!("variant InvalidArgument: field `arg`: {from} -> {to}, same bytes");
        assert!(
            report.compatible.iter().any(|f| f.contains(&expected)),
            "no '{expected}' in {:#?}",
            report.compatible
        );
    }
}

/// Without what the refreshes replaced, the old definitions are known nowhere: the signatures differ, as before.
#[test]
fn without_the_kept_definitions_it_is_a_wire_break() {
    let (old, new, notes, _) = refreshed_in_place(false);
    let report = compare_defined(&old, &new, &[&notes.definitions]).unwrap();
    assert_eq!(report.change, Change::Breaking, "{report:#?}");
    assert!(
        report.breaking.iter().any(|f| f
            .to_string()
            .contains("trait ind_dep::Indication 0.1.0 has signature")),
        "{:#?}",
        report.breaking
    );
}
