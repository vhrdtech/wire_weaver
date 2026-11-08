//! EVO-27: the checker derives both bumps from one comparison, the wire version's and the crate version's, and
//! checks each on its own.

use super::*;

const OLD: &str = r#"
#[ww_api_root(wire = "w2.3")]
pub trait Api {
    fn led_on();
    fn configure(a: u8, b: u16);
    #[stream(u8)]
    fn samples(rate: u16);
}

#[derive_shrink_wrap]
pub struct Config {
    pub a: u8,
    pub b: u16,
}
"#;

/// `src` at wire version `wire` instead of `w2.3`.
fn at(src: &str, wire: &str) -> String {
    src.replace("wire = \"w2.3\"", &format!("wire = \"{wire}\""))
}

/// `old` at crate 0.1.0 compared with `new` at crate `version`.
fn bumps(old: &str, new: &str, version: &str) -> Report {
    compare(&load("0.1.0", old), &load(version, new)).unwrap()
}

/// What both checks say, `Ok` or the error text.
fn verdicts(report: &Report) -> (Result<(), String>, Result<(), String>) {
    (report.check_wire(), report.check_version())
}

/// The table row: a new method is a new revision and a crate patch.
#[test]
fn new_method_is_a_revision_and_a_patch() {
    let added = OLD.replace(
        "fn samples(rate: u16);",
        "fn samples(rate: u16);\n#[since = \"w2.4\"]\nfn led_off();",
    );
    let report = bumps(OLD, &at(&added, "w2.4"), "0.1.1");
    assert_eq!(report.wire_bump, WireBump::Revision, "{report:#?}");
    assert_eq!(report.change.required_bump(), Bump::Compatible);
    assert_eq!(report.minimal_wire(), Some(WireVersion::new(2, 4)));
    assert_eq!(report.wire_reasons, ["Api::led_off added"]);
    assert_eq!(verdicts(&report), (Ok(()), Ok(())));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        report.required(),
        "wire w2.3 -> w2.4 or later (a new wire revision), crate 0.1.0 -> 0.1.1 or later (compatible position \
         (patch before 1.0, minor after))"
    );

    // the wire version not bumped: the wire check fails on its own, the crate one passes
    let report = bumps(OLD, &added, "0.1.1");
    let (wire, version) = verdicts(&report);
    assert_eq!(version, Ok(()));
    let e = wire.unwrap_err();
    assert_eq!(
        e,
        "test_api wire w2.3 -> w2.3 has Api::led_off added, which needs a new wire revision: w2.4 or later.\n\
         Api::led_off has #[since = \"w2.4\"], newer than the API's wire version w2.3"
    );
    // a new field of an own type is a revision too
    let field = OLD.replace(
        "pub b: u16,\n}",
        "pub b: u16,\n#[since = \"w2.4\"]\n#[default = None]\npub c: Option<u8>,\n}",
    );
    let report = bumps(OLD, &at(&field, "w2.4"), "0.1.1");
    assert_eq!(report.wire_bump, WireBump::Revision, "{report:#?}");
    assert_eq!(report.wire_reasons, ["Config: field `c` added"]);
}

/// The table row: a trailing argument with a default is a new revision and a crate breaking bump.
#[test]
fn trailing_argument_is_a_revision_and_a_breaking_bump() {
    let new = OLD.replace(
        "fn configure(a: u8, b: u16);",
        "fn configure(a: u8, b: u16, #[default = 5] c: u8);",
    );
    let report = bumps(OLD, &at(&new, "w2.4"), "0.2.0");
    assert_eq!(report.wire_bump, WireBump::Revision, "{report:#?}");
    assert_eq!(report.change.required_bump(), Bump::Breaking);
    assert_eq!(
        report.wire_reasons,
        ["Api::configure: arguments added or removed at the end"]
    );
    assert_eq!(verdicts(&report), (Ok(()), Ok(())));

    // a crate patch is not enough, the wire is fine
    let report = bumps(OLD, &at(&new, "w2.4"), "0.1.1");
    let (wire, version) = verdicts(&report);
    assert_eq!(wire, Ok(()));
    assert!(version.unwrap_err().contains("0.2.0 or later"));

    // the crate bumped, the wire version not: only the wire check fails, asking for a revision, not a line
    let report = bumps(OLD, &new, "0.2.0");
    let (wire, version) = verdicts(&report);
    assert_eq!(version, Ok(()));
    assert!(
        wire.unwrap_err()
            .ends_with("which needs a new wire revision: w2.4 or later")
    );
}

/// The table row: a wire break is a new line and a crate breaking bump.
#[test]
fn wire_break_is_a_line_and_a_breaking_bump() {
    let new = OLD.replace(
        "fn configure(a: u8, b: u16);",
        "fn configure(a: u8, b: u32);",
    );
    let report = bumps(OLD, &at(&new, "w3.0"), "0.2.0");
    assert_eq!(report.wire_bump, WireBump::Line, "{report:#?}");
    assert_eq!(report.change.required_bump(), Bump::Breaking);
    assert_eq!(report.minimal_wire(), Some(WireVersion::new(3, 0)));
    assert_eq!(verdicts(&report), (Ok(()), Ok(())));
    assert!(
        report
            .required()
            .starts_with("wire w2.3 -> w3.0 or later (a new wire line), crate 0.1.0 -> 0.2.0"),
        "{}",
        report.required()
    );

    // a new revision is not enough for a break
    let report = bumps(OLD, &at(&new, "w2.4"), "0.2.0");
    let e = report.check_wire().unwrap_err();
    assert!(
        e.starts_with("test_api wire w2.3 -> w2.4 has a wire break (Api::configure:"),
        "{e}"
    );
    assert!(
        e.ends_with("which needs a new wire line: w3.0 or later"),
        "{e}"
    );

    // a removal, too
    let removed = OLD.replace("    fn led_on();\n", "");
    let report = bumps(OLD, &at(&removed, "w3.0"), "0.2.0");
    assert_eq!(report.wire_bump, WireBump::Line, "{report:#?}");
}

/// A break of Rust code only (a rename, a `#[flatten]` promotion) needs a crate breaking bump and no wire bump at
/// all.
#[test]
fn rust_only_breaks_need_no_wire_bump() {
    let renamed = OLD.replace("fn led_on();", "fn light_on();");
    let flattened = OLD.replace(
        "fn configure(a: u8, b: u16);",
        "fn configure(#[flatten] config: Config);",
    );
    for new in [renamed, flattened] {
        let report = bumps(OLD, &new, "0.2.0");
        assert_eq!(report.change, Change::Breaking, "{report:#?}");
        assert!(report.breaking.iter().all(|f| f.rule.keeps_wire()));
        assert_eq!(report.wire_bump, WireBump::None, "{report:#?}");
        assert_eq!(verdicts(&report), (Ok(()), Ok(())));
        assert!(
            report
                .required()
                .starts_with("wire w2.3 (no wire bump), crate")
        );
    }
    // doc comments only: a crate patch, no wire bump
    let docs = OLD.replace("fn led_on();", "/// Lights up\nfn led_on();");
    let report = bumps(OLD, &docs, "0.1.1");
    assert_eq!(report.change, Change::DocsOnly);
    assert_eq!(report.wire_bump, WireBump::None);
    assert_eq!(verdicts(&report), (Ok(()), Ok(())));
}

/// A new line by hand for a change of meaning, with nothing the checker can see, is accepted; so is a revision
/// nothing needs.
#[test]
fn line_bumped_by_hand_is_accepted() {
    for wire in ["w3.0", "w2.4", "w7.1"] {
        let report = bumps(OLD, &at(OLD, wire), "0.2.0");
        assert_eq!(report.wire_bump, WireBump::None, "{report:#?}");
        assert_eq!(report.check_wire(), Ok(()), "{wire}");
    }
    // a new line also covers an addition
    let added = OLD.replace(
        "fn samples(rate: u16);",
        "fn samples(rate: u16);\n#[since = \"w3.0\"]\nfn led_off();",
    );
    let report = bumps(OLD, &at(&added, "w3.0"), "0.2.0");
    assert_eq!(report.wire_bump, WireBump::Revision);
    assert_eq!(verdicts(&report), (Ok(()), Ok(())));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

/// A wire version going back, or no longer declared, fails; one declared for the first time has nothing to be
/// compared with.
#[test]
fn wire_version_going_back_or_dropped_fails() {
    let report = bumps(OLD, &at(OLD, "w2.2"), "0.1.1");
    assert_eq!(
        report.check_wire(),
        Err("test_api wire version w2.2 is older than w2.3".to_string())
    );
    let undeclared = OLD.replace("#[ww_api_root(wire = \"w2.3\")]", "#[ww_api_root]");
    let report = bumps(OLD, &undeclared, "0.1.1");
    assert_eq!(
        report.check_wire(),
        Err("test_api declared wire version w2.3, the new version declares none".to_string())
    );
    let report = bumps(&undeclared, OLD, "0.1.1");
    assert_eq!(report.check_wire(), Ok(()));
    assert_eq!(report.minimal_wire(), None);
    // no wire version on either side: the bump is still derived, nothing is checked
    let added = undeclared.replace(
        "fn samples(rate: u16);",
        "fn samples(rate: u16);\nfn led_off();",
    );
    let report = bumps(&undeclared, &added, "0.1.1");
    assert_eq!(report.wire_bump, WireBump::Revision);
    assert_eq!(report.check_wire(), Ok(()));
    assert!(
        report
            .required()
            .starts_with("no wire version declared (a new wire revision if it were), crate"),
        "{}",
        report.required()
    );
}

/// A `#[since]` naming a revision above the root's wire version fails the wire check, whatever the bump.
#[test]
fn since_above_the_root_fails() {
    let added = OLD.replace(
        "fn samples(rate: u16);",
        "fn samples(rate: u16);\n#[since = \"w2.5\"]\nfn led_off();",
    );
    let report = bumps(OLD, &at(&added, "w2.4"), "0.1.1");
    assert_eq!(report.wire_bump, WireBump::Revision);
    assert_eq!(
        report.check_wire(),
        Err(
            "Api::led_off has #[since = \"w2.5\"], newer than the API's wire version w2.4"
                .to_string()
        )
    );
}
