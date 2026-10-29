//! EVO-15 with the real `ww_si::Fixed<U, P, T, B>` (STD-55), read from ww_si's source as an API crate depending on it
//! sees it.

use super::*;

fn ww_si_dep() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ww_stdlib/ww_si");
    format!("ww_si = {{ path = {:?} }}\n", path.canonicalize().unwrap())
}

/// An API using `ty` for a method argument (the host writes it), a read-only property (the device writes it) and a
/// struct field.
fn api(arg: &str, prop: &str, field: &str) -> String {
    format!(
        r#"
        use ww_si::{{Duty16, Fixed, Micros, Millis, Range, RangeU64, RangeDec, Unbounded, Quantity}};
        use ww_si::prefix::{{Micro, Milli, One, Pow10, Pow2, Product, Quotient, Rational}};
        use ww_si::units;
        use ww_si::prefix;
        use ww_si::units::{{Second, Meter, Ratio}};

        #[ww_api_root]
        pub trait Api {{
            fn set(shared: {arg});
            property!(ro uptime: {prop});
        }}

        #[derive_shrink_wrap]
        pub struct Sample {{
            pub at: {field},
        }}
    "#
    )
}

fn same(ty: &str) -> String {
    api(ty, ty, ty)
}

/// `src` loaded as an API depending on ww_si: each source once per test thread, a load walks ww_si's and
/// exact_units' source (~0.1 s).
fn load(src: &str) -> ApiBundleOwned {
    thread_local! {
        static LOADED: std::cell::RefCell<std::collections::HashMap<String, ApiBundleOwned>> =
            Default::default();
    }
    LOADED.with_borrow_mut(|loaded| {
        loaded
            .entry(src.to_string())
            .or_insert_with(|| load_with("0.1.0", src, &ww_si_dep()))
            .clone()
    })
}

fn report(old: &str, new: &str) -> Report {
    compare(&load(old), &load(new)).unwrap()
}

fn assert_rule(report: &Report, rule: Rule, contains: &str) {
    assert_eq!(report.change, Change::Breaking, "{report:#?}");
    assert!(
        report
            .breaking
            .iter()
            .any(|f| f.rule == rule && f.contains(contains)),
        "no {rule:?} finding with '{contains}' in {:#?}",
        report.breaking
    );
}

const MICROS: &str = "Fixed<units::Second, prefix::Micro, u64, Unbounded>";

/// `shared_us: u64` -> `shared: Fixed<Second, Micro, u64>`: the same bytes, compatible both ways, reported as such.
/// `Fixed` comes from ww_si's snapshot (the API sees it skipped), so this also checks the definition is found there.
#[test]
fn plain_to_fixed_and_back_is_compatible() {
    let (plain, fixed) = (same("u64"), same("Fixed<Second, Micro, u64>"));
    let report_ = report(&plain, &fixed);
    assert_eq!(report_.change, Change::Compatible, "{report_:#?}");
    assert!(
        report_.compatible.contains(&format!(
            "Api::set: argument `shared`: u64 -> {MICROS}, same bytes"
        )),
        "{:#?}",
        report_.compatible
    );
    assert_eq!(report(&fixed, &plain).change, Change::Compatible);
    assert_eq!(
        report(&same("u64"), &same("Micros<u64>")).change,
        Change::Compatible
    );
}

/// D13: `B` is in the model on every instance, `Unbounded` too, so the three-argument form and the four-argument one
/// with `Unbounded` (and an alias of either) are the same type.
#[test]
fn default_bound_is_the_same_type() {
    let three = same("Fixed<Second, Micro, u64>");
    for other in ["Fixed<Second, Micro, u64, Unbounded>", "Micros<u64>"] {
        assert_eq!(report(&three, &same(other)).change, Change::None, "{other}");
    }
}

/// A unit or prefix changed: the same bytes, but a reader takes µs for ms.
#[test]
fn unit_or_prefix_changed_is_breaking() {
    let old = same("Fixed<Second, Micro, u64>");
    assert_rule(
        &report(&old, &same("Fixed<Second, Milli, u64>")),
        Rule::Meaning,
        "Api::set: argument `shared`: P of Fixed: marker prefix::Micro on the old version, prefix::Milli on the new \
         version: the same bytes in another scale, 1 on the old version is 0.001 (exactly 1·10⁻³) on the new \
         version, which would read it as 1",
    );
    assert_rule(
        &report(&old, &same("Millis<u64>")),
        Rule::Meaning,
        "marker prefix::Micro on the old version, prefix::Milli",
    );
    assert_rule(
        &report(&old, &same("Fixed<Meter, Micro, u64>")),
        Rule::Meaning,
        "U of Fixed: marker units::Second on the old version, units::Meter on the new version: s vs m, another \
         dimension",
    );
}

/// A changed `T` breaks as it does for plain numbers.
#[test]
fn changed_storage_is_breaking() {
    assert_breaking(
        &report(
            &same("Fixed<Second, Milli, u16>"),
            &same("Fixed<Second, Milli, u32>"),
        ),
        "argument 2 of Fixed: u16 on the old version, u32 on the new version",
    );
    assert_breaking(
        &report(&same("u16"), &same("Fixed<Second, Milli, u32>")),
        "u16 on the old version, u32 on the new version",
    );
}

/// `Range<MIN, MAX>` (read as ww_si's, not as `core::ops::Range`) narrowed or widened: breaking for the side that
/// refuses, named by who writes the item.
#[test]
fn bound_changed_is_breaking() {
    let bounded = |min: i64, max: i64| format!("Fixed<Ratio, One, i16, Range<{min}, {max}>>");
    let old = same(&bounded(-5, 300));
    assert_eq!(report(&old, &old).change, Change::None);
    // the property, written by the device
    let report_ = report(
        &old,
        &api(&bounded(-5, 300), &bounded(-5, 600), &bounded(-5, 300)),
    );
    assert_rule(
        &report_,
        Rule::Bound,
        "Api::uptime: B of Fixed: bound -5..=600 on the new version, -5..=300 on the old version: values the new \
         version writes outside -5..=300 are refused by the old version",
    );
    assert_eq!(report_.breaking.len(), 1, "{:#?}", report_.breaking);
    // the argument, written by the host
    assert_rule(
        &report(
            &old,
            &api(&bounded(0, 300), &bounded(-5, 300), &bounded(-5, 300)),
        ),
        Rule::Bound,
        "Api::set: argument `shared`: B of Fixed: bound -5..=300 on the old version, 0..=300 on the new version: \
         values the old version writes outside 0..=300 are refused by the new version",
    );
    // a bound added to a plain value or to an unbounded Fixed
    assert_rule(
        &report(&same("i16"), &old),
        Rule::Bound,
        "bound Unbounded on the old version, -5..=300 on the new version",
    );
    assert_rule(
        &report(&same("Fixed<Ratio, One, i16>"), &old),
        Rule::Bound,
        "B of Fixed: bound Unbounded on the old version, -5..=300 on the new version",
    );
}

/// `Quantity` -> `Fixed` is the intended move, and breaking: the finding says both.
#[test]
fn quantity_to_fixed_is_breaking_with_a_hint() {
    assert_breaking(
        &report(
            &same("Quantity<Second>"),
            &same("Fixed<Second, Milli, u32>"),
        ),
        "Api::set: argument `shared`: Quantity<units::Second, Unbounded> on the old version, Fixed<units::Second, \
         prefix::Milli, u32, Unbounded> on the new version: a Quantity sends its prefix and number type with every \
         value, a Fixed only the number: moving to Fixed is intended, but needs a major version",
    );
}

/// A client compares its API with a device's after putting skipped definitions back and preparing both.
#[test]
fn prepared_bundles() {
    let prepared = |src: &str| {
        let mut bundle = load_with("0.1.0", src, &ww_si_dep());
        // markers come back in-line with their definitions (INT-13), still compared by crate and name
        let not_inlined = wire_weaver_snapshots::inline_skipped(&mut bundle);
        assert!(
            not_inlined.iter().all(|n| !n.name.ends_with("Fixed")),
            "{not_inlined:?}"
        );
        ww_self::generics::prepare(&mut bundle).unwrap();
        bundle
    };
    let check = |client: &str, device: &str| -> Result<(), String> {
        let (client, device) = (prepared(client), prepared(device));
        let items = |bundle: &ApiBundleOwned| match &bundle.traits[0] {
            ApiLevelLocationOwned::InLine { level, .. } => level.items.clone(),
            _ => panic!("not in line"),
        };
        for (c, d) in items(&client).iter().zip(&items(&device)) {
            crate::compat::compare_items(
                (&client, c, "client"),
                (&device, d, "device"),
                &mut Bounds::default(),
            )?;
        }
        Ok(())
    };
    let micros = same("Micros<u64>");
    check(&micros, &micros).unwrap();
    check(&same("u64"), &micros).unwrap();
    let e = check(&micros, &same("Millis<u64>")).unwrap_err();
    assert!(
        e.contains("marker Micro on the client, Milli on the device"),
        "{e}"
    );
    let e = check(
        &same("Fixed<Ratio, One, i16, Range<-5, 300>>"),
        &same("Fixed<Ratio, One, i16, Range<-5, 600>>"),
    )
    .unwrap_err();
    assert!(
        e.contains("values the device writes outside -5..=300 are refused by the client"),
        "{e}"
    );
}

/// Scales without a name (`Pow10<E>`, `Rational<NUM, DEN>`) are markers with const arguments. Scales are compared
/// by the exact factor they stand for (EVO-17): another factor is breaking.
#[test]
fn exact_scales_of_another_value_are_breaking() {
    for (old, new, marker) in [
        (
            "Fixed<Second, Pow10<-4>, u32>",
            "Fixed<Second, Pow10<-3>, u32>",
            "marker prefix::Pow10<-4> on the old version, prefix::Pow10<-3> on the new version: the same bytes in \
             another scale, 1 on the old version is 0.1 (exactly 1·10⁻¹) on the new version",
        ),
        (
            "Fixed<Ratio, Rational<1, 65535>, u16>",
            "Fixed<Ratio, Rational<1, 65536>, u16>",
            "marker prefix::Rational<1, 65535> on the old version, prefix::Rational<1, 65536>",
        ),
        (
            "Fixed<Second, Product<Milli, Milli>, u32>",
            "Fixed<Second, Quotient<Milli, Milli>, u32>",
            "marker prefix::Product<prefix::Milli, prefix::Milli> on the old version, \
             prefix::Quotient<prefix::Milli, prefix::Milli> on the new version",
        ),
    ] {
        assert_rule(&report(&same(old), &same(new)), Rule::Meaning, marker);
        assert_eq!(report(&same(new), &same(new)).change, Change::None, "{new}");
    }
}

/// Another name for the same scale is compatible, and reported as the same value (EVO-17, si-evo's finding).
#[test]
fn exact_scales_of_the_same_value_are_compatible() {
    for (old, new) in [
        ("Fixed<Second, Milli, u32>", "Fixed<Second, Pow10<-3>, u32>"),
        (
            "Fixed<Second, Milli, u32>",
            "Fixed<Second, Rational<1, 1000>, u32>",
        ),
        (
            "Fixed<Ratio, Pow2<-16>, u16>",
            "Fixed<Ratio, Rational<1, 65536>, u16>",
        ),
        // the scale of a product or quotient of two Fixed values (STD-83) by its factor
        (
            "Fixed<Second, Micro, u32>",
            "Fixed<Second, Product<Milli, Milli>, u32>",
        ),
        (
            "Fixed<Second, Pow10<3>, u32>",
            "Fixed<Second, Quotient<Milli, Micro>, u32>",
        ),
    ] {
        let report_ = report(&same(old), &same(new));
        assert_eq!(
            report_.change,
            Change::Compatible,
            "{old} -> {new}: {report_:#?}"
        );
        assert!(
            report_.compatible.iter().any(|c| c
                .starts_with("Api::set: argument `shared`: P of Fixed: ")
                && c.ends_with(", same value")),
            "{report_:#?}"
        );
    }
    assert_eq!(
        report(&same("u16"), &same("Duty16")).change,
        Change::Compatible
    );
}
