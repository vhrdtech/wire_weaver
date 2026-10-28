---
level: advanced
---

# Units in the API model

A unit or scale marker an API uses (`Volt`, `prefix::Milli`, a crate's own `Rpm`) carries its definition in the API
model a device sends: its symbol, dimension and exact factor. A client, `ww` and Python learn the units of any crate
from that, without a table of their own, and compare two units by what they are, not by how they are written.

## Declaring units and scales

A crate declares its own markers with `unit!` and `scale!` (from `exact_units`, re-exported by `ww_si`). Each line is
a zero-size struct and its `impl Unit` / `impl FixedScale`; WireWeaver's walker reads the same lines from source, so
the compiler and the model agree:

```rust
ww_si::unit! {
    /// Revolutions per minute: π/30 rad/s
    pub struct Rpm = "rpm", [rad 1, s -1], Exact(1 / 30, pi 1);
}

ww_si::scale! {
    /// One step of a gyro, 70 mdps
    pub struct Mdps70 = "", Exact(70 / 1000);
}

#[ww_api_root]
trait Motor {
    property!(rw speed: Quantity<Rpm>);
    property!(ro gyro: Fixed<Per<Degree, Second>, Mdps70, i16>);
}
```

The dimension is a list of exponents of `s`, `m`, `kg`, `a`, `k`, `mol`, `cd`, `rad`, `bit`, or `counted` for a
counted base of its own (named by the crate and the marker). The factor is exact: `Exact(num / den)`, then optionally
`pow10 e`, `pow2 e`, `pi e` (see [Exact magnitudes](magnitude.md)); left out, the unit is coherent. Only literal values
work, since the walker reads them from source and doesn't evaluate constants. The full grammar is in the docs of
`exact_units::unit!`.

A marker with a hand-written `impl Unit` still compiles, but the walker refuses an API that uses it: a client
wouldn't know the unit.

## What the model holds

A marker struct's definition is `ItemStructExtra::marker`, a `ww_self::MarkerDef`:

| Variant                                                        | For                                                                |
|----------------------------------------------------------------|--------------------------------------------------------------------|
| `Unit(UnitDef { symbol, dim, magnitude })`                     | a named unit: `V`, `rpm`, a counted `bucket`                       |
| `Scale(ScaleDef { symbol, magnitude })`                        | a named scale: `prefix::Milli` (`"m"`, 10⁻³), a crate's `Mdps70`   |
| `Per`, `Times`, `Sqrt`                                         | `exact_units`' unit combinators, their arguments are the instance's |
| `Pow10`, `Pow2`, `Rational`, `FullScale`, `FullScaleMax`       | `exact_units`' generic scales, their const arguments say the value |
| `Unbounded`, `Range`, `RangeU64`, `RangeDec`                   | the bounds of a `Fixed`                                            |
| `Origin(OriginDef { unit, symbol, reference, offset, .. })`    | an origin of a `Point`: `origins::Celsius`, a crate's `origin!`    |
| `Angle`                                                        | `origins::Angle<U>`, its unit argument is the instance's           |

`magnitude` is the exact factor to the coherent SI unit of `dim`: 1 for a volt, π/30 for `rpm` (in rad/s), 8 for a
byte (in bits). Each marker is named by its crate and its path in it (`quantities_api`, `Rpm`), the identity it has
across bundles.

Not there yet:

- `ww_si`'s `DegreeCelsius` as the unit of a `Quantity` (an affine unit, not an exact factor) has no definition; a
  temperature in °C is a [`Point`](points.md) against `origins::Celsius`, and the runtime side follows with STD-74.
- Levels (STD-67), kinds that share a dimension (STD-68) and sentinels (STD-66) append their own variants to
  `MarkerDef` when their markers exist.
- `ww` and Python show a `Quantity` of any crate's unit (`1500 rpm`); a `Fixed` with its unit and scale follows with
  CLT-54.

## The canonical unit

`ww_self::units::unit_of(ty, bundle)` resolves a unit type through these definitions to a `CanonicalUnit`: the
named units it is a product of, sorted by crate and name, each with its power, plus its dimension and exact factor.
Two units are equal when they name the same markers to the same powers, as in F#: `Per<Meter, Second>` and
`Times<Per<Ratio, Second>, Meter>` are both `m·s⁻¹`. Units of the same dimension convert into each other even when
their expressions differ: `factor_to` gives the exact factor (π/30 from `rpm` to `rad/s`), `None` across dimensions.
It is shown with its symbols, `(rad/s)⁻¹·N·m` for `Per<Torque, AngularVelocity>`.

`ww_self::units::scale_of(ty, bundle)` gives the exact factor of a `Fixed`'s scale: a named scale, `Pow10<E>`,
`Pow2<E>`, `Rational<NUM, DEN>`, `FullScale<V, BITS>` (`V / 2^BITS`) or `FullScaleMax<V, BITS>` (`V / (2^BITS - 1)`).

A bundle from a device is untrusted: both walk at most `MAX_UNIT_NESTING` (16) levels of `Per`, `Times`, `Sqrt` and
full-scale markers, and an overflowing dimension or factor is an error, not a panic.

## Origins

The origin of a [`Point<O, F>`](points.md) is a marker as well, its definition a `MarkerDef::Origin(OriginDef)`:

| Field       | Is                                                                                         | °C        |
|-------------|--------------------------------------------------------------------------------------------|-----------|
| `unit`      | the unit of the points' numbers and offsets, written out as a `UnitDef`                     | K         |
| `symbol`    | written after a point's number                                                              | `"°C"`    |
| `reference` | what `offset` is measured from: `None` for the unit's own zero, else a `MarkerRef`          | `None`    |
| `offset`    | where the origin lies from the reference's zero, an exact `Offset` in `unit`                | 273.15    |
| `modulus`   | the period for points that wrap, `None` for points that don't                              | `None`    |
| `step`      | the step the shown number counts, in `unit`                                                 | 1         |
| `shown`     | where the shown number's zero lies: `offset`, or that of the origin it is declared above    | 273.15    |

A definition is flat on purpose: it holds no `Type` of the model, only values and `MarkerRef { crate_id, name }`
(gauge pressure's `exact_units::point::AmbientPressure`), so a pass that renumbers the model's types never has to
look inside one. A crate's own origin is declared with `origin!` and written out by the walker:

```rust
ww_si::origin! {
    /// The board's temperature sensor reads 0 at 25 °C
    pub struct BoardTemp { above: Celsius, offset: Offset::int(25) }
}
```

`BoardTemp` comes out as unit K, symbol `"°C"`, offset 298.15 K and `shown` 273.15 K: the register `raw / 256 + 25 °C`
is shown in °C. `origins::Angle<U>` has no fields of its own: it is a `MarkerDef::Angle`, and its unit argument says
the rest. `ww_self::units::origin_of(ty, bundle)` gives the `OriginDef` of either, an `Angle<Degree>` as unit `°`,
modulus 360; `marker_name_of` names an origin (or any marker) by its crate and name.

As for units, a hand-written `impl Origin` compiles but the walker refuses an API that uses it.

## On the wire

`ItemStruct::extra` is an `Option<RefBox<ItemStructExtra>>`, appended with `#[default = None]`: a plain struct writes
the single 0 bit it wrote before, so its bytes and its [signature](ww_self.md#signatures-and-putting-skipped-definitions-back)
stay as they were, and a client from before reads a newer bundle as one with an appended field it skips. Signatures
leave the marker definition out, as they leave out `ItemEnum::future_version`, so an API using `ww_si::units::Second`
keeps its signature. `MarkerDef` keeps its `Unknown` catch-all, unlike the rest of the model: a reader that doesn't
know a newer kind of marker treats the struct as a marker it can't convert, instead of refusing the bundle.

The definitions of `exact_units`' and `ww_si`'s markers are in the embedded [snapshots](../cli_reference.md#ww-api):
a device leaves them out of its introspection data, and the client puts them back.

The conformance corpus has cases `model.marker_def.*` and `model.item_struct_extra.*` (TEST-23), through the derive,
the dynamic client and Python.

## On the device

No firmware cost in code: markers are zero-size types, and `unit!` / `scale!` expand to the same constants a
hand-written impl has. The introspection data a firmware stores grows only by the definitions of its own crate's
markers (the definition of `Rpm` is 22 bytes, see the case `model.marker_def.unit`, an origin's about 40, see
`model.marker_def.origin`); stdlib markers are left out as skipped definitions.
