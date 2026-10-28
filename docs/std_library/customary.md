---
level: basic
---

# Imperial and US customary units

`exact_units` has a marker for every imperial, US customary and other non-SI unit still in use (feature `customary`,
part of `ww_si`'s `quantities`): inches, pounds, gallons, psi, Btu, horsepower, slugs, knots, mils. Each one's scale
is the exact rational of SI its legal definition gives, with the source cited in its docs, so a value in pound-force
seconds converts to newton seconds exactly, or doesn't convert at all.

```rust
use exact_units::{Fixed, prefix::{Milli, One, Pico}, units::{Newton, PoundForce, Second, Times}};

type LbfSeconds = Fixed<Times<PoundForce, Second>, Milli, i32>;

let impulse = LbfSeconds::new(2000); // 2 lbf·s
// 1 lbf·s is 4.4482216152605 N·s exactly
assert_eq!(
    impulse.convert::<Times<Newton, Second>, Pico, i64>(),
    Some(Fixed::new(8_896_443_230_521))
);
// 8.896443230521 N·s is not a whole number of mN·s: refused, not rounded
assert_eq!(impulse.convert::<Times<Newton, Second>, Milli, i64>(), None);
```

On the wire a `Fixed` in a customary unit is its integer, like any other `Fixed`: the unit is in the API, not in the
bytes. The markers are zero-size types, and `convert` costs what `Fixed::to` costs: its factor is worked out at
compile time.

## No bare ambiguous names

"Gallon", "ton", "ounce", "BTU" or "horsepower" each mean several units that differ by up to a fifth. None of them is
a marker: there is `GallonUs` and `GallonImperial`, `TonShort`, `TonLong` and `Tonne`, `BtuIt` and `BtuTh`. A test
checks that no marker is named by a word of the table below, and that every unit called after such a word is in its
group. The one exception is `Degree`, the angle: SI names it so, and a temperature degree is always "degree
Fahrenheit" or "degree Celsius".

<!-- generated: ambiguous -->

| Name | Symbols | Markers | Why |
|---|---|---|---|
| gallon | `gal` | `GallonUs`, `GallonImperial` | US 231 in³ vs imperial 4.546 09 L (20 % larger) |
| quart | `qt` | `QuartUsLiquid`, `QuartUsDry`, `QuartImperial` | HB 44 §2 note 8: say "dry" to tell the dry quart from the liquid one |
| pint | `pt` | `PintUsLiquid`, `PintUsDry`, `PintImperial` | 80/181/EEC also lists a rounded pint 0.5683 dm³ for UK/IE; the definition is WMA 1985's 0.568 261 25 dm³ |
| gill | `gi` | `GillUs`, `GillImperial` | US 1/4 US pint vs imperial 1/4 imperial pint |
| fluid ounce | `fl oz`, `fl. oz.` | `FluidOunceUs`, `FluidOunceImperial`, `FluidOunceFda` | US 1/128 gal, imperial 1/160 gal, US labeling 30 mL; 80/181/EEC's rounded 28.41 cm³ is not a definition |
| ounce | `oz` | `OunceAvdp`, `OunceTroy`, `OunceApothecaries`, `OunceFda`, `OunceForce`, `FluidOunceUs`, `FluidOunceImperial`, `FluidOunceFda` | HB 44 §2 note 9 reads an unqualified ounce as avoirdupois; troy and apothecaries 480 gr, avdp 437.5 gr; also used for the fluid ounce |
| pound | `lb`, `#` | `PoundAvdp`, `PoundTroy`, `PoundApothecaries`, `PoundForce` | HB 44 §2 note 9 reads an unqualified pound as avoirdupois; lbm (mass) vs lbf (force) are different dimensions |
| dram | `dr` | `DramAvdp`, `DramApothecaries`, `FluidDramUs` | avdp 1/16 oz (27.34 gr) vs apothecaries 60 gr vs the US fluid dram |
| drachm |  | `DramApothecaries`, `FluidDrachmImperial` | British spelling of the apothecaries dram; also the imperial fluid drachm |
| fluid dram | `fl dr` | `FluidDramUs`, `FluidDrachmImperial` | 1/8 US fl oz vs 1/8 imperial fl oz |
| minim | `min`, `♏` | `MinimUs`, `MinimImperial` | 1/480 US fl oz vs 1/480 imperial fl oz; "min" is also the minute |
| scruple |  | `ScrupleApothecaries`, `FluidScrupleImperial` | 20 gr (mass) vs the British fluid scruple (volume) |
| bushel | `bu` | `BushelUs`, `BushelImperial` | US 2150.42 in³ vs imperial 8 gal (3 % larger); heaped bushels are customs, not here |
| peck | `pk` | `PeckUs`, `PeckImperial` | 1/4 of the respective bushel |
| cup | `c` | `CupUs`, `CupFda` | US measuring cup 8 fl oz (236.6 mL) vs US labeling 240 mL; metric (250 mL) and imperial cups are not defined by the fetched sources |
| tablespoon | `tbsp`, `T` | `TablespoonUs`, `TablespoonFda` | US measuring 1/2 fl oz vs US labeling 15 mL |
| teaspoon | `tsp`, `t.` | `TeaspoonUs`, `TeaspoonFda` | US measuring 1/6 fl oz vs US labeling 5 mL |
| barrel | `bbl` | `BarrelPetroleum`, `BarrelFermentedLiquor`, `BarrelLiquidState`, `BarrelCistern`, `BarrelProofSpirits`, `BarrelDry`, `BarrelCranberry` | HB 44 footnote 22: 31 to 42 gal by law or industry; consult the application's regulation |
| ton | `ton`, `T` | `TonShort`, `TonLong`, `Tonne`, `TonForceShort`, `TonOfRefrigeration`, `TonOfTnt`, `TonWater`, `TonRegister` | HB 44 §2 note 11: "tn" is the required abbreviation of the short ton; note 10: unmodified "ton" is commonly the 2000 lb short ton in the US, 2240 lb in WMA 1985; mass, force, power, energy and volume all use the name |
| hundredweight | `cwt` | `HundredweightShort`, `HundredweightLong` | US 100 lb vs British 112 lb (HB 44 §2 note 10, §3) |
| quarter | `qr` | `QuarterMass`, `QuarterImperialVolume` | British 28 lb vs the British dry quarter of 8 bushels |
| foot |  | `FootInternational`, `FootUsSurvey` | 1 ft = 0.999 998 survey ft; since 1 Jan 2023 "foot" means the international foot (85 FR 62698), so ft is its symbol and the survey foot is qualified |
| mile |  | `MileInternational`, `MileUsSurvey`, `NauticalMileInternational`, `NauticalMileUsPre1954`, `NauticalMileUkAdmiralty` | statute (international) vs survey vs nautical; since 1 Jan 2023 "mile" is the international mile, mi its symbol |
| nautical mile | `NM`, `M`, `Nm` | `NauticalMileInternational`, `NauticalMileUsPre1954`, `NauticalMileUkAdmiralty` | 1852 m since 1929 (IHO), the US 1853.248 m until 1954, the UK Admiralty 6080 ft until 1970 |
| square mile |  | `SquareMileInternational`, `SquareMileUsSurvey` | international vs survey foot |
| acre |  | `AcreInternational`, `AcreUsSurvey` | international vs survey foot (4 ppm) |
| acre foot | `acre-ft` | `AcreFootInternational`, `AcreFootUsSurvey` | international vs survey foot |
| link |  | `LinkInternational`, `LinkUsSurvey` | international vs survey foot |
| rod |  | `RodInternational`, `RodUsSurvey` | international vs survey foot; also pole, perch |
| chain |  | `ChainInternational`, `ChainUsSurvey` | international vs survey foot |
| furlong |  | `FurlongInternational`, `FurlongUsSurvey` | international vs survey foot |
| fathom |  | `FathomInternational`, `FathomUsSurvey` | international vs survey foot |
| league |  | `LeagueInternational`, `LeagueUsSurvey` | international vs survey foot; the nautical league (3 nmi) is not defined by the fetched sources |
| cable |  | `CableUs` | US 720 ft (120 fathoms); the British cable (1/10 nmi) and others are not defined by the fetched sources |
| mil | `mil` | `Thou`, `MilNato`, `MilWarsawPact`, `StreckSwedish`, `Fixed<Radian, Milli, _>`, `CircularMil` | thickness 0.001 in vs angular mils: NATO 6400, Warsaw Pact 6000, Swedish 6300 per turn, the milliradian (~6283 per turn) |
| point | `pt` | `PointDtp`, `PointPrinters` | 1/72 in vs 0.013 837 in (HB 44 §5: "1/72 inch (approximately)"); Didot and other points are not defined by the fetched sources |
| pica | `pc` | `PicaDtp`, `PicaPrinters` | 1/6 in vs 12 printer's points |
| btu | `Btu`, `BTU` | `BtuIt`, `BtuTh`, `BtuMean`, `Btu39f`, `Btu59f`, `Btu60f` | IT and thermochemical are defined; mean, 39, 59 and 60 °F are measured values |
| calorie | `cal`, `Cal`, `kcal` | `CalorieIt`, `CalorieTh`, `CalorieMean`, `Calorie15c`, `Calorie20c`, `KilocalorieIt`, `KilocalorieTh`, `CalorieFoodIt`, `CalorieFoodTh` | the food Calorie is a kilocalorie (SP 811 footnote 11); IT 4.1868 J vs thermochemical 4.184 J |
| therm | `thm` | `ThermEc`, `ThermUs` | EC 105.506 MJ (based on Btu_IT) vs US 105.4804 MJ, the US gas industry's legal unit (SP 811 footnote 24) |
| horsepower | `hp`, `HP` | `HorsepowerMechanical`, `HorsepowerMetric`, `HorsepowerElectric`, `HorsepowerBoiler`, `HorsepowerWater` | mechanical 550 ft·lbf/s (also SP 811's "horsepower (U.K.)", 745.70 W), metric 75 kgf·m/s, electric 746 W, boiler and water measured |
| inch of mercury | `inHg`, `in Hg`, `"Hg` | `InchOfMercuryConventional`, `InchOfMercury32f`, `InchOfMercury60f` | conventional (ISO 80000) vs at a stated temperature (measured) |
| millimetre of mercury | `mmHg`, `mm Hg` | `MillimetreOfMercuryConventional`, `MillimetreOfMercuryEu`, `Torr` | conventional 133.322 387 415 Pa vs EU legal 133.322 Pa vs torr 101 325/760 Pa (133.322 368 Pa) |
| inch of water | `inH2O`, `in H2O`, `inWC`, `"WC` | `InchOfWaterConventional`, `InchOfWater39f`, `InchOfWater60f` | conventional (1000 kg/m³) vs at a stated temperature (measured) |
| foot of water | `ftH2O` | `FootOfWaterConventional`, `FootOfWater39f` | conventional vs at 39.2 °F (measured) |
| atmosphere |  | `AtmosphereStandard`, `AtmosphereTechnical` | 101 325 Pa vs 1 kgf/cm² = 98 066.5 Pa |
| degree | `deg` | `DegreeAngle`, `origins::Fahrenheit`, `origins::Rankine` | angle vs temperature intervals (and °C, outside this table); a temperature also needs its origin |

<!-- end: ambiguous -->

`just customary-docs` writes the generated tables of this page from the catalogue; `just test` fails when they are
stale.

## Mass and force

A pound of mass and a pound of force are different dimensions, and so different types:

| Marker       | Dimension | Definition                         |
|--------------|-----------|------------------------------------|
| `PoundAvdp`  | mass      | 0.453 592 37 kg (1959)             |
| `PoundForce` | force     | lb · g_n = 4.448 221 615 260 5 N   |
| `Kip`        | force     | 1000 lbf                           |
| `Poundal`    | force     | lb · ft/s² = 0.138 254 954 376 N   |
| `Slug`       | mass      | lbf · s²/ft = 14.593 902 937.. kg  |

`Fixed<PoundForce, ..>::convert::<PoundAvdp, ..>()` doesn't compile, and a dynamic client refuses the same. A slug is a mass of about 32.174 lb (g_n in ft/s²), not a pound.

## Temperatures

Degrees Fahrenheit and Rankine are points on a scale, not units: `Point<Fahrenheit, Fixed<Kelvin, Rational<5, 9>,
T>>` and `Point<Rankine, ..>` (see [points](points.md)). A temperature difference in °F is a `Fixed<Kelvin,
Rational<5, 9>, T>`.

## Mercury, water, Btu and calories

A pressure in inches or millimetres of mercury or of water depends on the density of the liquid, which depends on its
temperature. The markers say which one they are:

- conventional, a defined density and g_n: `InchOfMercuryConventional` (13 595.1 kg/m³, ISO 80000-4),
  `MillimetreOfMercuryConventional` (133.322 387 415 Pa), `InchOfWaterConventional` (1000 kg/m³); and `Torr`,
  101 325/760 Pa, and the EU's legal `MillimetreOfMercuryEu`, 133.322 Pa;
- at a stated temperature, `InchOfMercury32f`, `InchOfWater39f`: measured.

The same for Btu and calories: the International Table and thermochemical ones are defined, those at a temperature or
"mean" are measured. A measured unit's scale is the value NIST SP 811 prints, exact in this crate by convention only;
`UnitExpr::Named`'s `provenance` is `Provenance::Conventional` for them, `Defined` for every other unit:

<!-- generated: conventional -->

| Marker | Symbol | Value in SI |
|---|---|---|
| `InchOfMercury32f` | inHg (32 °F) | 3.38638 E+03 (NIST SP 811, 2008) |
| `InchOfMercury60f` | inHg (60 °F) | 3.37685 E+03 (NIST SP 811, 2008) |
| `InchOfWater39f` | inH2O (39.2 °F) | 2.49082 E+02 (NIST SP 811, 2008) |
| `InchOfWater60f` | inH2O (60 °F) | 2.4884 E+02 (NIST SP 811, 2008) |
| `FootOfWater39f` | ftH2O (39.2 °F) | 2.98898 E+03 (NIST SP 811, 2008) |
| `BtuMean` | Btu_mean | 1.05587 E+03 (NIST SP 811, 2008) |
| `Btu39f` | Btu_39 | 1.05967 E+03 (NIST SP 811, 2008) |
| `Btu59f` | Btu_59 | 1.05480 E+03 (NIST SP 811, 2008) |
| `Btu60f` | Btu_60 | 1.05468 E+03 (NIST SP 811, 2008) |
| `CalorieMean` | cal_mean | 4.19002 E+00 (NIST SP 811, 2008) |
| `Calorie15c` | cal_15 | 4.18580 E+00 (NIST SP 811, 2008) |
| `Calorie20c` | cal_20 | 4.18190 E+00 (NIST SP 811, 2008) |
| `HorsepowerBoiler` | hp (boiler) | 9.80950 E+03 (NIST SP 811, 2008) |
| `HorsepowerWater` | hp (water) | 7.46043 E+02 (NIST SP 811, 2008) |

<!-- end: conventional -->

## The Mars Climate Orbiter case

In 1999 a ground program wrote thruster impulse in pound-force seconds where the navigation software expected newton
seconds, and the orbiter was lost. With WireWeaver the unit is part of the API, so a client either converts exactly
or refuses; it never reads the number as another unit's.

- **Typed (Rust):** `Fixed::convert` as above; a `Quantity<Times<PoundForce, Second>>` is the same value with its
  prefix sent along.
- **Dynamic client:** `DynQuantity::convert(&DynUnit::from_name("Times<Newton, Second>")?)` gives
  `8896443230521 p(N·s)` for 2 lbf·s (an integer gets the largest SI prefix that keeps it whole, a float stays a
  float); a unit of another dimension is an error ("dimension .. is not ..").
- **Python:** `q.convert("Times<Newton, Second>")`, on a quantity read from the device:

```python
q = api.impulse_q.decode(raw)       # 2 lbf·s
si = q.convert("Times<Newton, Second>")
assert str(si) == "8896443230521 p(N·s)"
q.convert("Times<PoundAvdp, Second>")  # ValueError: dimension .. is not ..
```

`tests/customary` runs the case through a device: a `Fixed`, a `Quantity` and an `AnyQuantity` in lbf·s. A `Fixed`
field shown and converted with its unit in the dynamic client, Python and C is CLT-54.

## Sources

Every definition is from one of these documents, each read in full; a unit's docs say which and where:

<!-- generated: sources -->

- [Refinement of values for the yard and the pound, Federal Register 24 FR 5348 (1 Jul 1959), reprinted in NBS Misc. Publ. 233 (1960)](https://nvlpubs.nist.gov/nistpubs/Legacy/MP/nbsmiscellaneouspub233.pdf)
- [NIST Special Publication 811, Guide for the Use of the International System of Units (SI), Appendix B](https://www.nist.gov/pml/special-publication-811/nist-guide-si-appendix-b-conversion-factors/nist-guide-si-appendix-b8)
- [NIST Handbook 44 (2026), Appendix C, General Tables of Units of Measurement](https://www.nist.gov/document/2026-nist-handbook-44-appendix-c)
- [Weights and Measures Act 1985 (c. 72), Schedule 1](https://www.legislation.gov.uk/ukpga/1985/72/schedule/1)
- [3rd CGPM (1901), Resolution 2](https://www.bipm.org/en/committees/cg/cgpm/3-1901/resolution-2)
- [BIPM, The International System of Units (SI), 8th edition (2006)](https://www.bipm.org/documents/20126/41483022/si_brochure_8.pdf)
- [Federal Register 85 FR 62698 (5 Oct 2020), deprecation of the U.S. survey foot](https://www.govinfo.gov/content/pkg/FR-2020-10-05/pdf/2020-21902.pdf)
- [Council Directive 80/181/EEC of 20 December 1979 on units of measurement](https://www.legislation.gov.uk/eudr/1980/181/annex)
- [21 CFR 101.9 Nutrition labeling of food](https://www.ecfr.gov/current/title-21/section-101.9)

<!-- end: sources -->

A few definitions are stated by no document above. SP 811 prints a matching value for most of them, which the tests
check, but the defining relation is from ISO 80000, DIN or tradition:

<!-- generated: unsourced -->

| Marker | Definition | Why no primary source |
|---|---|---|
| `NauticalMileUkAdmiralty` | 6080 ft | UK Admiralty mile: no primary source fetched |
| `PicaPrinters` | 12 printer's points | pica = 12 points not stated in the fetched texts; SP 811 B.8 prints the matching value |
| `CircularMil` | area of a circle 1 mil across, π/4 mil² | π/4 mil² not stated in the fetched texts; SP 811 B.8 prints the matching value |
| `TonRegister` | 100 ft³ | 100 ft³ (Moorsom) not stated in the fetched texts; SP 811 B.8 prints the matching value |
| `Slug` | lbf·s²/ft | lbf·s²/ft not stated in the fetched texts; SP 811 B.8 prints the matching value |
| `Poundal` | lb·ft/s² | lb·ft/s² not stated in the fetched texts; SP 811 B.8 prints the matching value |
| `InchOfMercuryConventional` | 1 in · 13 595.1 kg/m³ · g_n | conventional mercury density 13 595.1 kg/m³ is ISO 80000-4, not fetched |
| `FootOfMercuryConventional` | 1 ft · 13 595.1 kg/m³ · g_n | conventional mercury density 13 595.1 kg/m³ is ISO 80000-4, not fetched |
| `MillimetreOfMercuryConventional` | 1 mm · 13 595.1 kg/m³ · g_n | conventional mercury density 13 595.1 kg/m³ is ISO 80000-4, not fetched |
| `Torr` | 1/760 atm | 1/760 atm not stated in the fetched texts; SP 811 B.8 prints the matching value |
| `FootPoundal` | ft · pdl | poundal = lb·ft/s² not stated in the fetched texts |
| `HorsepowerMetric` | 75 kgf·m/s | 75 kgf·m/s is DIN 66036, not fetched; SP 811 B.8 prints the matching value |
| `MilWarsawPact` | 1/6000 turn | 1/6000 turn: no primary source fetched |
| `StreckSwedish` | 1/6300 turn | 1/6300 turn: no primary source fetched |

<!-- end: unsourced -->
