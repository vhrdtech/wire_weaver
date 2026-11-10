//! EVO-40: within a wire line a resource is its position; across a break it is its path, or the place its
//! `#[moved_from]` names. A resource of the same name elsewhere is only proposed, never taken for the same one.

use super::*;

/// The crate `lib_rs` at crate version `version`, with its own `#[moved_from]`.
fn load_moved(version: &str, lib_rs: &str) -> (ApiBundleOwned, Vec<Move>) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/ww_evolution_tests_moved_from")
        .join(format!(
            "{version}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"test_api\"\nversion = \"{version}\"\nedition = \"2024\"\npublish = false\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        format!("use wire_weaver::prelude::*;\n\n{lib_rs}"),
    )
    .unwrap();
    let loaded = wire_weaver_core::load_crate_with_notes(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
    let (bundle, notes) = loaded.unwrap();
    let moves = notes.moved_from.iter().map(Move::from).collect();
    (bundle, moves)
}

/// `old` at crate 0.1.0 compared with `new` at 0.2.0, with `new`'s `#[moved_from]`.
fn compared(old: &str, new: &str) -> Report {
    let (old, _) = load_moved("0.1.0", old);
    let (new, moves) = load_moved("0.2.0", new);
    compare_moved(&old, &new, &moves).unwrap()
}

fn messages(report: &Report) -> String {
    report
        .breaking
        .iter()
        .map(|f| f.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

const W2: &str = r#"
#[ww_api_root(wire = "w2.1")]
pub trait Api {
    fn configure(rate: u16);
    property!(ro uptime: u32);
    fn self_test() -> bool;
}
"#;

/// Line 3: `uptime` and `self_test` in a new sub-level, `uptime` in a wider type.
const W3: &str = r#"
#[ww_api_root(wire = "w3.0")]
pub trait Api {
    fn configure(rate: u16);
    ww_impl!(health: Health);
}

#[ww_trait]
pub trait Health {
    property!(ro uptime: u64);
    #[moved_from(w2.1, Api::self_test)]
    fn self_test() -> bool;
}
"#;

/// A resource marked `#[moved_from]` with the same signature is reported as a move, not as a removal and an
/// addition; one of the same name without it, but another signature, is a new resource, said so.
#[test]
fn move_across_a_break() {
    let report = compared(W2, W3);
    assert!(report.across_line);
    assert!(report.moved_from_errors.is_empty(), "{report:#?}");
    let breaking = messages(&report);
    assert!(
        breaking.contains(
            "Api::self_test moved to Health::self_test (#[moved_from], the same signature)"
        ),
        "{breaking}"
    );
    assert!(!breaking.contains("Api::self_test removed"), "{breaking}");
    assert!(breaking.contains("Api::uptime removed"), "{breaking}");
    let moved = report
        .breaking
        .iter()
        .find(|f| f.rule == Rule::MovedAcross)
        .unwrap();
    assert!(
        moved
            .to_string()
            .contains("rules.md#moving-a-resource-to-another-place")
    );
    assert_eq!(
        report.proposals,
        [
            "Health::uptime is named like Api::uptime of w2.1, removed, but its signature differs (u64 on the new \
          version, u32 on the old version): a new resource, which #[moved_from] can't name"
        ]
    );
    assert!(
        !report
            .compatible
            .iter()
            .any(|c| c.contains("Health::self_test")),
        "{:?}",
        report.compatible
    );
    assert_eq!(report.wire_bump, WireBump::Line);
    assert_eq!(report.check_wire(), Ok(()));
    assert_eq!(report.check_version(), Ok(()));
}

/// Without the attribute, the same resource at another place is a removal and an addition, and the checker
/// proposes the attribute: it never adds it, nor takes the resource for the old one by its name.
#[test]
fn same_name_elsewhere_is_proposed() {
    let unmarked = W3.replace("    #[moved_from(w2.1, Api::self_test)]\n", "");
    let report = compared(W2, &unmarked);
    let breaking = messages(&report);
    assert!(breaking.contains("Api::self_test removed"), "{breaking}");
    assert!(!breaking.contains("moved to"), "{breaking}");
    assert!(
        report.proposals.contains(
            &"Health::self_test is named like Api::self_test of w2.1, removed: if it is the same resource, \
              mark it #[moved_from(w2.1, Api::self_test)]; a resource of the same name is never taken for the \
              same one by itself"
                .to_string()
        ),
        "{:#?}",
        report.proposals
    );
    assert_eq!(report.proposals.len(), 2, "{:#?}", report.proposals);
}

/// `on()` of a LED and `on()` of a motor on the next line: the same name, same signature, another meaning. Only
/// proposed, never matched.
#[test]
fn same_name_other_meaning_is_never_taken() {
    let w1 = r#"
        #[ww_api_root(wire = "w1.0")]
        pub trait Api { ww_impl!(led: Led); }
        #[ww_trait]
        pub trait Led { fn on(); }
    "#;
    let w2 = r#"
        #[ww_api_root(wire = "w2.0")]
        pub trait Api { ww_impl!(motor: Motor); }
        #[ww_trait]
        pub trait Motor { fn on(); }
    "#;
    let report = compared(w1, w2);
    let breaking = messages(&report);
    assert!(breaking.contains("trait Led removed"), "{breaking}");
    assert!(breaking.contains("Api::led removed"), "{breaking}");
    assert!(!breaking.contains("moved to"), "{breaking}");
    assert!(
        report.compatible.contains(&"trait Motor added".to_string()),
        "{:?}",
        report.compatible
    );
    assert_eq!(report.proposals.len(), 1, "{:#?}", report.proposals);
    assert!(
        report.proposals[0]
            .starts_with("Motor::on is named like Led::on of w1.0, removed: if it is the same"),
        "{}",
        report.proposals[0]
    );
}

/// Across a break a resource is its path: one renamed in place is a removal and an addition (not a rename with
/// another argument), one at another position is the same resource, its new position a wire change.
#[test]
fn across_a_break_by_path_not_position() {
    let w1 = r#"
        #[ww_api_root(wire = "w1.0")]
        pub trait Api { fn set_rate(rate: u16); fn read() -> u8; }
    "#;
    let w2 = r#"
        #[ww_api_root(wire = "w2.0")]
        pub trait Api { fn read() -> u8; fn configure(rate: u16, mode: u8); }
    "#;
    let report = compared(w1, w2);
    let breaking = messages(&report);
    assert!(breaking.contains("Api::set_rate removed"), "{breaking}");
    assert!(!breaking.contains("renamed"), "{breaking}");
    assert!(!breaking.contains("Api::read"), "{breaking}");
    assert!(
        report
            .compatible
            .contains(&"Api::configure added".to_string()),
        "{:?}",
        report.compatible
    );
    assert!(
        report
            .wire_reasons
            .contains(&"Api::read at position 0 on the new line, was 1".to_string()),
        "{:?}",
        report.wire_reasons
    );
    // a new position alone: the wire breaks, Rust code doesn't
    let swapped = r#"
        #[ww_api_root(wire = "w2.0")]
        pub trait Api { fn read() -> u8; fn set_rate(rate: u16); }
    "#;
    let report = compared(w1, swapped);
    assert_eq!(report.change, Change::Compatible, "{report:#?}");
    assert_eq!(report.wire_bump, WireBump::Line, "{report:#?}");
    assert_eq!(report.check_wire(), Ok(()));
    // within a line, the same swap is a move by position, as before
    let report = compared(w1, &swapped.replace("w2.0", "w1.1"));
    assert!(messages(&report).contains("Api::set_rate moved from id 0 to 1"));
}

/// Across a break a stream item replaced by another type of the crate is named as such, not compared field by
/// field; within a line, where the bytes are what counts, it is compared field by field as before.
#[test]
fn replaced_type_is_not_compared_field_by_field() {
    let w2 = r#"
        #[ww_api_root(wire = "w2.1")]
        pub trait Api { stream!(samples: Sample); }
        #[derive_shrink_wrap]
        pub struct Sample { pub seq: u16, pub temp: i32 }
    "#;
    let w3 = r#"
        #[ww_api_root(wire = "w3.0")]
        pub trait Api { stream!(samples: Batch); }
        #[derive_shrink_wrap]
        pub struct Sample { pub seq: u16, pub temp: i32 }
        #[derive_shrink_wrap]
        pub struct Batch { pub first: u16, pub temps: Vec<i32> }
    "#;
    let report = compared(w2, w3);
    let breaking = messages(&report);
    assert!(
        breaking.contains(
            "Api::samples: Batch on the new version, Sample on the old version: another type, not compared field \
             by field"
        ),
        "{breaking}"
    );
    assert!(!breaking.contains("temp"), "{breaking}");
    let report = compared(w2, &w3.replace("w3.0", "w2.2"));
    assert!(
        messages(&report).contains("Api::samples: struct Sample: field `temp`"),
        "{report:#?}"
    );
}

/// A `#[moved_from]` the checker can't verify fails the wire check: another signature, nothing at the place it
/// names, a place on the API's own line. One naming an earlier break is left alone within the next line.
#[test]
fn moved_from_that_doesnt_hold_is_refused() {
    let wider = W3.replace(
        "    property!(ro uptime: u64);",
        "    #[moved_from(w2.1, Api::uptime)]\n    property!(ro uptime: u64);",
    );
    let report = compared(W2, &wider);
    assert_eq!(report.moved_from_errors.len(), 1, "{report:#?}");
    let e = report.check_wire().unwrap_err();
    assert!(
        e.contains(
            "#[moved_from(w2.1, Api::uptime)] on Health::uptime: not the same signature as Api::uptime at w2.1, \
             u64 on the new version, u32 on the old version"
        ),
        "{e}"
    );
    assert!(messages(&report).contains("Api::uptime removed"));

    let nothing = W3.replace("Api::self_test)", "Api::self_check)");
    let e = compared(W2, &nothing).check_wire().unwrap_err();
    assert!(
        e.contains("on Health::self_test: w2.1, compared with, has no Api::self_check"),
        "{e}"
    );

    let same_line = W3.replace("(w2.1, Api::self_test)", "(w3.0, Api::self_test)");
    let e = compared(W2, &same_line).check_wire().unwrap_err();
    assert!(
        e.contains("the API is at w3.0: within a line a resource is its position"),
        "{e}"
    );

    // w3.0 -> w3.1: a move from line 2 can't be checked against w3.0; it is listed for the snapshot of line 2
    // (`Check` verifies it there), never dropped
    let next = W3.replace("w3.0", "w3.1");
    let report = compared(W3, &next);
    assert!(!report.across_line);
    assert!(report.moved_from_errors.is_empty(), "{report:#?}");
    assert!(report.breaking.is_empty(), "{report:#?}");
    assert_eq!(report.unverified_moves.len(), 1, "{report:#?}");
    assert_eq!(report.unverified_moves[0].from_path(), "Api::self_test");
    // and `verify_move` checks it against line 2
    let (w2, _) = load_moved("0.1.0", W2);
    let (w3_1, moves) = load_moved("0.2.0", &next);
    assert_eq!(verify_move(&w2, &w3_1, &moves[0]), Ok(()));
    let (w3_1, moves) = load_moved("0.2.0", &nothing.replace("w3.0", "w3.1"));
    let e = verify_move(&w2, &w3_1, &moves[0]).unwrap_err();
    assert!(e[0].contains("has no Api::self_check"), "{e:?}");
}

/// Every claim is checked, none dropped: an attribute naming its own place, two naming one place, and a place still
/// there on the new version are each reported (EVO-40).
#[test]
fn every_claim_is_reported() {
    // `self_test` stays at its place and names it; `Health::self_test` claims it too
    let both = W3.replace(
        "    ww_impl!(health: Health);\n}",
        "    ww_impl!(health: Health);\n    #[moved_from(w2.1, Api::self_test)]\n    fn self_test() -> bool;\n}",
    );
    let report = compared(W2, &both);
    let errors = report.moved_from_errors.join("\n");
    for e in [
        "#[moved_from(w2.1, Api::self_test)] on Api::self_test: names its own place: across a break a resource at \
         the same place is matched by its path anyway, remove the attribute",
        "on Api::self_test: Health::self_test names Api::self_test of line 2 too: one resource can't become two",
        "on Health::self_test: Api::self_test is still there on the new version: a resource that stayed can't have \
         moved too; remove the attribute, or the old resource",
    ] {
        assert!(errors.contains(e), "{e} not in:\n{errors}");
    }
    assert!(report.check_wire().is_err());
    assert!(!messages(&report).contains("moved to"), "{report:#?}");
}

/// A move whose source still exists at its old path is refused, saying so, instead of reading the old one as added.
#[test]
fn source_still_at_its_old_path() {
    let stayed = W3.replace(
        "    ww_impl!(health: Health);\n}",
        "    ww_impl!(health: Health);\n    fn self_test() -> bool;\n}",
    );
    let report = compared(W2, &stayed);
    assert_eq!(
        report.moved_from_errors,
        [
            "#[moved_from(w2.1, Api::self_test)] on Health::self_test: Api::self_test is still there on the new \
          version: a resource that stayed can't have moved too; remove the attribute, or the old resource"
        ]
    );
    // the one that stayed is the old one, by its path; the one marked is new
    let breaking = messages(&report);
    assert!(!breaking.contains("Api::self_test"), "{breaking}");
    assert!(!breaking.contains("moved to"), "{breaking}");
}

/// The revision named is checked too, not only the line: one newer than the old version, or older than the resource's
/// own `#[since]`, is refused.
#[test]
fn revision_named_is_checked() {
    let newer = W3.replace("(w2.1, Api::self_test)", "(w2.9, Api::self_test)");
    let e = compared(W2, &newer).check_wire().unwrap_err();
    assert!(
        e.contains(
            "w2.9 is newer than w2.1, the version compared with: name a revision Api::self_test had"
        ),
        "{e}"
    );
    let since = W2.replace(
        "    fn self_test() -> bool;",
        "    #[since = \"w2.1\"]\n    fn self_test() -> bool;",
    );
    let before = W3.replace("(w2.1, Api::self_test)", "(w2.0, Api::self_test)");
    let e = compared(&since, &before).check_wire().unwrap_err();
    assert!(
        e.contains("Api::self_test was added in w2.1 (#[since]), after w2.0"),
        "{e}"
    );
    // a revision it had is fine
    assert_eq!(compared(&since, W3).check_wire(), Ok(()));
}
