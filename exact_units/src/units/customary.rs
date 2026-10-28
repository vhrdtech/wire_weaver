//! Imperial, US customary and other units outside the SI (STD-72): every one the exact rational of SI its legal
//! definition gives, with the source of that definition in its docs (NIST SP 811 Appendix B, NIST Handbook 44
//! Appendix C, the 1959 international yard and pound, the UK Weights and Measures Act 1985, the 3rd CGPM for g_n).
//!
//! A name that means more than one unit is never a marker of its own: there is no `Gallon`, `Ounce`, `Ton`, `Btu` or
//! `Horsepower`, only [GallonUs] and [GallonImperial], [OunceAvdp] and [OunceTroy], [TonShort], [TonLong] and
//! [Tonne], [BtuIt] and [BtuTh], [HorsepowerMechanical] and [HorsepowerMetric]. A pound of mass ([PoundAvdp]) and a
//! pound of force ([PoundForce]) are different dimensions, and a [Slug] is a mass 32.174.. lb apart.
//!
//! A unit fixed by a physical property (mercury or water at a temperature, a calorie or Btu at a temperature, the
//! boiler horsepower) says so in its docs: its scale is the value SP 811 prints, not a definition.
//!
//! Temperatures in °F and °R are [Point](crate::Point)s against the origins [Fahrenheit](crate::origins::Fahrenheit)
//! and [Rankine](crate::origins::Rankine), in steps of `Rational<5, 9>` of a [Kelvin](super::Kelvin); angles in
//! degrees and turns are [Degree](super::Degree) and [Turn](super::Turn), a milliradian `Fixed<Radian, Milli, T>`.
//!
//! [CUSTOMARY] lists them all, [by_name](super::by_name) finds them by name like the SI markers.

#[cfg(all(test, feature = "std"))]
mod sourced;

crate::unit! {
    @table
    /// Every marker of this module, in the order of the docs.
    pub static CUSTOMARY;

    /// Inch: 1/36 yd.
    ///
    /// - 1959 yard and pound (24 FR 5348): 1 yd = 0.9144 m exactly
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct Inch = "in", [m 1], Exact(127 / 1, pow10 -4, pow2 1);

    /// Foot (international): 1/3 yd = 0.3048 m.
    ///
    /// - 1959 yard and pound (24 FR 5348): 1 yd = 0.9144 m exactly
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct FootInternational = "ft", [m 1], Exact(381 / 1, pow10 -4, pow2 3);

    /// Yard: 0.9144 m.
    ///
    /// - 1959 yard and pound (24 FR 5348): 1 yd = 0.9144 m exactly
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct Yard = "yd", [m 1], Exact(1143 / 1, pow10 -4, pow2 3);

    /// Mile (international, statute): 5280 ft = 1760 yd.
    ///
    /// - NIST HB 44 App. C: §2 Units of length, Gunter's or surveyors chain units
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct MileInternational = "mi", [m 1], Exact(12573 / 1, pow10 -3, pow2 7);

    /// Thou (mil, 0.001 in): 0.001 in.
    ///
    /// - NIST HB 44 App. C: §4 Units of length, thickness measurement: 1 mil = 0.001 in
    /// - NIST SP 811: B.8 row of this unit
    pub struct Thou = "thou", [m 1], Exact(127 / 1, pow10 -7, pow2 1);

    /// Microinch: 10⁻⁶ in.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct Microinch = "µin", [m 1], Exact(127 / 1, pow10 -10, pow2 1);

    /// Hand: 4 in (custom, bracketed in HB 44).
    ///
    /// - NIST HB 44 App. C: §5 Tables of equivalents, Units of length
    pub struct Hand = "hh", [m 1], Exact(127 / 1, pow10 -4, pow2 3);

    /// Link (Gunter's, international foot): 0.66 ft.
    ///
    /// - NIST HB 44 App. C: §2 Units of length, Gunter's or surveyors chain units
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    pub struct LinkInternational = "li", [m 1], Exact(12573 / 1, pow10 -6, pow2 4);

    /// Rod, pole or perch (international foot): 16.5 ft.
    ///
    /// - NIST HB 44 App. C: §2 Units of length, Gunter's or surveyors chain units
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    pub struct RodInternational = "rd", [m 1], Exact(12573 / 1, pow10 -4, pow2 2);

    /// Chain (Gunter's, international foot): 66 ft = 22 yd.
    ///
    /// - NIST HB 44 App. C: §2 Units of length, Gunter's or surveyors chain units
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    pub struct ChainInternational = "ch", [m 1], Exact(12573 / 1, pow10 -4, pow2 4);

    /// Furlong (international foot): 660 ft = 220 yd.
    ///
    /// - NIST HB 44 App. C: §2 Units of length, Gunter's or surveyors chain units
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    pub struct FurlongInternational = "fur", [m 1], Exact(12573 / 1, pow10 -3, pow2 4);

    /// Fathom (international foot): 6 ft.
    ///
    /// - NIST HB 44 App. C: §2 Units of length, Gunter's or surveyors chain units
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    pub struct FathomInternational = "fath", [m 1], Exact(1143 / 1, pow10 -4, pow2 4);

    /// Cable's length (US, 120 fathoms): 720 ft = 120 fathoms.
    ///
    /// - NIST HB 44 App. C: §2 Units of length, Gunter's or surveyors chain units
    /// - NIST HB 44 App. C: §5 Tables of equivalents, Units of length
    pub struct CableUs = "cable (US)", [m 1], Exact(3429 / 1, pow10 -3, pow2 6);

    /// League (land, international foot): 3 mi = 15 840 ft.
    ///
    /// - NIST HB 44 App. C: §2 Units of length, Gunter's or surveyors chain units
    /// - NIST HB 44 App. C: §5 Tables of equivalents, Units of length
    pub struct LeagueInternational = "lea", [m 1], Exact(37719 / 1, pow10 -3, pow2 7);

    /// Foot (US survey): 1200/3937 m. Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - 1959 yard and pound (24 FR 5348): US survey foot = 1200/3937 m, kept for geodetic surveys
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    /// - NIST SP 811: B.8 row of this unit
    pub struct FootUsSurvey = "ft (US survey)", [m 1], Exact(3 / 3937, pow10 2, pow2 2);

    /// Mile (US survey): 5280 survey ft. Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    /// - NIST SP 811: B.8 row of this unit
    pub struct MileUsSurvey = "mi (US survey)", [m 1], Exact(99 / 3937, pow10 3, pow2 6);

    /// Link (US survey): 0.66 survey ft. Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    pub struct LinkUsSurvey = "li (US survey)", [m 1], Exact(99 / 3937, pow2 3);

    /// Rod (US survey): 16.5 survey ft. Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    /// - NIST SP 811: B.8 row of this unit
    pub struct RodUsSurvey = "rd (US survey)", [m 1], Exact(99 / 3937, pow10 2, pow2 1);

    /// Chain (US survey): 66 survey ft. Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    /// - NIST SP 811: B.8 row of this unit
    pub struct ChainUsSurvey = "ch (US survey)", [m 1], Exact(99 / 3937, pow10 2, pow2 3);

    /// Furlong (US survey): 660 survey ft. Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    pub struct FurlongUsSurvey = "fur (US survey)", [m 1], Exact(99 / 3937, pow10 3, pow2 3);

    /// Fathom (US survey): 6 survey ft. Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    /// - NIST SP 811: B.8 row of this unit
    pub struct FathomUsSurvey = "fath (US survey)", [m 1], Exact(9 / 3937, pow10 2, pow2 3);

    /// League (US survey): 3 survey mi. Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    pub struct LeagueUsSurvey = "lea (US survey)", [m 1], Exact(297 / 3937, pow10 3, pow2 6);

    /// Nautical mile (international): 1852 m (IHO, Monaco 1929).
    ///
    /// - SI Brochure 8: Table 8 note (d)
    /// - NIST SP 811: B.8 footnote 20
    /// - NIST HB 44 App. C: §2 Units of length, Gunter's or surveyors chain units
    /// - NIST SP 811: B.8 row of this unit
    pub struct NauticalMileInternational = "nmi", [m 1], Exact(463 / 1, pow2 2);

    /// Nautical mile (US, before 1 Jul 1954): 1853.248 m (≈ 6080.20 ft). Legacy: replaced by the international nautical mile on 1 Jul 1954.
    ///
    /// - 1959 yard and pound (24 FR 5348): footnote 6: US nautical mile of 1853.248 m = 6080.20 ft before 1 Jul 1954
    /// - NIST HB 44 App. C: §5 Units of length footnote 21: formerly 6080.20 ft
    pub struct NauticalMileUsPre1954 = "nmi (US 1954)", [m 1], Exact(28957 / 1, pow10 -3, pow2 6);

    /// Nautical mile (UK Admiralty): 6080 ft. Legacy: replaced by the international nautical mile in the UK in 1970.
    ///
    /// - Unsourced: UK Admiralty mile: no primary source fetched
    pub struct NauticalMileUkAdmiralty = "nmi (Admiralty)", [m 1], Exact(7239 / 1, pow10 -3, pow2 8);

    /// Point (computer, DTP, 1/72 in): 1/72 in.
    ///
    /// - NIST SP 811: B.8 row label: point (computer) (1/72 in)
    pub struct PointDtp = "pt (DTP)", [m 1], Exact(127 / 9, pow10 -4, pow2 -2);

    /// Pica (computer, DTP, 1/6 in): 1/6 in.
    ///
    /// - NIST SP 811: B.8 row label: pica (computer) (1/6 in)
    pub struct PicaDtp = "pc (DTP)", [m 1], Exact(127 / 3, pow10 -4);

    /// Point (printer's): 0.013 837 in.
    ///
    /// - NIST HB 44 App. C: §5 Units of length: 1 point = 0.013 837 inch (exactly)
    /// - 1959 yard and pound (24 FR 5348): Units of length: point (printers)
    /// - NIST SP 811: B.8 row of this unit
    pub struct PointPrinters = "pt (printer's)", [m 1], Exact(1757299 / 1, pow10 -10, pow2 1);

    /// Pica (printer's): 12 printer's points.
    ///
    /// - Unsourced: pica = 12 points not stated in the fetched texts; SP 811 B.8 prints the matching value
    /// - NIST SP 811: B.8 row of this unit
    pub struct PicaPrinters = "pc (printer's)", [m 1], Exact(5271897 / 1, pow10 -10, pow2 3);

    /// Square inch: in².
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct SquareInch = "in²", [m 2], Exact(16129 / 1, pow10 -8, pow2 2);

    /// Square foot: 144 in².
    ///
    /// - NIST HB 44 App. C: §2 Units of area
    /// - NIST SP 811: B.8 row of this unit
    pub struct SquareFoot = "ft²", [m 2], Exact(145161 / 1, pow10 -8, pow2 6);

    /// Square yard: 9 ft².
    ///
    /// - NIST HB 44 App. C: §2 Units of area
    /// - NIST SP 811: B.8 row of this unit
    pub struct SquareYard = "yd²", [m 2], Exact(1306449 / 1, pow10 -8, pow2 6);

    /// Square mile (international): 640 acres.
    ///
    /// - NIST HB 44 App. C: §2 Units of area
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct SquareMileInternational = "mi²", [m 2], Exact(158080329 / 1, pow10 -6, pow2 14);

    /// Square mile (US survey): 640 survey acres. Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - NIST SP 811: B.8 row of this unit
    pub struct SquareMileUsSurvey = "mi² (US survey)", [m 2], Exact(9801 / 15499969, pow10 6, pow2 12);

    /// Acre (international foot): 43 560 ft² = 4840 yd².
    ///
    /// - NIST HB 44 App. C: §2 Units of area
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    pub struct AcreInternational = "ac", [m 2], Exact(158080329 / 1, pow10 -7, pow2 8);

    /// Acre (US survey): 43 560 survey ft². Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - NIST SP 811: B.8 row of this unit
    pub struct AcreUsSurvey = "ac (US survey)", [m 2], Exact(9801 / 15499969, pow10 5, pow2 6);

    /// Rood: 1210 yd².
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    pub struct Rood = "ro", [m 2], Exact(158080329 / 1, pow10 -7, pow2 6);

    /// Circular mil: area of a circle 1 mil across, π/4 mil².
    ///
    /// - Unsourced: π/4 mil² not stated in the fetched texts; SP 811 B.8 prints the matching value
    /// - NIST SP 811: B.8 row of this unit
    pub struct CircularMil = "cmil", [m 2], Exact(16129 / 1, pow10 -14, pi 1);

    /// Cubic inch: in³.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 footnote 13: 1.638 706 4 E-05 exactly
    /// - NIST SP 811: B.8 row of this unit
    pub struct CubicInch = "in³", [m 3], Exact(2048383 / 1, pow10 -12, pow2 3);

    /// Cubic foot: 1728 in³.
    ///
    /// - NIST HB 44 App. C: §2 Units of volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct CubicFoot = "ft³", [m 3], Exact(55306341 / 1, pow10 -12, pow2 9);

    /// Cubic yard: 27 ft³.
    ///
    /// - NIST HB 44 App. C: §2 Units of volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct CubicYard = "yd³", [m 3], Exact(1493271207 / 1, pow10 -12, pow2 9);

    /// Acre-foot (international foot): 43 560 ft³.
    ///
    /// - NIST HB 44 App. C: §2 footnote 4: acre-foot now also on the international foot
    /// - 85 FR 62698: survey foot deprecated after 31 Dec 2022, units on it now on the international foot
    pub struct AcreFootInternational = "ac·ft", [m 3], Exact(60228605349 / 1, pow10 -11, pow2 11);

    /// Acre-foot (US survey): 43 560 survey ft³. Legacy: survey foot deprecated after 31 Dec 2022.
    ///
    /// - NIST SP 811: B.6 US survey foot and units based on it
    /// - NIST SP 811: B.8 row of this unit
    pub struct AcreFootUsSurvey = "ac·ft (US survey)", [m 3], Exact(29403 / 61023377953, pow10 7, pow2 8);

    /// Cord (firewood): 128 ft³.
    ///
    /// - NIST HB 44 App. C: §5 Tables of equivalents, Units of capacity or volume
    /// - 1959 yard and pound (24 FR 5348): Units of volume: cord
    /// - NIST SP 811: B.8 row of this unit
    pub struct Cord = "cd", [m 3], Exact(55306341 / 1, pow10 -12, pow2 16);

    /// Register ton: 100 ft³.
    ///
    /// - Unsourced: 100 ft³ (Moorsom) not stated in the fetched texts; SP 811 B.8 prints the matching value
    /// - NIST SP 811: B.8 row of this unit
    pub struct TonRegister = "RT", [m 3], Exact(55306341 / 1, pow10 -10, pow2 9);

    /// Gallon (US liquid): 231 in³.
    ///
    /// - NIST HB 44 App. C: §2 Units of liquid volume, Apothecaries units of liquid volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct GallonUs = "gal (US)", [m 3], Exact(473176473 / 1, pow10 -12, pow2 3);

    /// Quart (US liquid): 57.75 in³ = 1/4 gal.
    ///
    /// - NIST HB 44 App. C: §2 Units of liquid volume, Apothecaries units of liquid volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct QuartUsLiquid = "liq qt (US)", [m 3], Exact(473176473 / 1, pow10 -12, pow2 1);

    /// Pint (US liquid): 28.875 in³ = 1/8 gal.
    ///
    /// - NIST HB 44 App. C: §2 Units of liquid volume, Apothecaries units of liquid volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct PintUsLiquid = "liq pt (US)", [m 3], Exact(473176473 / 1, pow10 -12);

    /// Cup (US measuring): 8 fl oz.
    ///
    /// - NIST HB 44 App. C: §5 Units of capacity or volume: 1 cup, measuring = 8 fl oz (exactly)
    /// - NIST SP 811: B.8 row of this unit
    pub struct CupUs = "cup (US)", [m 3], Exact(473176473 / 1, pow10 -12, pow2 -1);

    /// Gill (US): 1/4 pt.
    ///
    /// - NIST HB 44 App. C: §2 Units of liquid volume, Apothecaries units of liquid volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct GillUs = "gi (US)", [m 3], Exact(473176473 / 1, pow10 -12, pow2 -2);

    /// Fluid ounce (US): 1/16 pt = 1/128 gal.
    ///
    /// - NIST HB 44 App. C: §2 Units of liquid volume, Apothecaries units of liquid volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct FluidOunceUs = "fl oz (US)", [m 3], Exact(473176473 / 1, pow10 -12, pow2 -4);

    /// Tablespoon (US measuring): 1/2 fl oz.
    ///
    /// - NIST HB 44 App. C: §5 Units of capacity or volume: 1 tablespoon, measuring = 1/2 fl oz (exactly)
    /// - NIST SP 811: B.8 row of this unit
    pub struct TablespoonUs = "tbsp (US)", [m 3], Exact(473176473 / 1, pow10 -12, pow2 -5);

    /// Teaspoon (US measuring): 1/3 tbsp.
    ///
    /// - NIST HB 44 App. C: §5 Units of capacity or volume: 1 teaspoon, measuring = 1/3 tablespoon (exactly)
    /// - NIST SP 811: B.8 row of this unit
    pub struct TeaspoonUs = "tsp (US)", [m 3], Exact(157725491 / 1, pow10 -12, pow2 -5);

    /// Fluid dram (US): 1/8 fl oz.
    ///
    /// - NIST HB 44 App. C: §2 Units of liquid volume, Apothecaries units of liquid volume
    pub struct FluidDramUs = "fl dr (US)", [m 3], Exact(473176473 / 1, pow10 -12, pow2 -7);

    /// Minim (US): 1/60 fl dr.
    ///
    /// - NIST HB 44 App. C: §2 Units of liquid volume, Apothecaries units of liquid volume
    pub struct MinimUs = "min (US)", [m 3], Exact(157725491 / 1, pow10 -13, pow2 -8);

    /// Bushel (US, struck): 2150.42 in³.
    ///
    /// - NIST HB 44 App. C: §2 Units of dry volume
    /// - NIST HB 44 App. C: §5 Tables of equivalents, Units of capacity or volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct BushelUs = "bu (US)", [m 3], Exact(220244188543 / 1, pow10 -14, pow2 4);

    /// Peck (US): 1/4 bu.
    ///
    /// - NIST HB 44 App. C: §2 Units of dry volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct PeckUs = "pk (US)", [m 3], Exact(220244188543 / 1, pow10 -14, pow2 2);

    /// Quart (US dry): 1/32 bu.
    ///
    /// - NIST HB 44 App. C: §2 Units of dry volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct QuartUsDry = "dry qt (US)", [m 3], Exact(220244188543 / 1, pow10 -14, pow2 -1);

    /// Pint (US dry): 1/64 bu.
    ///
    /// - NIST HB 44 App. C: §2 Units of dry volume
    /// - NIST SP 811: B.8 row of this unit
    pub struct PintUsDry = "dry pt (US)", [m 3], Exact(220244188543 / 1, pow10 -14, pow2 -2);

    /// Barrel (petroleum, 42 US gal): 42 gal.
    ///
    /// - NIST HB 44 App. C: §5 Units of capacity or volume, footnote 22 (barrels)
    /// - NIST SP 811: B.8 row of this unit
    pub struct BarrelPetroleum = "bbl (oil)", [m 3], Exact(9936705933 / 1, pow10 -12, pow2 4);

    /// Barrel (US federal fermented liquor tax, 31 gal): 31 gal.
    ///
    /// - NIST HB 44 App. C: §5 Units of capacity or volume, footnote 22 (barrels)
    pub struct BarrelFermentedLiquor = "bbl (beer)", [m 3], Exact(14668470663 / 1, pow10 -12, pow2 3);

    /// Barrel for liquids (US state laws, 31.5 gal): 31.5 gal.
    ///
    /// - NIST HB 44 App. C: §5 Units of capacity or volume, footnote 22 (barrels)
    pub struct BarrelLiquidState = "bbl (liquid)", [m 3], Exact(29810117799 / 1, pow10 -12, pow2 2);

    /// Barrel (cistern, 36 gal): 36 gal.
    ///
    /// - NIST HB 44 App. C: §5 Units of capacity or volume, footnote 22 (barrels)
    pub struct BarrelCistern = "bbl (cistern)", [m 3], Exact(4258588257 / 1, pow10 -12, pow2 5);

    /// Barrel (US federal proof spirits, 40 gal): 40 gal.
    ///
    /// - NIST HB 44 App. C: §5 Units of capacity or volume, footnote 22 (barrels)
    pub struct BarrelProofSpirits = "bbl (spirits)", [m 3], Exact(473176473 / 1, pow10 -11, pow2 5);

    /// Barrel (US standard, dry commodities): 7056 in³.
    ///
    /// - NIST HB 44 App. C: §5 Tables of equivalents, Units of capacity or volume
    pub struct BarrelDry = "bbl (dry)", [m 3], Exact(903336903 / 1, pow10 -12, pow2 7);

    /// Barrel (US standard, cranberry): 5826 in³.
    ///
    /// - NIST HB 44 App. C: §5 Tables of equivalents, Units of capacity or volume
    pub struct BarrelCranberry = "bbl (cranberry)", [m 3], Exact(5966939679 / 1, pow10 -12, pow2 4);

    /// Cup (US nutrition labeling): 240 mL.
    ///
    /// - 21 CFR 101.9: (b)(5)(viii): for nutrition labeling, tsp 5 mL, tbsp 15 mL, cup 240 mL, fl oz 30 mL, oz 28 g
    pub struct CupFda = "cup (FDA)", [m 3], Exact(3 / 1, pow10 -5, pow2 3);

    /// Fluid ounce (US nutrition labeling): 30 mL.
    ///
    /// - 21 CFR 101.9: (b)(5)(viii): for nutrition labeling, tsp 5 mL, tbsp 15 mL, cup 240 mL, fl oz 30 mL, oz 28 g
    pub struct FluidOunceFda = "fl oz (FDA)", [m 3], Exact(3 / 1, pow10 -5);

    /// Tablespoon (US nutrition labeling): 15 mL.
    ///
    /// - 21 CFR 101.9: (b)(5)(viii): for nutrition labeling, tsp 5 mL, tbsp 15 mL, cup 240 mL, fl oz 30 mL, oz 28 g
    pub struct TablespoonFda = "tbsp (FDA)", [m 3], Exact(3 / 1, pow10 -5, pow2 -1);

    /// Teaspoon (US nutrition labeling): 5 mL.
    ///
    /// - 21 CFR 101.9: (b)(5)(viii): for nutrition labeling, tsp 5 mL, tbsp 15 mL, cup 240 mL, fl oz 30 mL, oz 28 g
    pub struct TeaspoonFda = "tsp (FDA)", [m 3], Exact(1 / 1, pow10 -5, pow2 -1);

    /// Gallon (imperial): 4.546 09 dm³.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    /// - NIST SP 811: B.8 row of this unit
    pub struct GallonImperial = "gal (imp)", [m 3], Exact(454609 / 1, pow10 -8);

    /// Quart (imperial): 1/4 gal.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    pub struct QuartImperial = "qt (imp)", [m 3], Exact(454609 / 1, pow10 -8, pow2 -2);

    /// Pint (imperial): 0.568 261 25 dm³.
    ///
    /// - UK WMA 1985: Sch. 1 Pt IV (imperial units of volume)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    pub struct PintImperial = "pt (imp)", [m 3], Exact(454609 / 1, pow10 -8, pow2 -3);

    /// Gill (imperial): 1/4 pint.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    /// - NIST SP 811: B.8 row of this unit
    pub struct GillImperial = "gi (imp)", [m 3], Exact(454609 / 1, pow10 -8, pow2 -5);

    /// Fluid ounce (imperial): 1/20 pint.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    /// - NIST SP 811: B.8 row of this unit
    pub struct FluidOunceImperial = "fl oz (imp)", [m 3], Exact(454609 / 1, pow10 -9, pow2 -4);

    /// Fluid drachm (imperial): 1/8 fl oz.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    pub struct FluidDrachmImperial = "fl dr (imp)", [m 3], Exact(454609 / 1, pow10 -9, pow2 -7);

    /// Fluid scruple (imperial): 1/3 fl drachm = 20 minims.
    ///
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    pub struct FluidScrupleImperial = "fl s (imp)", [m 3], Exact(454609 / 3, pow10 -9, pow2 -7);

    /// Minim (imperial): 1/60 fl drachm.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    pub struct MinimImperial = "min (imp)", [m 3], Exact(454609 / 3, pow10 -10, pow2 -8);

    /// Peck (imperial): 2 gal.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    pub struct PeckImperial = "pk (imp)", [m 3], Exact(454609 / 1, pow10 -8, pow2 1);

    /// Bushel (imperial): 8 gal.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    pub struct BushelImperial = "bu (imp)", [m 3], Exact(454609 / 1, pow10 -8, pow2 3);

    /// Quarter (imperial, 8 bushels): 8 bushels.
    ///
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    pub struct QuarterImperialVolume = "qr (imp vol)", [m 3], Exact(454609 / 1, pow10 -8, pow2 6);

    /// Water ton (English, 224 imperial gal): 224 imperial gal.
    ///
    /// - NIST HB 44 App. C: §5 Tables of equivalents, Units of capacity or volume
    pub struct TonWater = "ton (water)", [m 3], Exact(3182263 / 1, pow10 -8, pow2 5);

    /// Grain: 1/7000 lb.
    ///
    /// - NIST HB 44 App. C: §2 Avoirdupois units of mass
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct Grain = "gr", [kg 1], Exact(6479891 / 1, pow10 -11);

    /// Dram (avoirdupois): 1/16 oz = 27 11/32 gr.
    ///
    /// - NIST HB 44 App. C: §2 Avoirdupois units of mass
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    pub struct DramAvdp = "dr avdp", [kg 1], Exact(45359237 / 1, pow10 -8, pow2 -8);

    /// Ounce (avoirdupois): 1/16 lb = 437.5 gr.
    ///
    /// - NIST HB 44 App. C: §2 Avoirdupois units of mass
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct OunceAvdp = "oz avdp", [kg 1], Exact(45359237 / 1, pow10 -8, pow2 -4);

    /// Pound (avoirdupois): 0.453 592 37 kg.
    ///
    /// - 1959 yard and pound (24 FR 5348): 1 lb = 0.453 592 37 kg exactly
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 footnote 22: 4.535 923 7 E-01 exactly
    /// - NIST SP 811: B.8 row of this unit
    pub struct PoundAvdp = "lb avdp", [kg 1], Exact(45359237 / 1, pow10 -8);

    /// Stone: 14 lb.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    pub struct Stone = "st", [kg 1], Exact(317514659 / 1, pow10 -8, pow2 1);

    /// Quarter (British, 28 lb): 28 lb.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST HB 44 App. C: §3 Notes on British units of measurement
    pub struct QuarterMass = "qr (mass)", [kg 1], Exact(317514659 / 1, pow10 -8, pow2 2);

    /// Cental: 100 lb.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    pub struct Cental = "ctl", [kg 1], Exact(45359237 / 1, pow10 -6);

    /// Hundredweight (short, net): 100 lb.
    ///
    /// - NIST HB 44 App. C: §2 Avoirdupois units of mass
    /// - NIST SP 811: B.8 row of this unit
    pub struct HundredweightShort = "cwt (short)", [kg 1], Exact(45359237 / 1, pow10 -6);

    /// Hundredweight (long, gross): 112 lb.
    ///
    /// - NIST HB 44 App. C: §2 Avoirdupois units of mass
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct HundredweightLong = "cwt (long)", [kg 1], Exact(317514659 / 1, pow10 -8, pow2 4);

    /// Ton (short, net): 2000 lb.
    ///
    /// - NIST HB 44 App. C: §2 Avoirdupois units of mass
    /// - NIST SP 811: B.8 row of this unit
    pub struct TonShort = "tn", [kg 1], Exact(45359237 / 1, pow10 -5, pow2 1);

    /// Ton (long, gross): 2240 lb.
    ///
    /// - NIST HB 44 App. C: §2 Avoirdupois units of mass
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct TonLong = "ton (long)", [kg 1], Exact(317514659 / 1, pow10 -7, pow2 5);

    /// Tonne (metric ton): 1000 kg.
    ///
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct Tonne = "t", [kg 1], Exact(1 / 1, pow10 3);

    /// Ounce (US nutrition labeling, 28 g): 28 g.
    ///
    /// - 21 CFR 101.9: (b)(5)(viii): for nutrition labeling, tsp 5 mL, tbsp 15 mL, cup 240 mL, fl oz 30 mL, oz 28 g
    pub struct OunceFda = "oz (FDA)", [kg 1], Exact(7 / 1, pow10 -3, pow2 2);

    /// Pennyweight: 24 gr.
    ///
    /// - NIST HB 44 App. C: §2 Troy units of mass
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct Pennyweight = "dwt", [kg 1], Exact(19439673 / 1, pow10 -11, pow2 3);

    /// Ounce (troy): 480 gr = 0.031 103 476 8 kg.
    ///
    /// - NIST HB 44 App. C: §2 Troy units of mass
    /// - UK WMA 1985: Sch. 1 Pt V (imperial units of mass)
    /// - NIST SP 811: B.8 row of this unit
    pub struct OunceTroy = "oz t", [kg 1], Exact(19439673 / 1, pow10 -10, pow2 4);

    /// Pound (troy): 12 oz t = 5760 gr.
    ///
    /// - NIST HB 44 App. C: §2 Troy units of mass
    /// - NIST SP 811: B.8 row of this unit
    pub struct PoundTroy = "lb t", [kg 1], Exact(58319019 / 1, pow10 -10, pow2 6);

    /// Scruple (apothecaries): 20 gr.
    ///
    /// - NIST HB 44 App. C: §2 Apothecaries units of mass
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    pub struct ScrupleApothecaries = "s ap", [kg 1], Exact(6479891 / 1, pow10 -10, pow2 1);

    /// Dram, drachm (apothecaries): 3 scruples = 60 gr.
    ///
    /// - NIST HB 44 App. C: §2 Apothecaries units of mass
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    pub struct DramApothecaries = "dr ap", [kg 1], Exact(19439673 / 1, pow10 -10, pow2 1);

    /// Ounce (apothecaries): 480 gr.
    ///
    /// - NIST HB 44 App. C: §2 Apothecaries units of mass
    /// - UK WMA 1985: Sch. 1 Pt VI (supplementary and other units)
    /// - NIST SP 811: B.8 row of this unit
    pub struct OunceApothecaries = "oz ap", [kg 1], Exact(19439673 / 1, pow10 -10, pow2 4);

    /// Pound (apothecaries): 12 oz ap = 5760 gr.
    ///
    /// - NIST HB 44 App. C: §2 Apothecaries units of mass
    /// - NIST SP 811: B.8 row of this unit
    pub struct PoundApothecaries = "lb ap", [kg 1], Exact(58319019 / 1, pow10 -10, pow2 6);

    /// Slug: lbf·s²/ft.
    ///
    /// - Unsourced: lbf·s²/ft not stated in the fetched texts; SP 811 B.8 prints the matching value
    /// - NIST SP 811: B.8 row of this unit
    pub struct Slug = "slug", [kg 1], Exact(8896443230521 / 381, pow10 -8, pow2 -4);

    /// Pound-force: lb · g_n.
    ///
    /// - 1959 yard and pound (24 FR 5348): 1 lb = 0.453 592 37 kg exactly
    /// - 3rd CGPM (1901): g_n = 980.665 cm/s²
    /// - NIST SP 811: B.8 footnote 23: 4.448 221 615 260 5 E+00 exactly with g_n
    /// - NIST SP 811: B.8 row of this unit
    pub struct PoundForce = "lbf", [s -2, m 1, kg 1], Exact(8896443230521 / 1, pow10 -12, pow2 -1);

    /// Ounce-force (avoirdupois): oz · g_n.
    ///
    /// - NIST HB 44 App. C: §2 Avoirdupois units of mass
    /// - 3rd CGPM (1901): g_n = 980.665 cm/s²
    /// - NIST SP 811: B.8 row of this unit
    pub struct OunceForce = "ozf", [s -2, m 1, kg 1], Exact(8896443230521 / 1, pow10 -12, pow2 -5);

    /// Kip (1000 lbf): 1000 lbf.
    ///
    /// - NIST SP 811: B.8 row label: kip (1 kip = 1000 lbf)
    pub struct Kip = "kip", [s -2, m 1, kg 1], Exact(8896443230521 / 1, pow10 -9, pow2 -1);

    /// Ton-force (short, 2000 lbf): 2000 lbf.
    ///
    /// - NIST SP 811: B.8 row label: ton-force (2000 lbf)
    pub struct TonForceShort = "tnf", [s -2, m 1, kg 1], Exact(8896443230521 / 1, pow10 -9);

    /// Poundal: lb·ft/s².
    ///
    /// - Unsourced: lb·ft/s² not stated in the fetched texts; SP 811 B.8 prints the matching value
    /// - NIST SP 811: B.8 row of this unit
    pub struct Poundal = "pdl", [s -2, m 1, kg 1], Exact(17281869297 / 1, pow10 -12, pow2 3);

    /// Kilogram-force (kilopond): kg · g_n.
    ///
    /// - 3rd CGPM (1901): g_n = 980.665 cm/s²
    /// - NIST SP 811: B.8 row of this unit
    pub struct KilogramForce = "kgf", [s -2, m 1, kg 1], Exact(196133 / 1, pow10 -4, pow2 -1);

    /// Pound-force per square inch: lbf/in².
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct Psi = "psi", [s -2, m -1, kg 1], Exact(8896443230521 / 16129, pow10 -4, pow2 -3);

    /// Kip per square inch: kip/in².
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct Ksi = "ksi", [s -2, m -1, kg 1], Exact(8896443230521 / 16129, pow10 -1, pow2 -3);

    /// Pound-force per square foot: lbf/ft².
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct Psf = "psf", [s -2, m -1, kg 1], Exact(8896443230521 / 145161, pow10 -4, pow2 -7);

    /// Inch of mercury (conventional): 1 in · 13 595.1 kg/m³ · g_n.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - Unsourced: conventional mercury density 13 595.1 kg/m³ is ISO 80000-4, not fetched
    /// - NIST SP 811: B.8 row of this unit
    pub struct InchOfMercuryConventional = "inHg (conv)", [s -2, m -1, kg 1], Exact(3386388640341 / 1, pow10 -9);

    /// Foot of mercury (conventional): 1 ft · 13 595.1 kg/m³ · g_n.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - Unsourced: conventional mercury density 13 595.1 kg/m³ is ISO 80000-4, not fetched
    /// - NIST SP 811: B.8 row of this unit
    pub struct FootOfMercuryConventional = "ftHg (conv)", [s -2, m -1, kg 1], Exact(10159165921023 / 1, pow10 -9, pow2 2);

    /// Millimetre of mercury (conventional): 1 mm · 13 595.1 kg/m³ · g_n.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - Unsourced: conventional mercury density 13 595.1 kg/m³ is ISO 80000-4, not fetched
    /// - NIST SP 811: B.8 row of this unit
    pub struct MillimetreOfMercuryConventional = "mmHg (conv)", [s -2, m -1, kg 1], Exact(26664477483 / 1, pow10 -8, pow2 -1);

    /// Millimetre of mercury (EU legal, blood pressure): 133.322 Pa.
    ///
    /// - 80/181/EEC: Annex ch. II: 1 mm Hg = 133,322 Pa (blood pressure)
    /// - SI Brochure 8: Table 8: 1 mmHg ≈ 133.322 Pa
    pub struct MillimetreOfMercuryEu = "mmHg (EU)", [s -2, m -1, kg 1], Exact(66661 / 1, pow10 -3, pow2 1);

    /// Torr: 1/760 atm.
    ///
    /// - Unsourced: 1/760 atm not stated in the fetched texts; SP 811 B.8 prints the matching value
    /// - NIST SP 811: B.8 row of this unit
    pub struct Torr = "Torr", [s -2, m -1, kg 1], Exact(4053 / 19, pow10 1, pow2 -4);

    /// Inch of mercury (32 °F): measured: mercury density at 32 °F. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - NIST SP 811: B.8 row of this unit
    pub struct InchOfMercury32f = "inHg (32 °F)", [s -2, m -1, kg 1], Exact(169319 / 1, pow10 -2, pow2 1), conventional("NIST SP 811", 2008);

    /// Inch of mercury (60 °F): measured: mercury density at 60 °F. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - NIST SP 811: B.8 row of this unit
    pub struct InchOfMercury60f = "inHg (60 °F)", [s -2, m -1, kg 1], Exact(67537 / 1, pow10 -1, pow2 -1), conventional("NIST SP 811", 2008);

    /// Inch of water (conventional): 1 in · 1000 kg/m³ · g_n.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - NIST SP 811: B.8 row of this unit
    pub struct InchOfWaterConventional = "inH2O (conv)", [s -2, m -1, kg 1], Exact(24908891 / 1, pow10 -5);

    /// Foot of water (conventional): 1 ft · 1000 kg/m³ · g_n.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - NIST SP 811: B.8 row of this unit
    pub struct FootOfWaterConventional = "ftH2O (conv)", [s -2, m -1, kg 1], Exact(74726673 / 1, pow10 -5, pow2 2);

    /// Millimetre of water (conventional): 1 mm · 1000 kg/m³ · g_n.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - NIST SP 811: B.8 row of this unit
    pub struct MillimetreOfWaterConventional = "mmH2O (conv)", [s -2, m -1, kg 1], Exact(196133 / 1, pow10 -4, pow2 -1);

    /// Inch of water (39.2 °F): measured: water density at 39.2 °F. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - NIST SP 811: B.8 row of this unit
    pub struct InchOfWater39f = "inH2O (39.2 °F)", [s -2, m -1, kg 1], Exact(124541 / 1, pow10 -3, pow2 1), conventional("NIST SP 811", 2008);

    /// Inch of water (60 °F): measured: water density at 60 °F. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - NIST SP 811: B.8 row of this unit
    pub struct InchOfWater60f = "inH2O (60 °F)", [s -2, m -1, kg 1], Exact(6221 / 1, pow10 -2, pow2 2), conventional("NIST SP 811", 2008);

    /// Foot of water (39.2 °F): measured: water density at 39.2 °F. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 footnote 12: manometer units from g_n and the liquid's density; conventional ones per ISO 80000
    /// - NIST SP 811: B.8 row of this unit
    pub struct FootOfWater39f = "ftH2O (39.2 °F)", [s -2, m -1, kg 1], Exact(149449 / 1, pow10 -2, pow2 1), conventional("NIST SP 811", 2008);

    /// Atmosphere (standard): 101 325 Pa.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct AtmosphereStandard = "atm", [s -2, m -1, kg 1], Exact(4053 / 1, pow10 2, pow2 -2);

    /// Atmosphere (technical, kgf/cm²): 1 kgf/cm².
    ///
    /// - NIST SP 811: B.8 footnote 8: 1 at = 1 kgf/cm²
    /// - NIST SP 811: B.8 row of this unit
    pub struct AtmosphereTechnical = "at", [s -2, m -1, kg 1], Exact(196133 / 1, pow2 -1);

    /// Foot pound-force: ft · lbf.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct FootPoundForce = "ft·lbf", [s -2, m 2, kg 1], Exact(3389544870828501 / 1, pow10 -16, pow2 2);

    /// Foot poundal: ft · pdl.
    ///
    /// - Unsourced: poundal = lb·ft/s² not stated in the fetched texts
    /// - NIST SP 811: B.8 row of this unit
    pub struct FootPoundal = "ft·pdl", [s -2, m 2, kg 1], Exact(6584392202157 / 1, pow10 -16, pow2 6);

    /// British thermal unit (International Table): 1.055 055 852 62 kJ; cal_IT/(g·°C) · lb · °F.
    ///
    /// - NIST SP 811: B.8 footnote 9: cal_IT = 4.1868 J (5th Int. Conf. Properties of Steam 1956), Btu_IT = 1.055 055 852 62 kJ exactly; cal_th = 4.184 J, Btu_th based on it
    /// - NIST SP 811: B.8 row of this unit
    pub struct BtuIt = "Btu_IT", [s -2, m 2, kg 1], Exact(52752792631 / 1, pow10 -8, pow2 1);

    /// British thermal unit (thermochemical): cal_th/(g·°C) · lb · °F.
    ///
    /// - NIST SP 811: B.8 footnote 9: cal_IT = 4.1868 J (5th Int. Conf. Properties of Steam 1956), Btu_IT = 1.055 055 852 62 kJ exactly; cal_th = 4.184 J, Btu_th based on it
    /// - NIST SP 811: B.8 row of this unit
    pub struct BtuTh = "Btu_th", [s -2, m 2, kg 1], Exact(23722880951 / 9, pow10 -7, pow2 2);

    /// British thermal unit (mean): measured. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct BtuMean = "Btu_mean", [s -2, m 2, kg 1], Exact(105587 / 1, pow10 -2), conventional("NIST SP 811", 2008);

    /// British thermal unit (39 °F): measured: water at 39 °F. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct Btu39f = "Btu_39", [s -2, m 2, kg 1], Exact(105967 / 1, pow10 -2), conventional("NIST SP 811", 2008);

    /// British thermal unit (59 °F): measured: water at 59 °F. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct Btu59f = "Btu_59", [s -2, m 2, kg 1], Exact(2637 / 1, pow10 -1, pow2 2), conventional("NIST SP 811", 2008);

    /// British thermal unit (60 °F): measured: water at 60 °F. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct Btu60f = "Btu_60", [s -2, m 2, kg 1], Exact(26367 / 1, pow10 -2, pow2 2), conventional("NIST SP 811", 2008);

    /// Calorie (International Table): 4.1868 J.
    ///
    /// - NIST SP 811: B.8 footnote 9: cal_IT = 4.1868 J (5th Int. Conf. Properties of Steam 1956), Btu_IT = 1.055 055 852 62 kJ exactly; cal_th = 4.184 J, Btu_th based on it
    /// - NIST SP 811: B.8 row of this unit
    pub struct CalorieIt = "cal_IT", [s -2, m 2, kg 1], Exact(10467 / 1, pow10 -4, pow2 2);

    /// Calorie (thermochemical): 4.184 J.
    ///
    /// - NIST SP 811: B.8 footnote 9: cal_IT = 4.1868 J (5th Int. Conf. Properties of Steam 1956), Btu_IT = 1.055 055 852 62 kJ exactly; cal_th = 4.184 J, Btu_th based on it
    /// - NIST SP 811: B.8 row of this unit
    pub struct CalorieTh = "cal_th", [s -2, m 2, kg 1], Exact(523 / 1, pow10 -3, pow2 3);

    /// Calorie (mean): measured. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct CalorieMean = "cal_mean", [s -2, m 2, kg 1], Exact(209501 / 1, pow10 -5, pow2 1), conventional("NIST SP 811", 2008);

    /// Calorie (15 °C): measured: water at 15 °C. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct Calorie15c = "cal_15", [s -2, m 2, kg 1], Exact(20929 / 1, pow10 -4, pow2 1), conventional("NIST SP 811", 2008);

    /// Calorie (20 °C): measured: water at 20 °C. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct Calorie20c = "cal_20", [s -2, m 2, kg 1], Exact(41819 / 1, pow10 -4), conventional("NIST SP 811", 2008);

    /// Kilocalorie (International Table): 1000 cal_IT.
    ///
    /// - NIST SP 811: B.8 footnote 9: cal_IT = 4.1868 J (5th Int. Conf. Properties of Steam 1956), Btu_IT = 1.055 055 852 62 kJ exactly; cal_th = 4.184 J, Btu_th based on it
    /// - NIST SP 811: B.8 row of this unit
    pub struct KilocalorieIt = "kcal_IT", [s -2, m 2, kg 1], Exact(10467 / 1, pow10 -1, pow2 2);

    /// Kilocalorie (thermochemical): 1000 cal_th.
    ///
    /// - NIST SP 811: B.8 footnote 9: cal_IT = 4.1868 J (5th Int. Conf. Properties of Steam 1956), Btu_IT = 1.055 055 852 62 kJ exactly; cal_th = 4.184 J, Btu_th based on it
    /// - NIST SP 811: B.8 row of this unit
    pub struct KilocalorieTh = "kcal_th", [s -2, m 2, kg 1], Exact(523 / 1, pow2 3);

    /// Calorie (food, kilogram calorie IT): 1 kcal_IT.
    ///
    /// - NIST SP 811: B.8 footnote 11 and row calorie_IT, kilogram (nutrition)
    pub struct CalorieFoodIt = "Cal_IT", [s -2, m 2, kg 1], Exact(10467 / 1, pow10 -1, pow2 2);

    /// Calorie (food, kilogram calorie thermochemical): 1 kcal_th.
    ///
    /// - NIST SP 811: B.8 footnote 11 and row calorie_th, kilogram (nutrition)
    pub struct CalorieFoodTh = "Cal_th", [s -2, m 2, kg 1], Exact(523 / 1, pow2 3);

    /// Therm (EC): 105.506 MJ.
    ///
    /// - NIST SP 811: B.8 footnote 24
    /// - 80/181/EEC: Annex ch. III: 1 therm = 105,506 × 10⁶ J
    /// - NIST SP 811: B.8 row of this unit
    pub struct ThermEc = "thm (EC)", [s -2, m 2, kg 1], Exact(52753 / 1, pow10 3, pow2 1);

    /// Therm (US): 105.4804 MJ (Federal Register 27 Jul 1968).
    ///
    /// - NIST SP 811: B.8 footnote 24
    /// - NIST SP 811: B.8 row of this unit
    pub struct ThermUs = "thm (US)", [s -2, m 2, kg 1], Exact(263701 / 1, pow10 2, pow2 2);

    /// Quad (10¹⁵ Btu_IT): 10¹⁵ Btu_IT.
    ///
    /// - NIST SP 811: B.8 row label: quad (10¹⁵ Btu_IT)
    pub struct Quad = "quad", [s -2, m 2, kg 1], Exact(52752792631 / 1, pow10 7, pow2 1);

    /// Ton of TNT (energy equivalent): 10⁹ cal_th = 4.184 GJ.
    ///
    /// - NIST SP 811: B.8 footnote 25: defined, not measured
    /// - NIST SP 811: B.8 row of this unit
    pub struct TonOfTnt = "tTNT", [s -2, m 2, kg 1], Exact(523 / 1, pow10 6, pow2 3);

    /// Horsepower hour (mechanical): hp · 3600 s.
    ///
    /// - NIST SP 811: B.8 row horsepower (550 ft·lbf/s)
    pub struct HorsepowerHour = "hp·h", [s -2, m 2, kg 1], Exact(335564942212021599 / 1, pow10 -12, pow2 3);

    /// Horsepower (mechanical, 550 ft·lbf/s): 550 ft·lbf/s.
    ///
    /// - NIST SP 811: B.8 row label: horsepower (550 ft·lbf/s)
    pub struct HorsepowerMechanical = "hp (mech)", [s -3, m 2, kg 1], Exact(37284993579113511 / 1, pow10 -14, pow2 1);

    /// Horsepower (metric, PS): 75 kgf·m/s.
    ///
    /// - Unsourced: 75 kgf·m/s is DIN 66036, not fetched; SP 811 B.8 prints the matching value
    /// - NIST SP 811: B.8 row of this unit
    pub struct HorsepowerMetric = "PS", [s -3, m 2, kg 1], Exact(588399 / 1, pow10 -2, pow2 -3);

    /// Horsepower (electric): 746 W.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct HorsepowerElectric = "hp (elec)", [s -3, m 2, kg 1], Exact(373 / 1, pow2 1);

    /// Horsepower (boiler): evaporation rate of water; no exact definition fits SP 811's value. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct HorsepowerBoiler = "hp (boiler)", [s -3, m 2, kg 1], Exact(19619 / 1, pow2 -1), conventional("NIST SP 811", 2008);

    /// Horsepower (water): measured. Measured, not defined: the value NIST SP 811 B.8 prints.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct HorsepowerWater = "hp (water)", [s -3, m 2, kg 1], Exact(746043 / 1, pow10 -3), conventional("NIST SP 811", 2008);

    /// Btu_IT per hour: Btu_IT/h.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct BtuItPerHour = "Btu_IT/h", [s -3, m 2, kg 1], Exact(52752792631 / 9, pow10 -10, pow2 -1);

    /// Ton of refrigeration (12 000 Btu_IT/h): 12 000 Btu_IT/h.
    ///
    /// - NIST SP 811: B.8 row label: ton of refrigeration (12 000 Btu_IT/h)
    pub struct TonOfRefrigeration = "TR", [s -3, m 2, kg 1], Exact(52752792631 / 3, pow10 -7, pow2 1);

    /// Mile per hour: mi/h.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct MilePerHour = "mph", [s -1, m 1], Exact(1397 / 1, pow10 -5, pow2 5);

    /// Knot: 1 nmi/h = 1852/3600 m/s.
    ///
    /// - SI Brochure 8: Table 8: 1 kn = (1852/3600) m/s
    pub struct Knot = "kn", [s -1, m 1], Exact(463 / 9, pow10 -2);

    /// Foot per second: ft/s.
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct FootPerSecond = "ft/s", [s -1, m 1], Exact(381 / 1, pow10 -4, pow2 3);

    /// Foot per second squared: ft/s².
    ///
    /// - NIST SP 811: B.8 row of this unit
    pub struct FootPerSecondSquared = "ft/s²", [s -2, m 1], Exact(381 / 1, pow10 -4, pow2 3);

    /// Minute of arc: 1/60°.
    ///
    /// - SI Brochure 8: Table 6: 1° = (π/180) rad, 1′ = (1/60)°, 1″ = (1/60)′
    pub struct Arcminute = "′", [rad 1], Exact(1 / 27, pow10 -2, pow2 -2, pi 1);

    /// Second of arc: 1/60′.
    ///
    /// - SI Brochure 8: Table 6: 1° = (π/180) rad, 1′ = (1/60)°, 1″ = (1/60)′
    pub struct Arcsecond = "″", [rad 1], Exact(1 / 81, pow10 -3, pow2 -3, pi 1);

    /// Gon (grade): 0.9°.
    ///
    /// - NIST SP 811: B.8 rows gon: 9.0 E-01 degree (bold)
    pub struct Gon = "gon", [rad 1], Exact(1 / 1, pow10 -2, pow2 -1, pi 1);

    /// Mil (angle, NATO, 1/6400 turn): 0.056 25° = 1/6400 turn.
    ///
    /// - NIST SP 811: B.8 rows mil (angle): 5.625 E-02 degree (bold)
    pub struct MilNato = "mil (NATO)", [rad 1], Exact(1 / 1, pow10 -2, pow2 -5, pi 1);

    /// Mil (angle, Warsaw Pact, 1/6000 turn): 1/6000 turn.
    ///
    /// - Unsourced: 1/6000 turn: no primary source fetched
    pub struct MilWarsawPact = "mil (WP)", [rad 1], Exact(1 / 3, pow10 -3, pi 1);

    /// Streck (angle, Swedish, 1/6300 turn): 1/6300 turn.
    ///
    /// - Unsourced: 1/6300 turn: no primary source fetched
    pub struct StreckSwedish = "streck", [rad 1], Exact(1 / 63, pow10 -2, pow2 1, pi 1);
}
