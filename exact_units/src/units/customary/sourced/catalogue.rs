//! The catalogue: defining constants, units, temperature origins and ambiguous names.
//!
//! A magnitude is built from the few relations the sources fix (the 1959 yard and pound, g_n, the US gallon in cubic
//! inches, the imperial gallon in litres, the IT and thermochemical calories) through the chains the sources state
//! (`1 rod = 16.5 ft`), so a unit's magnitude is its definition, not a number copied from a table. SP 811's printed
//! values are only used to check them (tests) and, for the `Measured` units, as the value itself.

use super::Src::*;
use super::{Ambiguous, Cite, Dim, Mag, Origin, Unit, dim};

const LEN: Dim = dim(0, 1, 0, 0, 0);
const AREA: Dim = dim(0, 2, 0, 0, 0);
const VOL: Dim = dim(0, 3, 0, 0, 0);
const MASS: Dim = dim(0, 0, 1, 0, 0);
const FORCE: Dim = dim(-2, 1, 1, 0, 0);
const PRESSURE: Dim = dim(-2, -1, 1, 0, 0);
const ENERGY: Dim = dim(-2, 2, 1, 0, 0);
const POWER: Dim = dim(-3, 2, 1, 0, 0);
const TEMP: Dim = dim(0, 0, 0, 1, 0);
const SPEED: Dim = dim(-1, 1, 0, 0, 0);
const ACCEL: Dim = dim(-2, 1, 0, 0, 0);
const ANGLE: Dim = dim(0, 0, 0, 0, 1);

// The defining relations.
/// 1959: 1 yd = 0.9144 m.
pub const YD: Mag = Mag::dec(9144, -4);
pub const FT: Mag = YD.over(3);
pub const IN: Mag = YD.over(36);
pub const MI: Mag = FT.times(5280);
pub const THOU: Mag = IN.mul(Mag::dec(1, -3));
/// 1959 (and 1893 before it): the US survey foot, 1200/3937 m.
pub const SFT: Mag = Mag::ratio(1200, 3937);
/// 1959: 1 lb = 0.45359237 kg.
pub const LB: Mag = Mag::dec(45359237, -8);
pub const GR: Mag = LB.over(7000);
pub const OZ: Mag = LB.over(16);
pub const OZ_T: Mag = GR.times(480);
/// 3rd CGPM 1901: g_n = 980.665 cm/s².
pub const GN: Mag = Mag::dec(980665, -5);
pub const LBF: Mag = LB.mul(GN);
/// lb · ft/s²
pub const PDL: Mag = LB.mul(FT);
pub const IN2: Mag = IN.powi(2);
pub const FT2: Mag = FT.powi(2);
pub const YD2: Mag = YD.powi(2);
pub const IN3: Mag = IN.powi(3);
pub const FT3: Mag = FT.powi(3);
pub const SFT2: Mag = SFT.powi(2);
/// HB 44: 1 gal = 231 in³.
pub const GAL_US: Mag = IN3.times(231);
pub const PT_US: Mag = GAL_US.over(8);
pub const FLOZ_US: Mag = PT_US.over(16);
/// HB 44: 1 bu = 2150.42 in³.
pub const BU_US: Mag = IN3.mul(Mag::dec(215042, -2));
/// WMA 1985: 1 gallon = 4.54609 dm³.
pub const GAL_IMP: Mag = Mag::dec(454609, -8);
pub const PT_IMP: Mag = GAL_IMP.over(8);
pub const FLOZ_IMP: Mag = PT_IMP.over(20);
/// 1 mL
pub const ML: Mag = Mag::dec(1, -6);
/// 5th International Conference on the Properties of Steam, 1956: cal_IT = 4.1868 J.
pub const CAL_IT: Mag = Mag::dec(41868, -4);
pub const CAL_TH: Mag = Mag::dec(4184, -3);
/// 1 °F (interval) = 5/9 K.
pub const DEG_F: Mag = Mag::ratio(5, 9);
/// cal/(g·°C) · lb · °F: the heat for one pound of water by one degree Fahrenheit.
pub const BTU_IT: Mag = CAL_IT.times(1000).mul(LB).mul(DEG_F);
pub const BTU_TH: Mag = CAL_TH.times(1000).mul(LB).mul(DEG_F);
/// Conventional mercury density, 13 595.1 kg/m³ (ISO 80000-4, not fetched).
pub const HG: Mag = Mag::dec(135951, -1);
/// Conventional water density, 1000 kg/m³ (SP 811 B.8 prints mmH2O conventional = 9.80665 Pa, bold).
pub const WATER: Mag = Mag::int(1000);
pub const HOUR: Mag = Mag::int(3600);
pub const HP_MECH: Mag = FT.mul(LBF).times(550);
pub const DEG: Mag = Mag::PI.over(180);
pub const TURN: Mag = Mag::PI.times(2);

const C_1959_YD: Cite = Cite(Yd1959, "1 yd = 0.9144 m exactly");
const C_1959_LB: Cite = Cite(Yd1959, "1 lb = 0.453 592 37 kg exactly");
const C_1959_SFT: Cite = Cite(
    Yd1959,
    "US survey foot = 1200/3937 m, kept for geodetic surveys",
);
const C_FR2020: Cite = Cite(
    Fr2020,
    "survey foot deprecated after 31 Dec 2022, units on it now on the international foot",
);
const C_HB_LEN: Cite = Cite(
    Hb44,
    "§2 Units of length, Gunter's or surveyors chain units",
);
const C_HB_LEN5: Cite = Cite(Hb44, "§5 Tables of equivalents, Units of length");
const C_HB_AREA: Cite = Cite(Hb44, "§2 Units of area");
const C_HB_LIQ: Cite = Cite(
    Hb44,
    "§2 Units of liquid volume, Apothecaries units of liquid volume",
);
const C_HB_DRY: Cite = Cite(Hb44, "§2 Units of dry volume");
const C_HB_VOL5: Cite = Cite(
    Hb44,
    "§5 Tables of equivalents, Units of capacity or volume",
);
const C_HB_BBL: Cite = Cite(
    Hb44,
    "§5 Units of capacity or volume, footnote 22 (barrels)",
);
const C_HB_AVDP: Cite = Cite(Hb44, "§2 Avoirdupois units of mass");
const C_HB_TROY: Cite = Cite(Hb44, "§2 Troy units of mass");
const C_HB_AP: Cite = Cite(Hb44, "§2 Apothecaries units of mass");
const C_HB_UK: Cite = Cite(Hb44, "§3 Notes on British units of measurement");
const C_SP_B6: Cite = Cite(Sp811, "B.6 US survey foot and units based on it");
const C_SP_B8: Cite = Cite(Sp811, "B.8 row of this unit");
const C_SP_FN12: Cite = Cite(
    Sp811,
    "B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000",
);
const C_WMA4: Cite = Cite(Wma1985, "Sch. 1 Pt IV (imperial units of volume)");
const C_WMA5: Cite = Cite(Wma1985, "Sch. 1 Pt V (imperial units of mass)");
const C_WMA6: Cite = Cite(Wma1985, "Sch. 1 Pt VI (supplementary and other units)");
const C_CGPM_GN: Cite = Cite(Cgpm1901, "g_n = 980.665 cm/s²");
const C_SI8_T6: Cite = Cite(
    SiBrochure8,
    "Table 6: 1° = (π/180) rad, 1′ = (1/60)°, 1″ = (1/60)′",
);
const C_FDA: Cite = Cite(
    Cfr21_101_9,
    "(b)(5)(viii): for nutrition labeling, tsp 5 mL, tbsp 15 mL, cup 240 mL, fl oz 30 mL, oz 28 g",
);
const C_SP_FN9: Cite = Cite(
    Sp811,
    "B.8 footnote 9: cal_IT = 4.1868 J (5th Int. Conf. Properties of Steam 1956), Btu_IT = 1.055 055 852 62 kJ exactly; cal_th = 4.184 J, Btu_th based on it",
);

// one row per unit, read as a table
#[rustfmt::skip]
pub static UNITS: &[Unit] = &[
    // ---- length, international (1959) ----
    Unit::new("inch", "inch", "in", LEN, IN, "1/36 yd", &[C_1959_YD, C_WMA6, C_SP_B8]).bold("2.54", -2),
    Unit::new("foot_international", "foot (international)", "ft", LEN, FT, "1/3 yd = 0.3048 m", &[C_1959_YD, C_FR2020, C_WMA6, C_SP_B8]).bold("3.048", -1),
    Unit::new("yard", "yard", "yd", LEN, YD, "0.9144 m", &[C_1959_YD, C_WMA6, C_SP_B8]).bold("9.144", -1),
    Unit::new("mile_international", "mile (international, statute)", "mi", LEN, MI, "5280 ft = 1760 yd", &[C_HB_LEN, C_FR2020, C_WMA6, C_SP_B8]).bold("1.609344", 3),
    Unit::new("thou", "thou (mil, 0.001 in)", "thou", LEN, THOU, "0.001 in", &[Cite(Hb44, "§4 Units of length, thickness measurement: 1 mil = 0.001 in"), C_SP_B8]).bold("2.54", -5),
    Unit::new("microinch", "microinch", "µin", LEN, IN.mul(Mag::dec(1, -6)), "10⁻⁶ in", &[C_SP_B8]).bold("2.54", -8),
    Unit::new("hand", "hand", "hh", LEN, IN.times(4), "4 in (custom, bracketed in HB 44)", &[C_HB_LEN5]),
    Unit::new("link_international", "link (Gunter's, international foot)", "li", LEN, FT.mul(Mag::dec(66, -2)), "0.66 ft", &[C_HB_LEN, C_FR2020]),
    Unit::new("rod_international", "rod, pole or perch (international foot)", "rd", LEN, FT.mul(Mag::dec(165, -1)), "16.5 ft", &[C_HB_LEN, C_FR2020]),
    Unit::new("chain_international", "chain (Gunter's, international foot)", "ch", LEN, FT.times(66), "66 ft = 22 yd", &[C_HB_LEN, C_FR2020, C_WMA6]),
    Unit::new("furlong_international", "furlong (international foot)", "fur", LEN, FT.times(660), "660 ft = 220 yd", &[C_HB_LEN, C_FR2020, C_WMA6]),
    Unit::new("fathom_international", "fathom (international foot)", "fath", LEN, FT.times(6), "6 ft", &[C_HB_LEN, C_FR2020]),
    Unit::new("cable_us", "cable's length (US, 120 fathoms)", "cable (US)", LEN, FT.times(720), "720 ft = 120 fathoms", &[C_HB_LEN, C_HB_LEN5]),
    Unit::new("league_international", "league (land, international foot)", "lea", LEN, MI.times(3), "3 mi = 15 840 ft", &[C_HB_LEN, C_HB_LEN5]),
    // ---- length, US survey foot (legacy) ----
    Unit::new("foot_us_survey", "foot (US survey)", "ft (US survey)", LEN, SFT, "1200/3937 m", &[C_1959_SFT, C_SP_B6, C_FR2020, C_SP_B8]).sp("3.048006", -1).legacy("survey foot deprecated after 31 Dec 2022"),
    Unit::new("mile_us_survey", "mile (US survey)", "mi (US survey)", LEN, SFT.times(5280), "5280 survey ft", &[C_SP_B6, C_FR2020, C_SP_B8]).sp("1.609347", 3).legacy("survey foot deprecated after 31 Dec 2022"),
    Unit::new("link_us_survey", "link (US survey)", "li (US survey)", LEN, SFT.mul(Mag::dec(66, -2)), "0.66 survey ft", &[C_SP_B6, C_FR2020]).legacy("survey foot deprecated after 31 Dec 2022"),
    Unit::new("rod_us_survey", "rod (US survey)", "rd (US survey)", LEN, SFT.mul(Mag::dec(165, -1)), "16.5 survey ft", &[C_SP_B6, C_FR2020, C_SP_B8]).sp("5.029210", 0).legacy("survey foot deprecated after 31 Dec 2022"),
    Unit::new("chain_us_survey", "chain (US survey)", "ch (US survey)", LEN, SFT.times(66), "66 survey ft", &[C_SP_B6, C_FR2020, C_SP_B8]).sp("2.011684", 1).legacy("survey foot deprecated after 31 Dec 2022"),
    Unit::new("furlong_us_survey", "furlong (US survey)", "fur (US survey)", LEN, SFT.times(660), "660 survey ft", &[C_SP_B6, C_FR2020]).legacy("survey foot deprecated after 31 Dec 2022"),
    Unit::new("fathom_us_survey", "fathom (US survey)", "fath (US survey)", LEN, SFT.times(6), "6 survey ft", &[C_SP_B6, C_FR2020, C_SP_B8]).sp("1.828804", 0).legacy("survey foot deprecated after 31 Dec 2022"),
    Unit::new("league_us_survey", "league (US survey)", "lea (US survey)", LEN, SFT.times(15840), "3 survey mi", &[C_SP_B6, C_FR2020]).legacy("survey foot deprecated after 31 Dec 2022"),
    // ---- length, nautical ----
    Unit::new("nautical_mile_international", "nautical mile (international)", "nmi", LEN, Mag::int(1852), "1852 m (IHO, Monaco 1929)", &[Cite(SiBrochure8, "Table 8 note (d)"), Cite(Sp811, "B.8 footnote 20"), C_HB_LEN, C_SP_B8]).bold("1.852", 3),
    Unit::new("nautical_mile_us_pre1954", "nautical mile (US, before 1 Jul 1954)", "nmi (US 1954)", LEN, Mag::dec(1853248, -3), "1853.248 m (≈ 6080.20 ft)", &[Cite(Yd1959, "footnote 6: US nautical mile of 1853.248 m = 6080.20 ft before 1 Jul 1954"), Cite(Hb44, "§5 Units of length footnote 21: formerly 6080.20 ft")]).legacy("replaced by the international nautical mile on 1 Jul 1954"),
    Unit::new("nautical_mile_uk_admiralty", "nautical mile (UK Admiralty)", "nmi (Admiralty)", LEN, FT.times(6080), "6080 ft", &[Cite(Unsourced, "UK Admiralty mile: no primary source fetched")]).legacy("replaced by the international nautical mile in the UK in 1970"),
    // ---- length, typographic ----
    Unit::new("point_dtp", "point (computer, DTP, 1/72 in)", "pt (DTP)", LEN, IN.over(72), "1/72 in", &[Cite(Sp811, "B.8 row label: point (computer) (1/72 in)")]).sp("3.527778", -4),
    Unit::new("pica_dtp", "pica (computer, DTP, 1/6 in)", "pc (DTP)", LEN, IN.over(6), "1/6 in", &[Cite(Sp811, "B.8 row label: pica (computer) (1/6 in)")]).sp("4.233333", -3),
    Unit::new("point_printers", "point (printer's)", "pt (printer's)", LEN, IN.mul(Mag::dec(13837, -6)), "0.013 837 in", &[Cite(Hb44, "§5 Units of length: 1 point = 0.013 837 inch (exactly)"), Cite(Yd1959, "Units of length: point (printers)"), C_SP_B8]).sp("3.514598", -4),
    Unit::new("pica_printers", "pica (printer's)", "pc (printer's)", LEN, IN.mul(Mag::dec(13837, -6)).times(12), "12 printer's points", &[Cite(Unsourced, "pica = 12 points not stated in the fetched texts; SP 811 B.8 prints the matching value"), C_SP_B8]).sp("4.217518", -3),
    // ---- area ----
    Unit::new("square_inch", "square inch", "in²", AREA, IN2, "in²", &[C_WMA6, C_SP_B8]).bold("6.4516", -4),
    Unit::new("square_foot", "square foot", "ft²", AREA, FT2, "144 in²", &[C_HB_AREA, C_SP_B8]).bold("9.290304", -2),
    Unit::new("square_yard", "square yard", "yd²", AREA, YD2, "9 ft²", &[C_HB_AREA, C_SP_B8]).sp("8.361274", -1),
    Unit::new("square_mile_international", "square mile (international)", "mi²", AREA, MI.powi(2), "640 acres", &[C_HB_AREA, C_WMA6, C_SP_B8]).sp("2.589988", 6),
    Unit::new("square_mile_us_survey", "square mile (US survey)", "mi² (US survey)", AREA, SFT.times(5280).powi(2), "640 survey acres", &[C_SP_B6, C_SP_B8]).sp("2.589998", 6).legacy("survey foot deprecated after 31 Dec 2022"),
    Unit::new("acre_international", "acre (international foot)", "ac", AREA, FT2.times(43560), "43 560 ft² = 4840 yd²", &[C_HB_AREA, C_FR2020, C_WMA6]),
    Unit::new("acre_us_survey", "acre (US survey)", "ac (US survey)", AREA, SFT2.times(43560), "43 560 survey ft²", &[C_SP_B6, C_SP_B8]).sp("4.046873", 3).legacy("survey foot deprecated after 31 Dec 2022"),
    Unit::new("rood", "rood", "ro", AREA, YD2.times(1210), "1210 yd²", &[C_WMA6]),
    Unit::new("circular_mil", "circular mil", "cmil", AREA, THOU.powi(2).mul(Mag::PI).over(4), "area of a circle 1 mil across, π/4 mil²", &[Cite(Unsourced, "π/4 mil² not stated in the fetched texts; SP 811 B.8 prints the matching value"), C_SP_B8]).sp("5.067075", -10),
    // ---- volume: cubic ----
    Unit::new("cubic_inch", "cubic inch", "in³", VOL, IN3, "in³", &[C_WMA6, Cite(Sp811, "B.8 footnote 13: 1.638 706 4 E-05 exactly"), C_SP_B8]).sp("1.638706", -5),
    Unit::new("cubic_foot", "cubic foot", "ft³", VOL, FT3, "1728 in³", &[Cite(Hb44, "§2 Units of volume"), C_SP_B8]).sp("2.831685", -2),
    Unit::new("cubic_yard", "cubic yard", "yd³", VOL, YD.powi(3), "27 ft³", &[Cite(Hb44, "§2 Units of volume"), C_SP_B8]).sp("7.645549", -1),
    Unit::new("acre_foot_international", "acre-foot (international foot)", "ac·ft", VOL, FT3.times(43560), "43 560 ft³", &[Cite(Hb44, "§2 footnote 4: acre-foot now also on the international foot"), C_FR2020]),
    Unit::new("acre_foot_us_survey", "acre-foot (US survey)", "ac·ft (US survey)", VOL, SFT.powi(3).times(43560), "43 560 survey ft³", &[C_SP_B6, C_SP_B8]).sp("1.233489", 3).legacy("survey foot deprecated after 31 Dec 2022"),
    Unit::new("cord", "cord (firewood)", "cd", VOL, FT3.times(128), "128 ft³", &[C_HB_VOL5, Cite(Yd1959, "Units of volume: cord"), C_SP_B8]).sp("3.624556", 0),
    Unit::new("ton_register", "register ton", "RT", VOL, FT3.times(100), "100 ft³", &[Cite(Unsourced, "100 ft³ (Moorsom) not stated in the fetched texts; SP 811 B.8 prints the matching value"), C_SP_B8]).sp("2.831685", 0),
    // ---- volume: US liquid ----
    Unit::new("gallon_us", "gallon (US liquid)", "gal (US)", VOL, GAL_US, "231 in³", &[C_HB_LIQ, C_SP_B8]).sp("3.785412", -3),
    Unit::new("quart_us_liquid", "quart (US liquid)", "liq qt (US)", VOL, GAL_US.over(4), "57.75 in³ = 1/4 gal", &[C_HB_LIQ, C_SP_B8]).sp("9.463529", -4),
    Unit::new("pint_us_liquid", "pint (US liquid)", "liq pt (US)", VOL, PT_US, "28.875 in³ = 1/8 gal", &[C_HB_LIQ, C_SP_B8]).sp("4.731765", -4),
    Unit::new("cup_us", "cup (US measuring)", "cup (US)", VOL, FLOZ_US.times(8), "8 fl oz", &[Cite(Hb44, "§5 Units of capacity or volume: 1 cup, measuring = 8 fl oz (exactly)"), C_SP_B8]).sp("2.365882", -4),
    Unit::new("gill_us", "gill (US)", "gi (US)", VOL, PT_US.over(4), "1/4 pt", &[C_HB_LIQ, C_SP_B8]).sp("1.182941", -4),
    Unit::new("fluid_ounce_us", "fluid ounce (US)", "fl oz (US)", VOL, FLOZ_US, "1/16 pt = 1/128 gal", &[C_HB_LIQ, C_SP_B8]).sp("2.957353", -5),
    Unit::new("tablespoon_us", "tablespoon (US measuring)", "tbsp (US)", VOL, FLOZ_US.over(2), "1/2 fl oz", &[Cite(Hb44, "§5 Units of capacity or volume: 1 tablespoon, measuring = 1/2 fl oz (exactly)"), C_SP_B8]).sp("1.478676", -5),
    Unit::new("teaspoon_us", "teaspoon (US measuring)", "tsp (US)", VOL, FLOZ_US.over(6), "1/3 tbsp", &[Cite(Hb44, "§5 Units of capacity or volume: 1 teaspoon, measuring = 1/3 tablespoon (exactly)"), C_SP_B8]).sp("4.928922", -6),
    Unit::new("fluid_dram_us", "fluid dram (US)", "fl dr (US)", VOL, FLOZ_US.over(8), "1/8 fl oz", &[C_HB_LIQ]),
    Unit::new("minim_us", "minim (US)", "min (US)", VOL, FLOZ_US.over(480), "1/60 fl dr", &[C_HB_LIQ]),
    // ---- volume: US dry ----
    Unit::new("bushel_us", "bushel (US, struck)", "bu (US)", VOL, BU_US, "2150.42 in³", &[C_HB_DRY, C_HB_VOL5, C_SP_B8]).sp("3.523907", -2),
    Unit::new("peck_us", "peck (US)", "pk (US)", VOL, BU_US.over(4), "1/4 bu", &[C_HB_DRY, C_SP_B8]).sp("8.809768", -3),
    Unit::new("quart_us_dry", "quart (US dry)", "dry qt (US)", VOL, BU_US.over(32), "1/32 bu", &[C_HB_DRY, C_SP_B8]).sp("1.101221", -3),
    Unit::new("pint_us_dry", "pint (US dry)", "dry pt (US)", VOL, BU_US.over(64), "1/64 bu", &[C_HB_DRY, C_SP_B8]).sp("5.506105", -4),
    // ---- volume: barrels (US) ----
    Unit::new("barrel_petroleum", "barrel (petroleum, 42 US gal)", "bbl (oil)", VOL, GAL_US.times(42), "42 gal", &[C_HB_BBL, C_SP_B8]).sp("1.589873", -1),
    Unit::new("barrel_fermented_liquor", "barrel (US federal fermented liquor tax, 31 gal)", "bbl (beer)", VOL, GAL_US.times(31), "31 gal", &[C_HB_BBL]),
    Unit::new("barrel_liquid_state", "barrel for liquids (US state laws, 31.5 gal)", "bbl (liquid)", VOL, GAL_US.mul(Mag::dec(315, -1)), "31.5 gal", &[C_HB_BBL]),
    Unit::new("barrel_cistern", "barrel (cistern, 36 gal)", "bbl (cistern)", VOL, GAL_US.times(36), "36 gal", &[C_HB_BBL]),
    Unit::new("barrel_proof_spirits", "barrel (US federal proof spirits, 40 gal)", "bbl (spirits)", VOL, GAL_US.times(40), "40 gal", &[C_HB_BBL]),
    Unit::new("barrel_dry", "barrel (US standard, dry commodities)", "bbl (dry)", VOL, IN3.times(7056), "7056 in³", &[C_HB_VOL5]),
    Unit::new("barrel_cranberry", "barrel (US standard, cranberry)", "bbl (cranberry)", VOL, IN3.times(5826), "5826 in³", &[C_HB_VOL5]),
    // ---- volume: FDA nutrition labeling ----
    Unit::new("cup_fda", "cup (US nutrition labeling)", "cup (FDA)", VOL, ML.times(240), "240 mL", &[C_FDA]),
    Unit::new("fluid_ounce_fda", "fluid ounce (US nutrition labeling)", "fl oz (FDA)", VOL, ML.times(30), "30 mL", &[C_FDA]),
    Unit::new("tablespoon_fda", "tablespoon (US nutrition labeling)", "tbsp (FDA)", VOL, ML.times(15), "15 mL", &[C_FDA]),
    Unit::new("teaspoon_fda", "teaspoon (US nutrition labeling)", "tsp (FDA)", VOL, ML.times(5), "5 mL", &[C_FDA]),
    // ---- volume: imperial ----
    Unit::new("gallon_imperial", "gallon (imperial)", "gal (imp)", VOL, GAL_IMP, "4.546 09 dm³", &[C_WMA6, C_HB_UK, C_SP_B8]).bold("4.54609", -3),
    Unit::new("quart_imperial", "quart (imperial)", "qt (imp)", VOL, GAL_IMP.over(4), "1/4 gal", &[C_WMA6, C_HB_UK]),
    Unit::new("pint_imperial", "pint (imperial)", "pt (imp)", VOL, PT_IMP, "0.568 261 25 dm³", &[C_WMA4, C_HB_UK]),
    Unit::new("gill_imperial", "gill (imperial)", "gi (imp)", VOL, PT_IMP.over(4), "1/4 pint", &[C_WMA6, C_HB_UK, C_SP_B8]).sp("1.420653", -4),
    Unit::new("fluid_ounce_imperial", "fluid ounce (imperial)", "fl oz (imp)", VOL, FLOZ_IMP, "1/20 pint", &[C_WMA6, C_HB_UK, C_SP_B8]).sp("2.841306", -5),
    Unit::new("fluid_drachm_imperial", "fluid drachm (imperial)", "fl dr (imp)", VOL, FLOZ_IMP.over(8), "1/8 fl oz", &[C_WMA6, C_HB_UK]),
    Unit::new("fluid_scruple_imperial", "fluid scruple (imperial)", "fl s (imp)", VOL, FLOZ_IMP.over(24), "1/3 fl drachm = 20 minims", &[C_HB_UK]),
    Unit::new("minim_imperial", "minim (imperial)", "min (imp)", VOL, FLOZ_IMP.over(480), "1/60 fl drachm", &[C_WMA6, C_HB_UK]),
    Unit::new("peck_imperial", "peck (imperial)", "pk (imp)", VOL, GAL_IMP.times(2), "2 gal", &[C_WMA6, C_HB_UK]),
    Unit::new("bushel_imperial", "bushel (imperial)", "bu (imp)", VOL, GAL_IMP.times(8), "8 gal", &[C_WMA6, C_HB_UK]),
    Unit::new("quarter_imperial_volume", "quarter (imperial, 8 bushels)", "qr (imp vol)", VOL, GAL_IMP.times(64), "8 bushels", &[C_HB_UK]),
    Unit::new("ton_water", "water ton (English, 224 imperial gal)", "ton (water)", VOL, GAL_IMP.times(224), "224 imperial gal", &[C_HB_VOL5]),
    // ---- mass: avoirdupois ----
    Unit::new("grain", "grain", "gr", MASS, GR, "1/7000 lb", &[C_HB_AVDP, C_WMA6, C_SP_B8]).bold("6.479891", -5),
    Unit::new("dram_avdp", "dram (avoirdupois)", "dr avdp", MASS, OZ.over(16), "1/16 oz = 27 11/32 gr", &[C_HB_AVDP, C_WMA6]),
    Unit::new("ounce_avdp", "ounce (avoirdupois)", "oz avdp", MASS, OZ, "1/16 lb = 437.5 gr", &[C_HB_AVDP, C_WMA6, C_SP_B8]).sp("2.834952", -2),
    Unit::new("pound_avdp", "pound (avoirdupois)", "lb avdp", MASS, LB, "0.453 592 37 kg", &[C_1959_LB, C_WMA6, Cite(Sp811, "B.8 footnote 22: 4.535 923 7 E-01 exactly"), C_SP_B8]).sp("4.535924", -1),
    Unit::new("stone", "stone", "st", MASS, LB.times(14), "14 lb", &[C_WMA6, C_HB_UK]),
    Unit::new("quarter_mass", "quarter (British, 28 lb)", "qr (mass)", MASS, LB.times(28), "28 lb", &[C_WMA6, C_HB_UK]),
    Unit::new("cental", "cental", "ctl", MASS, LB.times(100), "100 lb", &[C_WMA6]),
    Unit::new("hundredweight_short", "hundredweight (short, net)", "cwt (short)", MASS, LB.times(100), "100 lb", &[C_HB_AVDP, C_SP_B8]).sp("4.535924", 1),
    Unit::new("hundredweight_long", "hundredweight (long, gross)", "cwt (long)", MASS, LB.times(112), "112 lb", &[C_HB_AVDP, C_WMA6, C_SP_B8]).sp("5.080235", 1),
    Unit::new("ton_short", "ton (short, net)", "tn", MASS, LB.times(2000), "2000 lb", &[C_HB_AVDP, C_SP_B8]).sp("9.071847", 2),
    Unit::new("ton_long", "ton (long, gross)", "ton (long)", MASS, LB.times(2240), "2240 lb", &[C_HB_AVDP, C_WMA6, C_SP_B8]).sp("1.016047", 3),
    Unit::new("tonne", "tonne (metric ton)", "t", MASS, Mag::int(1000), "1000 kg", &[C_WMA6, C_SP_B8]).bold("1.0", 3),
    Unit::new("ounce_fda", "ounce (US nutrition labeling, 28 g)", "oz (FDA)", MASS, Mag::dec(28, -3), "28 g", &[C_FDA]),
    // ---- mass: troy and apothecaries ----
    Unit::new("pennyweight", "pennyweight", "dwt", MASS, GR.times(24), "24 gr", &[C_HB_TROY, C_WMA6, C_SP_B8]).sp("1.555174", -3),
    Unit::new("ounce_troy", "ounce (troy)", "oz t", MASS, OZ_T, "480 gr = 0.031 103 476 8 kg", &[C_HB_TROY, C_WMA5, C_SP_B8]).sp("3.110348", -2),
    Unit::new("pound_troy", "pound (troy)", "lb t", MASS, OZ_T.times(12), "12 oz t = 5760 gr", &[C_HB_TROY, C_SP_B8]).sp("3.732417", -1),
    Unit::new("scruple_apothecaries", "scruple (apothecaries)", "s ap", MASS, GR.times(20), "20 gr", &[C_HB_AP, C_WMA6]),
    Unit::new("dram_apothecaries", "dram, drachm (apothecaries)", "dr ap", MASS, GR.times(60), "3 scruples = 60 gr", &[C_HB_AP, C_WMA6]),
    Unit::new("ounce_apothecaries", "ounce (apothecaries)", "oz ap", MASS, OZ_T, "480 gr", &[C_HB_AP, C_WMA6, C_SP_B8]).sp("3.110348", -2),
    Unit::new("pound_apothecaries", "pound (apothecaries)", "lb ap", MASS, OZ_T.times(12), "12 oz ap = 5760 gr", &[C_HB_AP, C_SP_B8]).sp("3.732417", -1),
    // ---- mass: gravitational ----
    Unit::new("slug", "slug", "slug", MASS, LBF.div(FT), "lbf·s²/ft", &[Cite(Unsourced, "lbf·s²/ft not stated in the fetched texts; SP 811 B.8 prints the matching value"), C_SP_B8]).sp("1.459390", 1),
    // ---- force ----
    Unit::new("pound_force", "pound-force", "lbf", FORCE, LBF, "lb · g_n", &[C_1959_LB, C_CGPM_GN, Cite(Sp811, "B.8 footnote 23: 4.448 221 615 260 5 E+00 exactly with g_n"), C_SP_B8]).sp("4.448222", 0),
    Unit::new("ounce_force", "ounce-force (avoirdupois)", "ozf", FORCE, LBF.over(16), "oz · g_n", &[C_HB_AVDP, C_CGPM_GN, C_SP_B8]).sp("2.780139", -1),
    Unit::new("kip", "kip (1000 lbf)", "kip", FORCE, LBF.times(1000), "1000 lbf", &[Cite(Sp811, "B.8 row label: kip (1 kip = 1000 lbf)")]).sp("4.448222", 3),
    Unit::new("ton_force_short", "ton-force (short, 2000 lbf)", "tnf", FORCE, LBF.times(2000), "2000 lbf", &[Cite(Sp811, "B.8 row label: ton-force (2000 lbf)")]).sp("8.896443", 3),
    Unit::new("poundal", "poundal", "pdl", FORCE, PDL, "lb·ft/s²", &[Cite(Unsourced, "lb·ft/s² not stated in the fetched texts; SP 811 B.8 prints the matching value"), C_SP_B8]).sp("1.382550", -1),
    Unit::new("kilogram_force", "kilogram-force (kilopond)", "kgf", FORCE, GN, "kg · g_n", &[C_CGPM_GN, C_SP_B8]).bold("9.80665", 0),
    // ---- pressure ----
    Unit::new("psi", "pound-force per square inch", "psi", PRESSURE, LBF.div(IN2), "lbf/in²", &[C_SP_B8]).sp("6.894757", 3),
    Unit::new("ksi", "kip per square inch", "ksi", PRESSURE, LBF.div(IN2).times(1000), "kip/in²", &[C_SP_B8]).sp("6.894757", 6),
    Unit::new("psf", "pound-force per square foot", "psf", PRESSURE, LBF.div(FT2), "lbf/ft²", &[C_SP_B8]).sp("4.788026", 1),
    Unit::new("inch_of_mercury_conventional", "inch of mercury (conventional)", "inHg (conv)", PRESSURE, IN.mul(HG).mul(GN), "1 in · 13 595.1 kg/m³ · g_n", &[C_SP_FN12, Cite(Unsourced, "conventional mercury density 13 595.1 kg/m³ is ISO 80000-4, not fetched"), C_SP_B8]).sp("3.386389", 3),
    Unit::new("foot_of_mercury_conventional", "foot of mercury (conventional)", "ftHg (conv)", PRESSURE, FT.mul(HG).mul(GN), "1 ft · 13 595.1 kg/m³ · g_n", &[C_SP_FN12, Cite(Unsourced, "conventional mercury density 13 595.1 kg/m³ is ISO 80000-4, not fetched"), C_SP_B8]).sp("4.063666", 4),
    Unit::new("millimetre_of_mercury_conventional", "millimetre of mercury (conventional)", "mmHg (conv)", PRESSURE, Mag::dec(1, -3).mul(HG).mul(GN), "1 mm · 13 595.1 kg/m³ · g_n", &[C_SP_FN12, Cite(Unsourced, "conventional mercury density 13 595.1 kg/m³ is ISO 80000-4, not fetched"), C_SP_B8]).sp("1.333224", 2),
    Unit::new("millimetre_of_mercury_eu", "millimetre of mercury (EU legal, blood pressure)", "mmHg (EU)", PRESSURE, Mag::dec(133322, -3), "133.322 Pa", &[Cite(Eec80181, "Annex ch. II: 1 mm Hg = 133,322 Pa (blood pressure)"), Cite(SiBrochure8, "Table 8: 1 mmHg ≈ 133.322 Pa")]),
    Unit::new("torr", "torr", "Torr", PRESSURE, Mag::ratio(101325, 760), "1/760 atm", &[Cite(Unsourced, "1/760 atm not stated in the fetched texts; SP 811 B.8 prints the matching value"), C_SP_B8]).sp("1.333224", 2),
    Unit::new("inch_of_mercury_32f", "inch of mercury (32 °F)", "inHg (32 °F)", PRESSURE, Mag::dec(338638, -2), "measured: mercury density at 32 °F", &[C_SP_FN12, C_SP_B8]).sp("3.38638", 3).measured(),
    Unit::new("inch_of_mercury_60f", "inch of mercury (60 °F)", "inHg (60 °F)", PRESSURE, Mag::dec(337685, -2), "measured: mercury density at 60 °F", &[C_SP_FN12, C_SP_B8]).sp("3.37685", 3).measured(),
    Unit::new("inch_of_water_conventional", "inch of water (conventional)", "inH2O (conv)", PRESSURE, IN.mul(WATER).mul(GN), "1 in · 1000 kg/m³ · g_n", &[C_SP_FN12, C_SP_B8]).sp("2.490889", 2),
    Unit::new("foot_of_water_conventional", "foot of water (conventional)", "ftH2O (conv)", PRESSURE, FT.mul(WATER).mul(GN), "1 ft · 1000 kg/m³ · g_n", &[C_SP_FN12, C_SP_B8]).sp("2.989067", 3),
    Unit::new("millimetre_of_water_conventional", "millimetre of water (conventional)", "mmH2O (conv)", PRESSURE, Mag::dec(1, -3).mul(WATER).mul(GN), "1 mm · 1000 kg/m³ · g_n", &[C_SP_FN12, C_SP_B8]).bold("9.80665", 0),
    Unit::new("inch_of_water_39f", "inch of water (39.2 °F)", "inH2O (39.2 °F)", PRESSURE, Mag::dec(249082, -3), "measured: water density at 39.2 °F", &[C_SP_FN12, C_SP_B8]).sp("2.49082", 2).measured(),
    Unit::new("inch_of_water_60f", "inch of water (60 °F)", "inH2O (60 °F)", PRESSURE, Mag::dec(24884, -2), "measured: water density at 60 °F", &[C_SP_FN12, C_SP_B8]).sp("2.4884", 2).measured(),
    Unit::new("foot_of_water_39f", "foot of water (39.2 °F)", "ftH2O (39.2 °F)", PRESSURE, Mag::dec(298898, -2), "measured: water density at 39.2 °F", &[C_SP_FN12, C_SP_B8]).sp("2.98898", 3).measured(),
    Unit::new("atmosphere_standard", "atmosphere (standard)", "atm", PRESSURE, Mag::int(101325), "101 325 Pa", &[C_SP_B8]).bold("1.01325", 5),
    Unit::new("atmosphere_technical", "atmosphere (technical, kgf/cm²)", "at", PRESSURE, GN.mul(Mag::dec(1, 4)), "1 kgf/cm²", &[Cite(Sp811, "B.8 footnote 8: 1 at = 1 kgf/cm²"), C_SP_B8]).bold("9.80665", 4),
    // ---- energy ----
    Unit::new("foot_pound_force", "foot pound-force", "ft·lbf", ENERGY, FT.mul(LBF), "ft · lbf", &[C_SP_B8]).sp("1.355818", 0),
    Unit::new("foot_poundal", "foot poundal", "ft·pdl", ENERGY, FT.mul(PDL), "ft · pdl", &[Cite(Unsourced, "poundal = lb·ft/s² not stated in the fetched texts"), C_SP_B8]).sp("4.214011", -2),
    Unit::new("btu_it", "British thermal unit (International Table)", "Btu_IT", ENERGY, BTU_IT, "1.055 055 852 62 kJ; cal_IT/(g·°C) · lb · °F", &[C_SP_FN9, C_SP_B8]).sp("1.055056", 3),
    Unit::new("btu_th", "British thermal unit (thermochemical)", "Btu_th", ENERGY, BTU_TH, "cal_th/(g·°C) · lb · °F", &[C_SP_FN9, C_SP_B8]).sp("1.054350", 3),
    Unit::new("btu_mean", "British thermal unit (mean)", "Btu_mean", ENERGY, Mag::dec(105587, -2), "measured", &[C_SP_B8]).sp("1.05587", 3).measured(),
    Unit::new("btu_39f", "British thermal unit (39 °F)", "Btu_39", ENERGY, Mag::dec(105967, -2), "measured: water at 39 °F", &[C_SP_B8]).sp("1.05967", 3).measured(),
    Unit::new("btu_59f", "British thermal unit (59 °F)", "Btu_59", ENERGY, Mag::dec(105480, -2), "measured: water at 59 °F", &[C_SP_B8]).sp("1.05480", 3).measured(),
    Unit::new("btu_60f", "British thermal unit (60 °F)", "Btu_60", ENERGY, Mag::dec(105468, -2), "measured: water at 60 °F", &[C_SP_B8]).sp("1.05468", 3).measured(),
    Unit::new("calorie_it", "calorie (International Table)", "cal_IT", ENERGY, CAL_IT, "4.1868 J", &[C_SP_FN9, C_SP_B8]).bold("4.1868", 0),
    Unit::new("calorie_th", "calorie (thermochemical)", "cal_th", ENERGY, CAL_TH, "4.184 J", &[C_SP_FN9, C_SP_B8]).bold("4.184", 0),
    Unit::new("calorie_mean", "calorie (mean)", "cal_mean", ENERGY, Mag::dec(419002, -5), "measured", &[C_SP_B8]).sp("4.19002", 0).measured(),
    Unit::new("calorie_15c", "calorie (15 °C)", "cal_15", ENERGY, Mag::dec(418580, -5), "measured: water at 15 °C", &[C_SP_B8]).sp("4.18580", 0).measured(),
    Unit::new("calorie_20c", "calorie (20 °C)", "cal_20", ENERGY, Mag::dec(418190, -5), "measured: water at 20 °C", &[C_SP_B8]).sp("4.18190", 0).measured(),
    Unit::new("kilocalorie_it", "kilocalorie (International Table)", "kcal_IT", ENERGY, CAL_IT.times(1000), "1000 cal_IT", &[C_SP_FN9, C_SP_B8]).bold("4.1868", 3),
    Unit::new("kilocalorie_th", "kilocalorie (thermochemical)", "kcal_th", ENERGY, CAL_TH.times(1000), "1000 cal_th", &[C_SP_FN9, C_SP_B8]).bold("4.184", 3),
    Unit::new("calorie_food_it", "Calorie (food, kilogram calorie IT)", "Cal_IT", ENERGY, CAL_IT.times(1000), "1 kcal_IT", &[Cite(Sp811, "B.8 footnote 11 and row calorie_IT, kilogram (nutrition)")]).bold("4.1868", 3),
    Unit::new("calorie_food_th", "Calorie (food, kilogram calorie thermochemical)", "Cal_th", ENERGY, CAL_TH.times(1000), "1 kcal_th", &[Cite(Sp811, "B.8 footnote 11 and row calorie_th, kilogram (nutrition)")]).bold("4.184", 3),
    Unit::new("therm_ec", "therm (EC)", "thm (EC)", ENERGY, Mag::dec(105506, 3), "105.506 MJ", &[Cite(Sp811, "B.8 footnote 24"), Cite(Eec80181, "Annex ch. III: 1 therm = 105,506 × 10⁶ J"), C_SP_B8]).bold("1.05506", 8),
    Unit::new("therm_us", "therm (US)", "thm (US)", ENERGY, Mag::dec(1054804, 2), "105.4804 MJ (Federal Register 27 Jul 1968)", &[Cite(Sp811, "B.8 footnote 24"), C_SP_B8]).bold("1.054804", 8),
    Unit::new("quad", "quad (10¹⁵ Btu_IT)", "quad", ENERGY, BTU_IT.mul(Mag::dec(1, 15)), "10¹⁵ Btu_IT", &[Cite(Sp811, "B.8 row label: quad (10¹⁵ Btu_IT)")]).sp("1.055056", 18),
    Unit::new("ton_of_tnt", "ton of TNT (energy equivalent)", "tTNT", ENERGY, CAL_TH.mul(Mag::dec(1, 9)), "10⁹ cal_th = 4.184 GJ", &[Cite(Sp811, "B.8 footnote 25: defined, not measured"), C_SP_B8]).bold("4.184", 9),
    Unit::new("horsepower_hour", "horsepower hour (mechanical)", "hp·h", ENERGY, HP_MECH.mul(HOUR), "hp · 3600 s", &[Cite(Sp811, "B.8 row horsepower (550 ft·lbf/s)")]),
    // ---- power ----
    Unit::new("horsepower_mechanical", "horsepower (mechanical, 550 ft·lbf/s)", "hp (mech)", POWER, HP_MECH, "550 ft·lbf/s", &[Cite(Sp811, "B.8 row label: horsepower (550 ft·lbf/s)")]).sp("7.456999", 2),
    Unit::new("horsepower_metric", "horsepower (metric, PS)", "PS", POWER, GN.times(75), "75 kgf·m/s", &[Cite(Unsourced, "75 kgf·m/s is DIN 66036, not fetched; SP 811 B.8 prints the matching value"), C_SP_B8]).sp("7.354988", 2),
    Unit::new("horsepower_electric", "horsepower (electric)", "hp (elec)", POWER, Mag::int(746), "746 W", &[C_SP_B8]).bold("7.46", 2),
    Unit::new("horsepower_boiler", "horsepower (boiler)", "hp (boiler)", POWER, Mag::dec(980950, -2), "evaporation rate of water; no exact definition fits SP 811's value", &[C_SP_B8]).sp("9.80950", 3).measured(),
    Unit::new("horsepower_water", "horsepower (water)", "hp (water)", POWER, Mag::dec(746043, -3), "measured", &[C_SP_B8]).sp("7.46043", 2).measured(),
    Unit::new("btu_it_per_hour", "Btu_IT per hour", "Btu_IT/h", POWER, BTU_IT.div(HOUR), "Btu_IT/h", &[C_SP_B8]).sp("2.930711", -1),
    Unit::new("ton_of_refrigeration", "ton of refrigeration (12 000 Btu_IT/h)", "TR", POWER, BTU_IT.times(12000).div(HOUR), "12 000 Btu_IT/h", &[Cite(Sp811, "B.8 row label: ton of refrigeration (12 000 Btu_IT/h)")]).sp("3.516853", 3),
    // ---- temperature intervals (origins below) ----
    Unit::new("degree_fahrenheit", "degree Fahrenheit (interval)", "°F", TEMP, DEG_F, "1/1.8 K", &[Cite(Sp811, "B.8 rows degree Fahrenheit: T/K = (t/°F + 459.67)/1.8"), C_SP_B8]).sp("5.555556", -1),
    Unit::new("degree_rankine", "degree Rankine (interval)", "°R", TEMP, DEG_F, "1/1.8 K", &[Cite(Sp811, "B.8 rows degree Rankine: T/K = (T/°R)/1.8"), C_SP_B8]).sp("5.555556", -1),
    // ---- speed, acceleration ----
    Unit::new("mile_per_hour", "mile per hour", "mph", SPEED, MI.div(HOUR), "mi/h", &[C_SP_B8]).bold("4.4704", -1),
    Unit::new("knot", "knot", "kn", SPEED, Mag::ratio(1852, 3600), "1 nmi/h = 1852/3600 m/s", &[Cite(SiBrochure8, "Table 8: 1 kn = (1852/3600) m/s")]),
    Unit::new("foot_per_second", "foot per second", "ft/s", SPEED, FT, "ft/s", &[C_SP_B8]).bold("3.048", -1),
    Unit::new("foot_per_second_squared", "foot per second squared", "ft/s²", ACCEL, FT, "ft/s²", &[C_SP_B8]).bold("3.048", -1),
    // ---- angle ----
    Unit::new("degree_angle", "degree (angle)", "°", ANGLE, DEG, "π/180 rad", &[C_SI8_T6, C_SP_B8]).sp("1.745329", -2),
    Unit::new("arcminute", "minute of arc", "′", ANGLE, DEG.over(60), "1/60°", &[C_SI8_T6]),
    Unit::new("arcsecond", "second of arc", "″", ANGLE, DEG.over(3600), "1/60′", &[C_SI8_T6]),
    Unit::new("gon", "gon (grade)", "gon", ANGLE, Mag::PI.over(200), "0.9°", &[Cite(Sp811, "B.8 rows gon: 9.0 E-01 degree (bold)")]).sp("1.570796", -2),
    Unit::new("turn", "revolution (turn)", "r", ANGLE, TURN, "2π rad", &[C_SP_B8]).sp("6.283185", 0),
    Unit::new("mil_nato", "mil (angle, NATO, 1/6400 turn)", "mil (NATO)", ANGLE, TURN.over(6400), "0.056 25° = 1/6400 turn", &[Cite(Sp811, "B.8 rows mil (angle): 5.625 E-02 degree (bold)")]).sp("9.817477", -4),
    Unit::new("mil_warsaw_pact", "mil (angle, Warsaw Pact, 1/6000 turn)", "mil (WP)", ANGLE, TURN.over(6000), "1/6000 turn", &[Cite(Unsourced, "1/6000 turn: no primary source fetched")]),
    Unit::new("streck_swedish", "streck (angle, Swedish, 1/6300 turn)", "streck", ANGLE, TURN.over(6300), "1/6300 turn", &[Cite(Unsourced, "1/6300 turn: no primary source fetched")]),
    Unit::new("milliradian", "milliradian (the 6283-per-turn mil)", "mrad", ANGLE, Mag::dec(1, -3), "10⁻³ rad", &[Cite(SiBrochure8, "radian and the SI prefix milli")]),
];

/// Absolute temperature scales on the customary intervals.
pub static ORIGINS: &[Origin] = &[
    Origin {
        id: "fahrenheit",
        unit: "degree_fahrenheit",
        // (t/°F + 459.67)/1.8 at t = 0
        zero: Some(Mag::ratio(45967, 180)),
        sources: &[Cite(
            Sp811,
            "B.8 rows degree Fahrenheit: T/K = (t/°F + 459.67)/1.8, bold",
        )],
    },
    Origin {
        id: "rankine",
        unit: "degree_rankine",
        zero: None,
        sources: &[Cite(
            Sp811,
            "B.8 rows degree Rankine: T/K = (T/°R)/1.8, bold",
        )],
    },
];

/// Names (and symbols) that mean more than one unit: never an id or a symbol of the table.
pub static AMBIGUOUS: &[Ambiguous] = &[
    Ambiguous {
        bare: "gallon",
        symbols: &["gal"],
        members: &["gallon_us", "gallon_imperial"],
        note: "US 231 in³ vs imperial 4.546 09 L (20 % larger)",
    },
    Ambiguous {
        bare: "quart",
        symbols: &["qt"],
        members: &["quart_us_liquid", "quart_us_dry", "quart_imperial"],
        note: "HB 44 §2 note 8: say \"dry\" to tell the dry quart from the liquid one",
    },
    Ambiguous {
        bare: "pint",
        symbols: &["pt"],
        members: &["pint_us_liquid", "pint_us_dry", "pint_imperial"],
        note: "80/181/EEC also lists a rounded pint 0.5683 dm³ for UK/IE; the definition is WMA 1985's 0.568 261 25 dm³",
    },
    Ambiguous {
        bare: "gill",
        symbols: &["gi"],
        members: &["gill_us", "gill_imperial"],
        note: "US 1/4 US pint vs imperial 1/4 imperial pint",
    },
    Ambiguous {
        bare: "fluid ounce",
        symbols: &["fl oz", "fl. oz."],
        members: &["fluid_ounce_us", "fluid_ounce_imperial", "fluid_ounce_fda"],
        note: "US 1/128 gal, imperial 1/160 gal, US labeling 30 mL; 80/181/EEC's rounded 28.41 cm³ is not a definition",
    },
    Ambiguous {
        bare: "ounce",
        symbols: &["oz"],
        members: &[
            "ounce_avdp",
            "ounce_troy",
            "ounce_apothecaries",
            "ounce_fda",
            "ounce_force",
            "fluid_ounce_us",
            "fluid_ounce_imperial",
            "fluid_ounce_fda",
        ],
        note: "HB 44 §2 note 9 reads an unqualified ounce as avoirdupois; troy and apothecaries 480 gr, avdp 437.5 gr; also used for the fluid ounce",
    },
    Ambiguous {
        bare: "pound",
        symbols: &["lb", "#"],
        members: &[
            "pound_avdp",
            "pound_troy",
            "pound_apothecaries",
            "pound_force",
        ],
        note: "HB 44 §2 note 9 reads an unqualified pound as avoirdupois; lbm (mass) vs lbf (force) are different dimensions",
    },
    Ambiguous {
        bare: "dram",
        symbols: &["dr"],
        members: &["dram_avdp", "dram_apothecaries", "fluid_dram_us"],
        note: "avdp 1/16 oz (27.34 gr) vs apothecaries 60 gr vs the US fluid dram",
    },
    Ambiguous {
        bare: "drachm",
        symbols: &[],
        members: &["dram_apothecaries", "fluid_drachm_imperial"],
        note: "British spelling of the apothecaries dram; also the imperial fluid drachm",
    },
    Ambiguous {
        bare: "fluid dram",
        symbols: &["fl dr"],
        members: &["fluid_dram_us", "fluid_drachm_imperial"],
        note: "1/8 US fl oz vs 1/8 imperial fl oz",
    },
    Ambiguous {
        bare: "minim",
        symbols: &["min", "♏"],
        members: &["minim_us", "minim_imperial"],
        note: "1/480 US fl oz vs 1/480 imperial fl oz; \"min\" is also the minute",
    },
    Ambiguous {
        bare: "scruple",
        symbols: &[],
        members: &["scruple_apothecaries", "fluid_scruple_imperial"],
        note: "20 gr (mass) vs the British fluid scruple (volume)",
    },
    Ambiguous {
        bare: "bushel",
        symbols: &["bu"],
        members: &["bushel_us", "bushel_imperial"],
        note: "US 2150.42 in³ vs imperial 8 gal (3 % larger); heaped bushels are customs, not here",
    },
    Ambiguous {
        bare: "peck",
        symbols: &["pk"],
        members: &["peck_us", "peck_imperial"],
        note: "1/4 of the respective bushel",
    },
    Ambiguous {
        bare: "cup",
        symbols: &["c"],
        members: &["cup_us", "cup_fda"],
        note: "US measuring cup 8 fl oz (236.6 mL) vs US labeling 240 mL; metric (250 mL) and imperial cups are not defined by the fetched sources",
    },
    Ambiguous {
        bare: "tablespoon",
        symbols: &["tbsp", "T"],
        members: &["tablespoon_us", "tablespoon_fda"],
        note: "US measuring 1/2 fl oz vs US labeling 15 mL",
    },
    Ambiguous {
        bare: "teaspoon",
        symbols: &["tsp", "t."],
        members: &["teaspoon_us", "teaspoon_fda"],
        note: "US measuring 1/6 fl oz vs US labeling 5 mL",
    },
    Ambiguous {
        bare: "barrel",
        symbols: &["bbl"],
        members: &[
            "barrel_petroleum",
            "barrel_fermented_liquor",
            "barrel_liquid_state",
            "barrel_cistern",
            "barrel_proof_spirits",
            "barrel_dry",
            "barrel_cranberry",
        ],
        note: "HB 44 footnote 22: 31 to 42 gal by law or industry; consult the application's regulation",
    },
    Ambiguous {
        bare: "ton",
        symbols: &["ton", "T"],
        members: &[
            "ton_short",
            "ton_long",
            "tonne",
            "ton_force_short",
            "ton_of_refrigeration",
            "ton_of_tnt",
            "ton_water",
            "ton_register",
        ],
        note: "HB 44 §2 note 11: \"tn\" is the required abbreviation of the short ton; note 10: unmodified \"ton\" is commonly the 2000 lb short ton in the US, 2240 lb in WMA 1985; mass, force, power, energy and volume all use the name",
    },
    Ambiguous {
        bare: "hundredweight",
        symbols: &["cwt"],
        members: &["hundredweight_short", "hundredweight_long"],
        note: "US 100 lb vs British 112 lb (HB 44 §2 note 10, §3)",
    },
    Ambiguous {
        bare: "quarter",
        symbols: &["qr"],
        members: &["quarter_mass", "quarter_imperial_volume"],
        note: "British 28 lb vs the British dry quarter of 8 bushels",
    },
    Ambiguous {
        bare: "foot",
        symbols: &[],
        members: &["foot_international", "foot_us_survey"],
        note: "1 ft = 0.999 998 survey ft; since 1 Jan 2023 \"foot\" means the international foot (85 FR 62698), so ft is its symbol and the survey foot is qualified",
    },
    Ambiguous {
        bare: "mile",
        symbols: &[],
        members: &[
            "mile_international",
            "mile_us_survey",
            "nautical_mile_international",
            "nautical_mile_us_pre1954",
            "nautical_mile_uk_admiralty",
        ],
        note: "statute (international) vs survey vs nautical; since 1 Jan 2023 \"mile\" is the international mile, mi its symbol",
    },
    Ambiguous {
        bare: "nautical mile",
        symbols: &["NM", "M", "Nm"],
        members: &[
            "nautical_mile_international",
            "nautical_mile_us_pre1954",
            "nautical_mile_uk_admiralty",
        ],
        note: "1852 m since 1929 (IHO), the US 1853.248 m until 1954, the UK Admiralty 6080 ft until 1970",
    },
    Ambiguous {
        bare: "square mile",
        symbols: &[],
        members: &["square_mile_international", "square_mile_us_survey"],
        note: "international vs survey foot",
    },
    Ambiguous {
        bare: "acre",
        symbols: &[],
        members: &["acre_international", "acre_us_survey"],
        note: "international vs survey foot (4 ppm)",
    },
    Ambiguous {
        bare: "acre foot",
        symbols: &["acre-ft"],
        members: &["acre_foot_international", "acre_foot_us_survey"],
        note: "international vs survey foot",
    },
    Ambiguous {
        bare: "link",
        symbols: &[],
        members: &["link_international", "link_us_survey"],
        note: "international vs survey foot",
    },
    Ambiguous {
        bare: "rod",
        symbols: &[],
        members: &["rod_international", "rod_us_survey"],
        note: "international vs survey foot; also pole, perch",
    },
    Ambiguous {
        bare: "chain",
        symbols: &[],
        members: &["chain_international", "chain_us_survey"],
        note: "international vs survey foot",
    },
    Ambiguous {
        bare: "furlong",
        symbols: &[],
        members: &["furlong_international", "furlong_us_survey"],
        note: "international vs survey foot",
    },
    Ambiguous {
        bare: "fathom",
        symbols: &[],
        members: &["fathom_international", "fathom_us_survey"],
        note: "international vs survey foot",
    },
    Ambiguous {
        bare: "league",
        symbols: &[],
        members: &["league_international", "league_us_survey"],
        note: "international vs survey foot; the nautical league (3 nmi) is not defined by the fetched sources",
    },
    Ambiguous {
        bare: "cable",
        symbols: &[],
        members: &["cable_us"],
        note: "US 720 ft (120 fathoms); the British cable (1/10 nmi) and others are not defined by the fetched sources",
    },
    Ambiguous {
        bare: "mil",
        symbols: &["mil"],
        members: &[
            "thou",
            "mil_nato",
            "mil_warsaw_pact",
            "streck_swedish",
            "milliradian",
            "circular_mil",
        ],
        note: "thickness 0.001 in vs angular mils: NATO 6400, Warsaw Pact 6000, Swedish 6300 per turn, the milliradian (~6283 per turn)",
    },
    Ambiguous {
        bare: "point",
        symbols: &["pt"],
        members: &["point_dtp", "point_printers"],
        note: "1/72 in vs 0.013 837 in (HB 44 §5: \"1/72 inch (approximately)\"); Didot and other points are not defined by the fetched sources",
    },
    Ambiguous {
        bare: "pica",
        symbols: &["pc"],
        members: &["pica_dtp", "pica_printers"],
        note: "1/6 in vs 12 printer's points",
    },
    Ambiguous {
        bare: "btu",
        symbols: &["Btu", "BTU"],
        members: &[
            "btu_it", "btu_th", "btu_mean", "btu_39f", "btu_59f", "btu_60f",
        ],
        note: "IT and thermochemical are defined; mean, 39, 59 and 60 °F are measured values",
    },
    Ambiguous {
        bare: "calorie",
        symbols: &["cal", "Cal", "kcal"],
        members: &[
            "calorie_it",
            "calorie_th",
            "calorie_mean",
            "calorie_15c",
            "calorie_20c",
            "kilocalorie_it",
            "kilocalorie_th",
            "calorie_food_it",
            "calorie_food_th",
        ],
        note: "the food Calorie is a kilocalorie (SP 811 footnote 11); IT 4.1868 J vs thermochemical 4.184 J",
    },
    Ambiguous {
        bare: "therm",
        symbols: &["thm"],
        members: &["therm_ec", "therm_us"],
        note: "EC 105.506 MJ (based on Btu_IT) vs US 105.4804 MJ, the US gas industry's legal unit (SP 811 footnote 24)",
    },
    Ambiguous {
        bare: "horsepower",
        symbols: &["hp", "HP"],
        members: &[
            "horsepower_mechanical",
            "horsepower_metric",
            "horsepower_electric",
            "horsepower_boiler",
            "horsepower_water",
        ],
        note: "mechanical 550 ft·lbf/s (also SP 811's \"horsepower (U.K.)\", 745.70 W), metric 75 kgf·m/s, electric 746 W, boiler and water measured",
    },
    Ambiguous {
        bare: "inch of mercury",
        symbols: &["inHg", "in Hg", "\"Hg"],
        members: &[
            "inch_of_mercury_conventional",
            "inch_of_mercury_32f",
            "inch_of_mercury_60f",
        ],
        note: "conventional (ISO 80000) vs at a stated temperature (measured)",
    },
    Ambiguous {
        bare: "millimetre of mercury",
        symbols: &["mmHg", "mm Hg"],
        members: &[
            "millimetre_of_mercury_conventional",
            "millimetre_of_mercury_eu",
            "torr",
        ],
        note: "conventional 133.322 387 415 Pa vs EU legal 133.322 Pa vs torr 101 325/760 Pa (133.322 368 Pa)",
    },
    Ambiguous {
        bare: "inch of water",
        symbols: &["inH2O", "in H2O", "inWC", "\"WC"],
        members: &[
            "inch_of_water_conventional",
            "inch_of_water_39f",
            "inch_of_water_60f",
        ],
        note: "conventional (1000 kg/m³) vs at a stated temperature (measured)",
    },
    Ambiguous {
        bare: "foot of water",
        symbols: &["ftH2O"],
        members: &["foot_of_water_conventional", "foot_of_water_39f"],
        note: "conventional vs at 39.2 °F (measured)",
    },
    Ambiguous {
        bare: "atmosphere",
        symbols: &[],
        members: &["atmosphere_standard", "atmosphere_technical"],
        note: "101 325 Pa vs 1 kgf/cm² = 98 066.5 Pa",
    },
    Ambiguous {
        bare: "degree",
        symbols: &["deg"],
        members: &["degree_angle", "degree_fahrenheit", "degree_rankine"],
        note: "angle vs temperature intervals (and °C, outside this table); a temperature also needs its origin",
    },
];
