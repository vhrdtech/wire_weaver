//! EVO-22: the arguments of a method or stream function are the fields of an `Unsized` args struct on the wire, so
//! they evolve as trailing struct fields do; adding or promoting them changes the Rust signatures, a break of its own.

use super::*;

const OLD: &str = r#"
#[ww_api_root(wire = "w1.0")]
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

/// The only breaking findings are Rust ones of `rule` ([Rule::Signature] by default), the wire is compatible.
fn assert_rust_only(report: &Report, contains: &str) {
    assert_eq!(report.change, Change::Breaking, "{report:?}");
    assert!(
        report
            .breaking
            .iter()
            .all(|f| f.rule == Rule::Signature && f.rule.keeps_wire()),
        "{:?}",
        report.breaking
    );
    assert!(
        report.breaking.iter().any(|f| f.contains(contains)),
        "{:?} does not contain '{contains}'",
        report.breaking
    );
    // never the wire rule's text
    assert!(
        report
            .breaking
            .iter()
            .all(|f| !f.to_string().contains("both ways")),
        "{:?}",
        report.breaking
    );
}

#[test]
fn trailing_arguments_with_defaults_keep_the_wire() {
    // the blinky case: led_on had no arguments
    let option = OLD.replace("fn led_on();", "fn led_on(brightness: Option<u8>);");
    assert_rust_only(
        &report(OLD, &option),
        "Api::led_on: takes 0 argument(s) on the old version, 1 on the new version: the same bytes on the wire",
    );
    for added in [
        "fn configure(a: u8, b: u16, c: Option<u8>);",
        "fn configure(a: u8, b: u16, c: RefVec<'i, u8>);",
        "fn configure(a: u8, b: u16, c: &'i str);",
        "fn configure(a: u8, b: u16, #[default = 5] c: u8);",
        "fn configure(a: u8, b: u16, #[default = 5] c: u8, d: Option<u8>);",
    ] {
        let new = OLD.replace("fn configure(a: u8, b: u16);", added);
        assert_rust_only(&report(OLD, &new), "Api::configure: takes 2 argument(s)");
        // and back: a newer client's extra arguments are skipped, an older device's defaults read
        assert_rust_only(&report(&new, OLD), "Api::configure: takes");
    }
    // stream functions too
    let new = OLD.replace(
        "fn samples(rate: u16);",
        "fn samples(rate: u16, burst: Option<u8>);",
    );
    assert_rust_only(&report(OLD, &new), "Api::samples: takes 1 argument(s)");
}

#[test]
fn an_argument_without_a_default_breaks_the_wire() {
    let new = OLD.replace("fn led_on();", "fn led_on(brightness: u8);");
    let r = report(OLD, &new);
    assert_breaking(
        &r,
        "Api::led_on: argument `brightness` is not sent by the old version and has no #[default]",
    );
    assert_eq!(r.breaking[0].rule, Rule::Resource);
    assert!(!r.breaking[0].rule.keeps_wire());

    // an Option before an argument without a default has none either
    let new = OLD.replace(
        "fn configure(a: u8, b: u16);",
        "fn configure(a: u8, b: u16, c: Option<u8>, d: u8);",
    );
    assert_breaking(
        &report(OLD, &new),
        "argument `c` is not sent by the old version and has no #[default]",
    );

    // another type of an argument is a type change, named by the argument
    let new = OLD.replace(
        "fn configure(a: u8, b: u16);",
        "fn configure(a: u8, b: u32);",
    );
    assert_breaking(&report(OLD, &new), "Api::configure: argument `b`:");
}

#[test]
fn promoting_arguments_to_a_flattened_struct_keeps_the_wire() {
    let flat = OLD.replace(
        "fn configure(a: u8, b: u16);",
        "fn configure(#[flatten] cfg: Config);",
    );
    assert_rust_only(
        &report(OLD, &flat),
        "Api::configure: 2 argument(s) on the old version became the #[flatten] struct `cfg`",
    );
    assert_rust_only(
        &report(&flat, OLD),
        "Api::configure: the #[flatten] struct `cfg` of the old version became 2 argument(s)",
    );

    // a struct argument without #[flatten] is a real argument, other bytes
    let wrapped = OLD.replace("fn configure(a: u8, b: u16);", "fn configure(cfg: Config);");
    let r = report(OLD, &wrapped);
    assert_eq!(r.change, Change::Breaking);
    assert_eq!(r.breaking[0].rule, Rule::Resource, "{r:?}");
}

#[test]
fn a_flattened_struct_grows_like_any_struct() {
    let flat = OLD.replace(
        "fn configure(a: u8, b: u16);",
        "fn configure(#[flatten] cfg: Config);",
    );
    let grown = flat.replace(
        "    pub b: u16,\n}",
        "    pub b: u16,\n    #[default = None]\n    pub c: Option<u8>,\n}",
    );
    let r = report(&flat, &grown);
    assert_ne!(r.change, Change::Breaking, "{r:?}");
    // the same as adding the argument
    assert_rust_only(&report(OLD, &grown), "became the #[flatten] struct `cfg`");

    let without_default = flat.replace("    pub b: u16,\n}", "    pub b: u16,\n    pub c: u8,\n}");
    assert_breaking(
        &report(&flat, &without_default),
        "argument `c` is not sent by the old version and has no #[default]",
    );
}
