#![cfg(test)]
//! Every magnitude checked exactly: against what NIST SP 811 prints (equal where bold, within half a unit of the last
//! printed digit otherwise), against the literal values the sources state, and for the rules of the table (no bare
//! ambiguous name, units of a group all listed, dimensions). Ends with the bit sizes STD-62's magnitude choice needs.

use super::catalogue::*;
use super::{AMBIGUOUS, Exact, Mag, ORIGINS, Src, Status, UNITS, Unit, dim};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// Just enough of an unsigned big integer for exact comparisons: little-endian u32 limbs.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Big(Vec<u32>);

impl Big {
    fn from(mut v: u128) -> Big {
        let mut limbs = Vec::new();
        while v != 0 {
            limbs.push(v as u32);
            v >>= 32;
        }
        Big(limbs)
    }

    fn mul(&self, o: &Big) -> Big {
        let mut out = vec![0u64; self.0.len() + o.0.len() + 1];
        for (i, &a) in self.0.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in o.0.iter().enumerate() {
                let t = out[i + j] + a as u64 * b as u64 + carry;
                out[i + j] = t & 0xffff_ffff;
                carry = t >> 32;
            }
            let mut k = i + o.0.len();
            while carry != 0 {
                let t = out[k] + carry;
                out[k] = t & 0xffff_ffff;
                carry = t >> 32;
                k += 1;
            }
        }
        let mut limbs: Vec<u32> = out.into_iter().map(|l| l as u32).collect();
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        Big(limbs)
    }

    fn pow(&self, n: u32) -> Big {
        (0..n).fold(Big::from(1), |acc, _| acc.mul(self))
    }

    fn cmp(&self, o: &Big) -> Ordering {
        self.0
            .len()
            .cmp(&o.0.len())
            .then_with(|| self.0.iter().rev().cmp(o.0.iter().rev()))
    }
}

/// π to 30 digits, below and above.
const PI_LO: u128 = 314159265358979323846264338327;
const PI_HI: u128 = 314159265358979323846264338328;
const PI_EXP: u32 = 29;

/// A fraction `n / d`.
type Frac = (Big, Big);

/// Exact bounds of a magnitude: equal unless it has a power of π.
fn bounds(m: Mag) -> (Frac, Frac) {
    let ten = Big::from(10);
    let two = Big::from(2);
    let mut n = Big::from(m.num)
        .mul(&ten.pow(m.pow10.max(0) as u32))
        .mul(&two.pow(m.pow2.max(0) as u32));
    let mut d = Big::from(m.den)
        .mul(&ten.pow((-m.pow10).max(0) as u32))
        .mul(&two.pow((-m.pow2).max(0) as u32));
    let k = m.pi.unsigned_abs() as u32;
    let scale = ten.pow(PI_EXP * k);
    let (lo, hi) = (Big::from(PI_LO).pow(k), Big::from(PI_HI).pow(k));
    if m.pi >= 0 {
        d = d.mul(&scale);
        ((n.mul(&lo), d.clone()), (n.mul(&hi), d))
    } else {
        n = n.mul(&scale);
        ((n.clone(), d.mul(&hi)), (n, d.mul(&lo)))
    }
}

/// Compares `n / d` with `p · 10^e`.
fn cmp_dec((n, d): &Frac, p: u128, e: i32) -> Ordering {
    let ten = Big::from(10);
    let (l, r) = if e >= 0 {
        (n.clone(), Big::from(p).mul(&ten.pow(e as u32)).mul(d))
    } else {
        (n.mul(&ten.pow((-e) as u32)), Big::from(p).mul(d))
    };
    l.cmp(&r)
}

/// `mantissa E exp10` as digits and the power of ten of the last digit: "6.894757", 3 is 6894757 · 10⁻³.
fn printed(mantissa: &str, exp10: i16) -> (u128, i32) {
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits: u128 = format!("{int}{frac}").parse().unwrap();
    (digits, exp10 as i32 - frac.len() as i32)
}

/// The magnitude rounds to `mantissa E exp10` (half a unit of the last digit either way), or equals it.
fn rounds_to(m: Mag, mantissa: &str, exp10: i16, exact: bool) -> bool {
    let (digits, e) = printed(mantissa, exp10);
    let (lo, hi) = bounds(m);
    if exact {
        return m.pi == 0 && cmp_dec(&lo, digits, e) == Ordering::Equal;
    }
    // compare 2·value with (2·digits ± 1) · 10^e
    let twice = |(n, d): &Frac| (n.mul(&Big::from(2)), d.clone());
    cmp_dec(&twice(&lo), 2 * digits - 1, e) != Ordering::Less
        && cmp_dec(&twice(&hi), 2 * digits + 1, e) != Ordering::Greater
}

fn unit(id: &str) -> &'static Unit {
    UNITS
        .iter()
        .find(|u| u.id == id)
        .unwrap_or_else(|| panic!("no unit {id}"))
}

#[test]
fn magnitudes_are_canonical() {
    for u in UNITS {
        let m = u.mag;
        assert_eq!(Mag::new(m.num, m.den, m.pow10, m.pow2, m.pi), m, "{}", u.id);
    }
}

#[test]
fn every_unit_rounds_to_what_sp811_prints() {
    let mut checked = 0;
    for u in UNITS {
        let Some(p) = u.sp811 else { continue };
        assert!(
            rounds_to(u.mag, p.mantissa, p.exp10, p.bold),
            "{}: {:?} is not SP 811's {} E{} ({})",
            u.id,
            u.mag,
            p.mantissa,
            p.exp10,
            if p.bold { "exact" } else { "rounded" }
        );
        checked += 1;
    }
    assert!(
        checked >= 100,
        "only {checked} units checked against SP 811"
    );
}

#[test]
fn measured_units_are_sp811_values_as_printed() {
    for u in UNITS.iter().filter(|u| u.exact == Exact::Measured) {
        let p = u
            .sp811
            .unwrap_or_else(|| panic!("{}: a measured unit needs its printed value", u.id));
        assert!(rounds_to(u.mag, p.mantissa, p.exp10, true), "{}", u.id);
    }
}

#[test]
fn literals_the_sources_state() {
    let eq = |m: Mag, digits: u128, exp10: i16| assert_eq!(m, Mag::dec(digits, exp10), "{m:?}");
    // SP 811 footnote 23: lbf = 4.448 221 615 260 5 N exactly with g_n
    eq(unit("pound_force").mag, 44482216152605, -13);
    // SP 811 footnote 9: Btu_IT = 1.055 055 852 62 kJ
    eq(unit("btu_it").mag, 105505585262, -8);
    // SP 811 footnote 13: in³ = 1.638 706 4 E-05 m³
    eq(unit("cubic_inch").mag, 16387064, -12);
    // SP 811 footnote 22: lb = 4.535 923 7 E-01 kg
    eq(unit("pound_avdp").mag, 45359237, -8);
    // WMA 1985 Pt V: ounce troy = 0.031 103 476 8 kg; Pt IV: pint = 0.568 261 25 dm³
    eq(unit("ounce_troy").mag, 311034768, -10);
    eq(unit("pint_imperial").mag, 56826125, -11);
    // HB 44 §2: pint 28.875 in³, quart 57.75 in³; §2 dry: quart 67.2006 in³ (rounded there), peck 537.605 in³
    assert_eq!(unit("pint_us_liquid").mag, IN3.mul(Mag::dec(28875, -3)));
    assert_eq!(unit("quart_us_liquid").mag, IN3.mul(Mag::dec(5775, -2)));
    assert_eq!(unit("peck_us").mag, IN3.mul(Mag::dec(537605, -3)));
    // HB 44 §5: furlong 201.168 m, league 4.828 032 km, rod 5.0292 m, fathom 1.8288 m, mil 25.4 µm (all exact)
    eq(unit("furlong_international").mag, 201168, -3);
    eq(unit("league_international").mag, 4828032, -3);
    eq(unit("rod_international").mag, 50292, -4);
    eq(unit("fathom_international").mag, 18288, -4);
    eq(unit("thou").mag, 254, -7);
    // HB 44 §4 (1959): 1 international foot = 0.999 998 survey foot exactly
    assert_eq!(
        unit("foot_international")
            .mag
            .div(unit("foot_us_survey").mag),
        Mag::dec(999998, -6)
    );
    assert_eq!(
        unit("mile_international")
            .mag
            .div(unit("mile_us_survey").mag),
        Mag::dec(999998, -6)
    );
    // WMA 1985 Pt VI: mile 1760 yd, acre 4840 yd², fluid ounce 1/20 pint, bushel 8 gallons
    assert_eq!(unit("mile_international").mag, YD.times(1760));
    assert_eq!(unit("acre_international").mag, YD2.times(4840));
    // SP 811 bold: gon = 0.9°, NATO mil = 0.056 25°
    assert_eq!(unit("gon").mag, DEG.mul(Mag::dec(9, -1)));
    assert_eq!(unit("mil_nato").mag, DEG.mul(Mag::dec(5625, -5)));
    // SI 8 Table 6: 1′ = π/10 800 rad, 1″ = π/648 000 rad
    assert_eq!(unit("arcminute").mag, Mag::PI.over(10800));
    assert_eq!(unit("arcsecond").mag, Mag::PI.over(648000));
    // HB 44 §5 gives the barrels in in³ (exact) and dry quarts (rounded): 105 and 86 45/64
    let dry_qt = unit("quart_us_dry").mag;
    assert!(rounds_to(
        unit("barrel_dry").mag.div(dry_qt),
        "105.0",
        0,
        false
    ));
    assert!(rounds_to(
        unit("barrel_cranberry").mag.div(dry_qt),
        "86.7",
        0,
        false
    ));
    // the mechanical horsepower is SP 811's "horsepower (U.K.)", 745.70 W
    assert!(rounds_to(
        unit("horsepower_mechanical").mag,
        "7.4570",
        2,
        false
    ));
    // the metric horsepower is 75 kgf·m/s
    assert_eq!(
        unit("horsepower_metric").mag,
        unit("kilogram_force").mag.times(75)
    );
}

#[test]
fn chains_hold() {
    let m = |id| unit(id).mag;
    assert_eq!(m("gallon_us"), m("quart_us_liquid").times(4));
    assert_eq!(m("gallon_us"), m("fluid_ounce_us").times(128));
    assert_eq!(m("gallon_imperial"), m("fluid_ounce_imperial").times(160));
    assert_eq!(m("gallon_imperial"), m("pint_imperial").times(8));
    assert_eq!(m("bushel_us"), m("pint_us_dry").times(64));
    assert_eq!(m("pound_avdp"), m("grain").times(7000));
    assert_eq!(m("ounce_avdp"), m("grain").mul(Mag::dec(4375, -1)));
    assert_eq!(m("pound_troy"), m("pennyweight").times(240));
    assert_eq!(
        m("pound_apothecaries"),
        m("scruple_apothecaries").times(288)
    );
    assert_eq!(m("ton_long"), m("hundredweight_long").times(20));
    assert_eq!(m("ton_long"), m("stone").times(160));
    assert_eq!(m("mile_international"), m("chain_international").times(80));
    assert_eq!(m("chain_international"), m("link_international").times(100));
    assert_eq!(
        m("square_mile_international"),
        m("acre_international").times(640)
    );
    assert_eq!(m("pica_printers"), m("point_printers").times(12));
    assert_eq!(m("pica_dtp"), m("point_dtp").times(12));
    assert_eq!(
        m("horsepower_hour"),
        m("foot_pound_force").times(550 * 3600)
    );
    assert_eq!(m("quad"), m("btu_it").mul(Mag::dec(1, 15)));
    assert_eq!(m("turn"), m("degree_angle").times(360));
    assert_eq!(m("turn"), m("gon").times(400));
    assert_eq!(m("knot"), m("nautical_mile_international").div(HOUR));
}

#[test]
fn pound_mass_force_and_slug() {
    let (lbm, lbf, slug, pdl) = (
        unit("pound_avdp"),
        unit("pound_force"),
        unit("slug"),
        unit("poundal"),
    );
    let mass = dim(0, 0, 1, 0, 0);
    let force = dim(-2, 1, 1, 0, 0);
    // lbm and slug are both mass, 32.174.. lbm apart; lbf is a force: never one number for the other
    assert_eq!(
        (lbm.dim, slug.dim, lbf.dim, pdl.dim),
        (mass, mass, force, force)
    );
    assert_ne!(lbm.mag, slug.mag);
    // lbf = lbm · g_n = slug · ft/s²; poundal = lbm · ft/s²
    assert_eq!(lbf.mag, lbm.mag.mul(GN));
    assert_eq!(lbf.mag, slug.mag.mul(FT));
    assert_eq!(pdl.mag, lbm.mag.mul(FT));
    // slug / lbm = g_n / (ft/s²) = 32.174 048 556 4.. (not a whole number)
    assert_eq!(slug.mag.div(lbm.mag), GN.div(FT));
    assert!(rounds_to(GN.div(FT), "32.17405", 0, false));
    // kip = 1000 lbf, ozf = lbf/16
    assert_eq!(unit("kip").mag, lbf.mag.times(1000));
    assert_eq!(unit("ounce_force").mag.times(16), lbf.mag);
}

#[test]
fn temperature_origins() {
    let f = ORIGINS.iter().find(|o| o.id == "fahrenheit").unwrap();
    let r = ORIGINS.iter().find(|o| o.id == "rankine").unwrap();
    assert_eq!(unit(f.unit).mag, DEG_F);
    assert_eq!(unit(r.unit).mag, DEG_F);
    assert!(
        ORIGINS
            .iter()
            .all(|o| o.sources.iter().all(|c| c.0 == Src::Sp811))
    );
    assert_eq!(r.zero, None);
    // 32 °F = 273.15 K (0 °C) and 212 °F = 373.15 K: 32·5/9 + 45967/180 = 27315/100
    let zero = f.zero.unwrap();
    // t·5/9 + 45967/180 = (100 t + 45967)/180
    let k = |t: u128| (Big::from(100 * t + 45967), Big::from(180));
    assert_eq!(zero, Mag::ratio(45967, 180));
    assert_eq!(cmp_dec(&k(32), 27315, -2), Ordering::Equal);
    assert_eq!(cmp_dec(&k(212), 37315, -2), Ordering::Equal);
    // and the zero is 459.67 °R
    assert_eq!(zero.div(DEG_F), Mag::dec(45967, -2));
}

fn bare_id(bare: &str) -> String {
    bare.replace(' ', "_")
}

#[test]
fn ids_and_symbols_unique_and_sourced() {
    let mut ids = BTreeSet::new();
    let mut symbols = BTreeSet::new();
    for u in UNITS {
        assert!(ids.insert(u.id), "id {} twice", u.id);
        assert!(symbols.insert(u.symbol), "symbol {} twice", u.symbol);
        assert!(!u.sources.is_empty(), "{} has no source", u.id);
        assert!(!u.def.is_empty() && !u.name.is_empty(), "{}", u.id);
        assert!(
            u.id.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{}",
            u.id
        );
        if let Status::Legacy(why) = u.status {
            assert!(!why.is_empty(), "{}", u.id);
        }
    }
    for s in Src::CITED {
        assert!(s.url().starts_with("https://") && !s.title().is_empty());
        assert!(
            UNITS.iter().any(|u| u.sources.iter().any(|c| c.0 == s)),
            "{s:?} cited by no unit"
        );
    }
}

#[test]
fn no_bare_ambiguous_name() {
    let ids: BTreeSet<_> = UNITS.iter().map(|u| u.id).collect();
    let mut bare_names = BTreeSet::new();
    for a in AMBIGUOUS {
        assert!(bare_names.insert(a.bare), "group {} twice", a.bare);
        assert!(!a.note.is_empty());
        for u in UNITS {
            assert_ne!(
                u.id,
                bare_id(a.bare),
                "a unit is named by the bare {:?}",
                a.bare
            );
            assert!(
                !u.name.eq_ignore_ascii_case(a.bare),
                "a unit is named by the bare {:?}",
                a.bare
            );
            assert!(
                !a.symbols.contains(&u.symbol),
                "{} has the ambiguous symbol {:?}",
                u.id,
                u.symbol
            );
        }
        for m in a.members {
            assert!(ids.contains(m), "group {}: no unit {m}", a.bare);
        }
        // each unit named after the bare word, of a dimension the group has, is in the group
        let dims: Vec<_> = a.members.iter().map(|m| unit(m).dim).collect();
        let prefix = format!("{}_", bare_id(a.bare));
        for u in UNITS
            .iter()
            .filter(|u| u.id.starts_with(&prefix) && dims.contains(&u.dim))
        {
            assert!(
                a.members.contains(&u.id),
                "{} is missing from the group {:?}",
                u.id,
                a.bare
            );
        }
    }
    // the names Roman listed (27 Oct) all have a group
    for bare in [
        "gallon",
        "fluid ounce",
        "pint",
        "quart",
        "bushel",
        "foot",
        "mile",
        "ton",
        "ounce",
        "pound",
        "btu",
        "calorie",
        "horsepower",
        "mil",
        "inch of mercury",
        "millimetre of mercury",
    ] {
        assert!(bare_names.contains(bare), "no group for {bare:?}");
    }
}

#[test]
fn unsourced_definitions_are_deliberate() {
    // Definitions no fetched primary source states. SP 811 prints a value for most of them, which the tests above
    // check, but the defining relation itself is from ISO 80000, DIN 66036 or tradition: add to this list only
    // with a reason in the unit's `Unsourced` cite.
    let unsourced: Vec<_> = UNITS
        .iter()
        .filter(|u| u.unsourced())
        .map(|u| u.id)
        .collect();
    assert_eq!(
        unsourced,
        [
            "nautical_mile_uk_admiralty",
            "pica_printers",
            "circular_mil",
            "ton_register",
            "slug",
            "poundal",
            "inch_of_mercury_conventional",
            "foot_of_mercury_conventional",
            "millimetre_of_mercury_conventional",
            "torr",
            "foot_poundal",
            "horsepower_metric",
            "mil_warsaw_pact",
            "streck_swedish",
        ]
    );
    for u in UNITS.iter().filter(|u| u.unsourced()) {
        let why = u.sources.iter().find(|c| c.0 == Src::Unsourced).unwrap().1;
        assert!(!why.is_empty(), "{}", u.id);
    }
}

/// Prime factors by trial division; a cofactor above 10⁶² stays as one entry (none in this table so far).
fn factor(mut n: u128) -> Vec<(u128, u32)> {
    let mut out = Vec::new();
    let mut p = 3u128;
    while p * p <= n && p < 1_000_000 {
        let mut k = 0;
        while n.is_multiple_of(p) {
            n /= p;
            k += 1;
        }
        if k > 0 {
            out.push((p, k));
        }
        p += 2;
    }
    if n > 1 {
        out.push((n, 1));
    }
    out
}

fn show_factors(n: u128) -> String {
    if n == 1 {
        return "1".into();
    }
    factor(n)
        .iter()
        .map(|&(p, k)| {
            if k == 1 {
                p.to_string()
            } else {
                format!("{p}^{k}")
            }
        })
        .collect::<Vec<_>>()
        .join("·")
}

/// The input to STD-62's choice of magnitude: bits of num and den once 2 and 5 are out, the factors, and the
/// products and conversion factors a customary API meets. `cargo nextest run -p ww_si bit_sizes --no-capture`.
#[test]
fn bit_sizes() {
    let products: &[(&str, Mag)] = &[
        ("psi", LBF.div(IN2)),
        ("psi²", LBF.div(IN2).powi(2)),
        ("lbf·s", LBF),
        ("(lbf·s)²", LBF.powi(2)),
        ("ft·lbf", FT.mul(LBF)),
        ("hp (mech)", HP_MECH),
        ("Btu_IT/(lb·°F)", BTU_IT.div(LB).div(DEG_F)),
        ("slug·ft²", LBF.div(FT).mul(FT2)),
        ("Btu_IT → ft·lbf", BTU_IT.div(FT.mul(LBF))),
        ("gal US → gal imp", GAL_US.div(GAL_IMP)),
        ("hp mech → hp metric", HP_MECH.div(GN.times(75))),
        ("inHg (conv) → psi", IN.mul(HG).mul(GN).div(LBF.div(IN2))),
        ("lbf·s → N·s (MCO)", LBF),
        ("psi → inHg (conv)", LBF.div(IN2).div(IN.mul(HG).mul(GN))),
        ("lbf·ft → N·m · psi", FT.mul(LBF).mul(LBF.div(IN2))),
    ];
    let mut rows: Vec<(String, Mag)> = UNITS.iter().map(|u| (u.id.to_string(), u.mag)).collect();
    rows.extend(products.iter().map(|&(n, m)| (format!("= {n}"), m)));
    println!(
        "{:<36} {:>4} {:>4} {:>6} {:>5} {:>3}  num factors / den factors",
        "unit", "num", "den", "10^", "2^", "π^"
    );
    let mut max_unit = (0, 0);
    let mut largest_prime = 0u128;
    let mut primes = BTreeMap::<u128, u32>::new();
    for (i, (name, m)) in rows.iter().enumerate() {
        println!(
            "{:<36} {:>4} {:>4} {:>6} {:>5} {:>3}  {} / {}",
            name,
            m.num_bits(),
            m.den_bits(),
            m.pow10,
            m.pow2,
            m.pi,
            show_factors(m.num),
            show_factors(m.den)
        );
        for (p, _) in factor(m.num).into_iter().chain(factor(m.den)) {
            largest_prime = largest_prime.max(p);
            *primes.entry(p).or_default() += 1;
        }
        if i < UNITS.len() {
            max_unit = (max_unit.0.max(m.num_bits()), max_unit.1.max(m.den_bits()));
        }
    }
    println!(
        "max over units: num {} bits, den {} bits; largest prime {largest_prime}",
        max_unit.0, max_unit.1
    );
    println!(
        "distinct primes ({}): {:?}",
        primes.len(),
        primes.keys().collect::<Vec<_>>()
    );
    let bits = |m: Mag| (m.num_bits(), m.den_bits());
    // the numbers sent to STD-62 (27 Oct 2026); a change here is a change of that input
    assert_eq!(bits(LBF.div(IN2)), (44, 14), "psi");
    assert_eq!(bits(FT.mul(LBF)), (52, 1), "ft·lbf");
    assert_eq!(bits(HP_MECH).0, 56, "hp");
    assert_eq!(bits(LBF.div(IN2).powi(2)).0, 87, "psi²");
    // (lbf·s)³ no longer fits u128: its num and den are the cubes of lbf's (canonical, so no gcd to take out)
    let cube = |n: u128| {
        32 * Big::from(n).pow(3).0.len() as u32
            - Big::from(n).pow(3).0.last().unwrap().leading_zeros()
    };
    println!(
        "(lbf·s)³: num {} bits, den {} bits",
        cube(LBF.num),
        cube(LBF.den)
    );
    assert_eq!((cube(LBF.num), cube(LBF.den)), (130, 1), "(lbf·s)³");
    assert_eq!(max_unit, MAX_UNIT_BITS);
}

/// Largest num and den of a single unit, in bits after 2 and 5 are out.
const MAX_UNIT_BITS: (u32, u32) = (59, 36);

#[test]
fn every_unit_is_in_si_terms_of_its_dimension() {
    // angles are in radians (π powers only there), nothing else carries π but the circular mil
    for u in UNITS {
        let angle = u.dim == dim(0, 0, 0, 0, 1);
        assert!(u.mag.pi == 0 || angle || u.id == "circular_mil", "{}", u.id);
    }
}
