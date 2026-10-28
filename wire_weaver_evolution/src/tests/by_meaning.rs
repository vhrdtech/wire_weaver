//! EVO-17: units, scales and origins compared by what they mean, from the definitions the API model carries (INT-13),
//! not by their names. The same value under another name is compatible; the same bytes with another factor, zero or
//! period is breaking, also when nothing else changed (the Mars Climate Orbiter's lbf·s read as N·s).

use super::*;
use ww_self::ApiLevelLocationOwned;

fn ww_si_dep() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ww_stdlib/ww_si");
    format!(
        "ww_si = {{ path = {:?}, features = [\"customary\"] }}\n",
        path.canonicalize().unwrap()
    )
}

/// Units and origins of the API's own crate, declared as a user would (`unit!` / `origin!` lines the walker reads).
/// The pound-force is the catalogue's (STD-72). The inch of mercury is NIST SP 811's value by convention at 32 °F.
const OWN_MARKERS: &str = r#"
    ww_si::unit! {
        /// Inch of mercury at 32 °F, of the crate's own.
        pub struct OwnInHg = "inHg", [kg 1, m -1, s -2], Exact(338638 / 1, pow10 -2), conventional("NIST SP 811", 2008);
        /// Metres per second as a unit of its own.
        pub struct MeterPerSecond = "m/s", [m 1, s -1];
        /// Kilometres per hour: 1000 m / 3600 s.
        pub struct KmPerHour = "km/h", [m 1, s -1], Exact(5 / 18);
        /// Millivolts as a unit of their own.
        pub struct MilliVolt = "mV", [kg 1, m 2, s -3, a -1], Pow10(-3);
    }

    ww_si::origin! {
        /// The same zero as `Celsius`, under another name.
        pub struct OwnCelsius { unit: Kelvin, symbol: "°C", offset: Offset::decimal(27315, -2) }
    }

    ww_si::origin! {
        /// A register reading 0 at 25 °C.
        pub struct BoardTemp { above: Celsius, offset: Offset::int(25) }
    }
"#;

/// An API using `ty` for a method argument (the host writes it), a read-only property (the device writes it) and a
/// struct field; `markers` is added to the crate (its own unit and origin declarations).
fn api_with(ty: &str, markers: &str) -> String {
    format!(
        r#"
        use ww_si::{{Fixed, Offset, Point}};
        use ww_si::exact_units::{{Saturating, Wrapping}};
        use ww_si::origins::{{Angle, Celsius}};
        use ww_si::prefix::{{Centi, Milli, One, Pow10}};
        use ww_si::units::{{Coulomb, Degree, Joule, Kelvin, Meter, Newton, Pascal, Per, PoundForce, Radian, Second, Times, Volt}};

        #[ww_api_root]
        pub trait Api {{
            fn set(shared: {ty});
            property!(ro uptime: {ty});
        }}

        #[derive_shrink_wrap]
        pub struct Sample {{
            pub at: {ty},
        }}

        {markers}
    "#
    )
}

fn api(ty: &str) -> String {
    api_with(ty, OWN_MARKERS)
}

fn report_of(old: &str, new: &str) -> Report {
    let dep = ww_si_dep();
    compare(
        &load_with("0.1.0", old, &dep),
        &load_with("0.1.0", new, &dep),
    )
    .unwrap()
}

fn report(old: &str, new: &str) -> Report {
    report_of(&api(old), &api(new))
}

fn assert_meaning(report: &Report, contains: &str) {
    assert_eq!(report.change, Change::Breaking, "{report:#?}");
    assert!(
        report
            .breaking
            .iter()
            .any(|f| f.rule == Rule::Meaning && f.contains(contains)),
        "no Meaning finding with '{contains}' in {:#?}",
        report.breaking
    );
}

/// Compatible both ways, each use reported as the same value.
fn assert_same_value(old: &str, new: &str) {
    for (from, to) in [(old, new), (new, old)] {
        let report = report(from, to);
        assert_eq!(
            report.change,
            Change::Compatible,
            "{from} -> {to}: {report:#?}"
        );
        assert!(
            report
                .compatible
                .iter()
                .any(|c| c.starts_with("Api::set: argument `shared`: ")
                    && c.ends_with(", same value")),
            "{from} -> {to}: {:#?}",
            report.compatible
        );
    }
}

/// The Mars Climate Orbiter: impulse in lbf·s on one side, N·s on the other. The same bytes, a number 4.45 times too
/// small on the reader: breaking, with the exact factor in the finding.
#[test]
fn mars_climate_orbiter() {
    let lbf_s = "Fixed<Times<PoundForce, Second>, One, i32>";
    let n_s = "Fixed<Times<Newton, Second>, One, i32>";
    assert_eq!(report(lbf_s, lbf_s).change, Change::None);
    let report_ = report(lbf_s, n_s);
    assert_meaning(
        &report_,
        "Api::set: argument `shared`: U of Fixed: marker units::Times<units::PoundForce, units::Second> on the old \
         version, units::Times<units::Newton, units::Second> on the new version: the same bytes in another scale, 1 \
         on the old version is 4.448222 (exactly 1·",
    );
    // the property, written by the device: from the new version to the old one
    assert_meaning(
        &report_,
        "Api::uptime: U of Fixed: marker units::Times<units::Newton, units::Second> on the new version, \
         units::Times<units::PoundForce, units::Second> on the old version: the same bytes in another scale, 1 on the new \
         version is 0.224809 (exactly 1·",
    );
    // the same through the field of a type
    assert_meaning(&report_, "struct Sample: field `at`: U of Fixed: marker");
}

/// The Mars Climate Orbiter at connect time: a client in N·s with a device in lbf·s, both prepared as a client has
/// them. The client refuses the resource instead of reading the number as it is.
#[test]
fn mars_climate_orbiter_at_connect_time() {
    let prepared = |src: &str| {
        let mut bundle = load_with("0.1.0", src, &ww_si_dep());
        wire_weaver_snapshots::inline_skipped(&mut bundle);
        ww_self::generics::prepare(&mut bundle).unwrap();
        bundle
    };
    let check = |client: &str, device: &str| -> Result<(), String> {
        let (client, device) = (prepared(&api(client)), prepared(&api(device)));
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
    let e = check(
        "Fixed<Times<Newton, Second>, One, i32>",
        "Fixed<Times<PoundForce, Second>, One, i32>",
    )
    .unwrap_err();
    assert!(e.contains("the same bytes in another scale"), "{e}");
    assert!(
        e.contains("1 on the client is 0.224809") || e.contains("1 on the device is 4.448222"),
        "{e}"
    );
    // another spelling of the same unit connects
    check(
        "Fixed<Per<Meter, Second>, Milli, i32>",
        "Fixed<MeterPerSecond, Milli, i32>",
    )
    .unwrap();
}

/// A unit of the crate's own redefined under the same name: every use means something else, and so does the type.
#[test]
fn unit_redefined_under_the_same_name() {
    let ty = "Fixed<MilliVolt, One, i32>";
    let wrong = OWN_MARKERS.replace("a -1], Pow10(-3)", "a -1], Pow10(0)");
    let report_ = report_of(&api_with(ty, &wrong), &api(ty));
    assert_meaning(
        &report_,
        "Api::set: argument `shared`: U of Fixed: MilliVolt redefined: the same bytes in another scale, 1 on the old \
         version is 1000",
    );
    assert_meaning(
        &report_,
        "unit MilliVolt redefined: the same bytes in another scale",
    );
    // a new symbol alone is not a new meaning
    let renamed = OWN_MARKERS.replace("\"mV\"", "\"mV_\"");
    assert_eq!(
        report_of(&api_with(ty, &renamed), &api(ty)).change,
        Change::Compatible
    );
}

/// A unit fixed by a physical property (STD-72): its value by convention updated (a newer edition, other digits) is
/// the same unit, listed apart; a unit that was defined becoming one by convention, or back, is another meaning.
#[test]
fn conventional_units() {
    let ty = "Fixed<OwnInHg, Milli, i32>";
    let updated = OWN_MARKERS.replace(
        "Exact(338638 / 1, pow10 -2), conventional(\"NIST SP 811\", 2008)",
        "Exact(3386389 / 1, pow10 -3), conventional(\"NIST SP 811\", 2024)",
    );
    let report_ = report_of(&api(ty), &api_with(ty, &updated));
    assert_eq!(report_.change, Change::Compatible, "{report_:#?}");
    assert!(
        report_
            .compatible
            .iter()
            .any(|c| c
                == "type OwnInHg: 1 inHg = 3386.38 in SI, conventional (NIST SP 811, 2008) -> 1 inHg = 3386.389 \
                    in SI, conventional (NIST SP 811, 2024), conventional value updated"),
        "{:#?}",
        report_.compatible
    );
    let defined = OWN_MARKERS.replace(", conventional(\"NIST SP 811\", 2008)", "");
    assert_meaning(
        &report_of(&api(ty), &api_with(ty, &defined)),
        "unit OwnInHg redefined: conventional (NIST SP 811, 2008) on the old version, defined on the new version",
    );
    // another unit by convention is another unit
    assert_meaning(
        &report(ty, "Fixed<ww_si::units::InchOfMercury60f, Milli, i32>"),
        "the same bytes in another scale",
    );
}

/// Another spelling of the same unit, or of the same unit times scale: the same value.
#[test]
fn same_value_spelled_differently_is_compatible() {
    assert_same_value(
        "Fixed<Per<Meter, Second>, Milli, i32>",
        "Fixed<MeterPerSecond, Milli, i32>",
    );
    assert_same_value(
        "Fixed<Volt, Milli, i32>",
        "Fixed<Per<Joule, Coulomb>, Pow10<-3>, i32>",
    );
}

/// Unit and scale together: mV as a unit of its own, or V in steps of 10⁻³.
#[test]
fn unit_and_scale_compared_together() {
    assert_same_value("Fixed<Volt, Milli, i32>", "Fixed<MilliVolt, One, i32>");
}

/// The same dimension in another scale, or another dimension: breaking.
#[test]
fn another_scale_or_dimension_is_breaking() {
    assert_meaning(
        &report(
            "Fixed<Per<Meter, Second>, One, i32>",
            "Fixed<KmPerHour, One, i32>",
        ),
        "the same bytes in another scale, 1 on the old version is 3.6 (exactly 1·18/5) on the new version",
    );
    assert_meaning(
        &report("Fixed<Volt, Milli, i32>", "Fixed<MilliVolt, Milli, i32>"),
        "1 on the old version is 1000 (exactly 1·10³) on the new version",
    );
}

/// Another dimension in the same bytes: breaking.
#[test]
fn another_dimension_is_breaking() {
    assert_meaning(
        &report("Fixed<Joule, One, i32>", "Fixed<Newton, One, i32>"),
        "another dimension",
    );
}

/// Origins of points by where they lie: the same zero under another name is compatible, another zero or a reference
/// only known at run time is breaking. How a point is shown (its symbol) doesn't matter.
#[test]
fn origins_by_meaning() {
    let point = |origin: &str| format!("Point<{origin}, Fixed<Kelvin, Centi, i16>>");
    assert_same_value(&point("Celsius"), &point("OwnCelsius"));
    assert_meaning(
        &report(&point("Celsius"), &point("BoardTemp")),
        "O of Point: origin origins::Celsius on the old version, BoardTemp on the new version: another zero (273.15 K vs \
         298.15 K), the new version would read the points as other positions",
    );
    assert_meaning(
        &report(&point("Celsius"), &point("ww_si::origins::AbsoluteZero")),
        "another zero",
    );
}

/// An origin against another reference, or a point in another step: breaking.
#[test]
fn origins_reference_and_step() {
    let point = |origin: &str| format!("Point<{origin}, Fixed<Kelvin, Centi, i16>>");
    assert_meaning(
        &report(
            "Point<ww_si::origins::Vacuum, Fixed<ww_si::units::Pascal, One, i32>>",
            "Point<ww_si::origins::Gauge, Fixed<ww_si::units::Pascal, One, i32>>",
        ),
        "another reference (the unit's zero vs exact_units::AmbientPressure)",
    );
    // an origin and a temperature in another step: the step is the Fixed's, compared as a scale
    assert_meaning(
        &report(
            &point("Celsius"),
            "Point<Celsius, Fixed<Kelvin, Milli, i16>>",
        ),
        "P of Fixed: marker prefix::Centi on the old version, prefix::Milli",
    );
}

/// Angles: the same turn in degrees and in radians is the same origin (zero, period of one turn), so only the number's
/// own unit decides; an angle that wraps and a plain one are different things.
#[test]
fn angles_by_meaning() {
    assert_same_value(
        "Point<Angle<Degree>, Fixed<Degree, Centi, u16>>",
        "Point<Angle<Radian>, Fixed<Degree, Centi, u16>>",
    );
    assert_meaning(
        &report(
            "Point<Angle<Degree>, Fixed<Degree, Centi, u16>>",
            "Point<Celsius, Fixed<Degree, Centi, u16>>",
        ),
        "another dimension",
    );
}

/// `Wrapping` and `Saturating` (STD-63) in the place of a bound: a wrapping number on one side only is breaking,
/// saturating is the same values with the same meaning as unbounded. A plain number can become either.
#[test]
fn wrapping_and_saturating() {
    let fixed = |b: &str| format!("Fixed<Radian, Pow10<-4>, u16, {b}>");
    assert_meaning(
        &report(&fixed("ww_si::Unbounded"), &fixed("Wrapping")),
        "B of Fixed: Unbounded on the old version, Wrapping on the new version: a wrapping number is a position \
         modulo its storage",
    );
    let r = report(&fixed("ww_si::Unbounded"), &fixed("Saturating"));
    assert_eq!(r.change, Change::Compatible, "{r:#?}");
    for b in ["Wrapping", "Saturating"] {
        assert_eq!(report("u16", &fixed(b)).change, Change::Compatible, "{b}");
    }
}

/// A bound to `Saturating` is a bound widened.
#[test]
fn bound_to_saturating_is_widened() {
    let fixed = |b: &str| format!("Fixed<Radian, Pow10<-4>, u16, {b}>");
    let report_ = report(&fixed("ww_si::Range<0, 100>"), &fixed("Saturating"));
    assert_eq!(report_.change, Change::Breaking, "{report_:#?}");
}
