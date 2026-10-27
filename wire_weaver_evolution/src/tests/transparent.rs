//! EVO-15: transparent wrappers (DER-18) in the checker, with a wrapper of this test's own in the shape of
//! `ww_si::Fixed<U, P, T, B>`: markers for the unit and prefix, bound markers named as ww_si's.

use super::*;
use crate::compat::compare_items;

/// The wrapper, its markers, and an API using `arg` for a method argument (the host writes it), `prop` for a
/// read-only property (the device writes it) and `field` for a struct field.
fn api(arg: &str, prop: &str, field: &str) -> String {
    format!(
        r#"
        use core::marker::PhantomData;

        pub struct Second;
        pub struct Meter;
        pub struct Micro;
        pub struct Milli;
        pub struct Unbounded;
        pub struct RangeU64<const MIN: u64, const MAX: u64>;
        pub struct RangeDec<const MIN: i64, const MAX: i64, const EXP: i8>;

        #[derive_shrink_wrap(transparent)]
        pub struct Fixed<U, P, T, B> {{
            pub value: T,
            pub _m: PhantomData<(U, P, B)>,
        }}

        /// Another wrapper of the same shape, with another meaning.
        #[derive_shrink_wrap(transparent)]
        pub struct Count<T> {{
            pub value: T,
        }}

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

const MICROS: &str = "Fixed<Second, Micro, u64, Unbounded>";

fn assert_rule(report: &Report, rule: Rule, contains: &str) {
    assert_eq!(report.change, Change::Breaking, "{report:?}");
    assert!(
        report
            .breaking
            .iter()
            .any(|f| f.rule == rule && f.contains(contains)),
        "no {rule:?} finding with '{contains}' in {:#?}",
        report.breaking
    );
}

/// `T` <-> a transparent wrapper of `T`: the same bytes, compatible both ways, and reported as such.
#[test]
fn plain_to_wrapper_and_back_is_compatible() {
    let plain = same("u64");
    let wrapped = same(MICROS);
    for (old, new, from, to) in [
        (&plain, &wrapped, "u64", MICROS),
        (&wrapped, &plain, MICROS, "u64"),
    ] {
        let report = report(old, new);
        assert_eq!(report.change, Change::Compatible, "{report:#?}");
        for path in [
            "Api::set: argument `shared`",
            "Api::uptime",
            "type Sample: field `at`",
        ] {
            let expected = format!("{path}: {from} -> {to}, same bytes");
            assert!(
                report.compatible.contains(&expected),
                "no '{expected}' in {:#?}",
                report.compatible
            );
        }
    }
    // nested: inside an Option and a Vec
    for outer in ["Option", "Vec"] {
        let r = report(
            &same(&format!("{outer}<u64>")),
            &same(&format!("{outer}<{MICROS}>")),
        );
        assert_eq!(r.change, Change::Compatible, "{r:#?}");
    }
}

/// A wrapper's unit or prefix changed: the same bytes, but a reader would take µs for ms.
#[test]
fn unit_or_prefix_changed_is_breaking() {
    let old = same(MICROS);
    let report_ = report(&old, &same("Fixed<Second, Milli, u64, Unbounded>"));
    assert_rule(
        &report_,
        Rule::Meaning,
        "marker Micro on the old version, Milli on the new version",
    );
    for path in ["Api::set", "Api::uptime", "Sample"] {
        assert!(
            report_
                .breaking
                .iter()
                .any(|f| f.rule == Rule::Meaning && f.message.contains(path)),
            "{path} not in {:#?}",
            report_.breaking
        );
    }
    assert_rule(
        &report(&old, &same("Fixed<Meter, Micro, u64, Unbounded>")),
        Rule::Meaning,
        "marker Second on the old version, Meter on the new version",
    );
    // one place is enough
    assert_rule(
        &report(
            &old,
            &api(MICROS, MICROS, "Fixed<Second, Milli, u64, Unbounded>"),
        ),
        Rule::Meaning,
        "field `at`",
    );
    // another wrapper of the same bytes
    assert_rule(
        &report(&old, &same("Count<u64>")),
        Rule::Meaning,
        "wrappers of the same bytes with another meaning",
    );
}

/// A changed `T` breaks as it does for plain numbers, inside a wrapper or across one.
#[test]
fn changed_storage_is_breaking() {
    let micros32 = same("Fixed<Second, Micro, u32, Unbounded>");
    assert_breaking(
        &report(&micros32, &same(MICROS)),
        "u32 on the old version, u64",
    );
    assert_breaking(
        &report(&same("u32"), &same(MICROS)),
        "u32 on the old version, u64",
    );
    assert_breaking(&report(&micros32, &same("u64")), "u32");
    assert_eq!(report(&same("u32"), &same("u64")).change, Change::Breaking);
}

/// A bound narrowed breaks the old writers (their values are refused), widened breaks the old readers (they refuse
/// the new values); the finding says which side writes the item: the host an argument, the device a property.
#[test]
fn bound_narrowed_or_widened_is_breaking() {
    let bounded = |min: u64, max: u64| format!("Fixed<Second, Micro, u64, RangeU64<{min}, {max}>>");
    let old = same(&bounded(0, 300));
    assert_eq!(report(&old, &old).change, Change::None);

    // the property: written by the device
    let widened = api(&bounded(0, 300), &bounded(0, 600), &bounded(0, 300));
    let report_ = report(&old, &widened);
    assert_rule(
        &report_,
        Rule::Bound,
        "Api::uptime: B of Fixed: bound 0..=600 on the new version, 0..=300 on the old version: values the new version \
         writes outside 0..=300 are refused by the old version",
    );
    assert_eq!(report_.breaking.len(), 1, "{:#?}", report_.breaking);
    let narrowed = api(&bounded(0, 300), &bounded(10, 300), &bounded(0, 300));
    assert_rule(
        &report(&old, &narrowed),
        Rule::Bound,
        "Api::uptime: B of Fixed: bound 0..=300 on the old version, 10..=300 on the new version: values the old version \
         writes outside 10..=300 are refused by the new version",
    );

    // the argument: written by the host
    let narrowed = api(&bounded(0, 100), &bounded(0, 300), &bounded(0, 300));
    assert_rule(
        &report(&old, &narrowed),
        Rule::Bound,
        "Api::set: argument `shared`: B of Fixed: bound 0..=300 on the old version, 0..=100 on the new \
         version",
    );
    // a struct field
    assert_rule(
        &report(
            &old,
            &api(&bounded(0, 300), &bounded(0, 300), &bounded(0, 301)),
        ),
        Rule::Bound,
        "values the new version writes outside 0..=300 are refused by the old version",
    );

    // Unbounded is wider than any range, a plain value as well
    assert_rule(
        &report(&old, &same(MICROS)),
        Rule::Bound,
        "bound Unbounded on the new version, 0..=300 on the old version",
    );
    assert_rule(
        &report(&same("u64"), &old),
        Rule::Bound,
        "bound Unbounded on the old version, 0..=300 on the new version",
    );
    assert_rule(
        &report(&old, &same("u64")),
        Rule::Bound,
        "bound Unbounded on the new version, 0..=300 on the old version",
    );
}

/// Decimal bounds (`RangeDec<MIN, MAX, EXP>` for floats) are compared by value, not by their digits.
#[test]
fn decimal_bounds_compare_by_value() {
    let dec = |min: i64, max: i64, exp: i8| {
        format!("Fixed<Second, One, f32, RangeDec<{min}, {max}, {exp}>>")
    };
    let with_one =
        |api: String| api.replace("pub struct Micro;", "pub struct Micro;\npub struct One;");
    let old = with_one(same(&dec(-5, 25, -1)));
    // -0.5..=2.5 written differently: same bounds, other definitions (compatible)
    let report_ = report(&old, &with_one(same(&dec(-50, 250, -2))));
    assert!(report_.breaking.is_empty(), "{:#?}", report_.breaking);
    assert_rule(
        &report(&old, &with_one(same(&dec(-5, 24, -1)))),
        Rule::Bound,
        "bound -0.5..=2.5 on the old version, -0.5..=2.4 on the new version",
    );
}

/// The same checks on bundles prepared as a client prepares a device's (instances turned into structs), as
/// `wire_weaver_client` compares its API with a connected device's.
#[test]
fn prepared_bundles() {
    let prepared = |src: &str| {
        let mut bundle = load("0.1.0", src);
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
            compare_items(
                (&client, c, "client"),
                (&device, d, "device"),
                &mut Bounds::default(),
            )?;
        }
        Ok(())
    };
    let micros = same(MICROS);
    check(&micros, &micros).unwrap();
    check(&same("u64"), &micros).unwrap();
    check(&micros, &same("u64")).unwrap();
    let e = check(&micros, &same("Fixed<Second, Milli, u64, Unbounded>")).unwrap_err();
    assert!(
        e.contains("marker Micro on the client, Milli on the device"),
        "{e}"
    );
    let e = check(
        &same("Fixed<Second, Micro, u64, RangeU64<0, 300>>"),
        &same("Fixed<Second, Micro, u64, RangeU64<0, 600>>"),
    )
    .unwrap_err();
    assert!(
        e.contains("values the device writes outside 0..=300 are refused by the client"),
        "{e}"
    );
}

/// The `transparent` flag survives a snapshot: written as `ww api save` writes it, read back, compared with the
/// source unchanged.
#[test]
fn snapshot_ron_round_trip() {
    let source = load("0.1.0", &same(MICROS));
    let text = wire_weaver_snapshots::render_ron(&source).unwrap();
    assert!(text.contains("transparent: true"), "{text}");
    let (read, recorded) = Recorded::parse(&text).unwrap();
    assert_eq!(bytes(&read, true).unwrap(), bytes(&source, true).unwrap());
    assert!(
        recorded.unrecorded().is_empty(),
        "{:?}",
        recorded.unrecorded()
    );
    let report = compare_recorded(&read, &recorded, &source, &Recorded::default()).unwrap();
    assert_eq!(report.change, Change::None, "{report:#?}");
    // and the compatible change from a plain field, against the saved snapshot
    let plain = load("0.1.0", &same("u64"));
    let (plain, recorded) =
        Recorded::parse(&wire_weaver_snapshots::render_ron(&plain).unwrap()).unwrap();
    let report = compare_recorded(&plain, &recorded, &source, &Recorded::default()).unwrap();
    assert_eq!(report.change, Change::Compatible, "{report:#?}");
}
