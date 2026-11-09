---
level: advanced
---

# Units in the API model

A unit or scale marker an API uses (`Volt`, `prefix::Milli`, a crate's own `Rpm`) carries its definition in the API
model a device sends: its identity, names, dimension and exact factor. A client, `ww` and Python learn the units of any crate
from that, without a table of their own, and compare two units by what they are, not by how they are written.

A crate declares its own markers with `unit!`, `scale!`, `origin!` and `quantity!` (from `exact_units`, re-exported
by `ww_si`); WireWeaver's walker reads the same lines from source, so the compiler and the model agree. For the
macros' grammar — the names, the dimension (halves, `base`, `base(Other)`), the scale, `quantity <Quantity>` and
`conventional(..)` provenance — see the book's
[Your own units](https://exact.vhrd.tech/units/declare/). A marker with a hand-written `impl Unit` still compiles,
but the walker refuses an API that uses it: a client wouldn't know the unit.

```rust
ww_si::unit! {
    /// Revolutions per minute: π/30 rad/s
    pub struct Rpm = "rpm", [rad 1, s -1], Exact(1 / 30, pi 1);
}

#[ww_api_root]
trait Motor {
    property!(rw speed: Quantity<Rpm>);
}
```

## What the model holds

A marker struct's definition is `ItemStructExtra::marker`, a `ww_self::MarkerDef`:

| Variant                                                        | For                                                                |
|----------------------------------------------------------------|--------------------------------------------------------------------|
| `Unit(exact_units::UnitDef)`                                   | a named unit: `V`, `rpm`, `dB`, `Ω`, `inHg` at 32 °F               |
| `Base(exact_units::UnitDef)`                                   | a unit that is a distinguished base of its own: `rad`, `Np`, `bit`, `sample`, a crate's `bucket` |
| `Scale(ScaleDef { id, symbol, magnitude })`                    | a named scale: `prefix::Milli` (`"m"`, 10⁻³), a crate's `Mdps70`   |
| `Per`, `Times`, `Sqrt`                                         | `exact_units`' unit combinators, their arguments are the instance's |
| `Pow10`, `Pow2`, `Rational`, `FullScale`, `FullScaleMax`       | `exact_units`' generic scales, their const arguments say the value |
| `Product`, `Quotient`                                          | the scale of `Fixed * Fixed` and `Fixed / Fixed`, its scale arguments are the instance's |
| `Unbounded`, `Range`, `RangeU64`, `RangeDec`                   | the bounds of a `Fixed`; each refuses NaN                          |
| `MaybeNan`                                                     | a float `Fixed` that may hold NaN, the one bound that takes it     |
| `Origin(OriginDef { id, unit, symbol, reference, offset, .. })` | an origin of a `Point`: `origins::Celsius`, a crate's `origin!`    |
| `Angle`                                                        | `origins::Angle<U>`, its unit argument is the instance's           |
| `Quantity(exact_units::QuantityDef)`                           | a quantity of ISO 80000's tree: `quantities::Height`, `Resistance`, a crate's own `quantity!` |
| `Of`                                                           | `units::Of<Q, U>`, the unit `U` measuring the quantity `Q`; `Phase<A>` is `Of<PhaseAngle, A>` |

A `unit!` line ending in `base` (`pub struct Bucket = "bucket", base;`) declares a unit and the base it is, so its
marker is a `Base`: the unit whole, as for `Unit`, and the base, exact_units' `BaseDef`, is `{ id: def.id }`, its
dimension that base to the 1st power. A scaled unit of another marker's base (`base(Neper)` for the decibel) is a
plain `Unit` whose dimension names that base. The walker's test checks this against `Unit::BASE` for every marker of
`exact_units`.

A unit's definition is `exact_units`' own `UnitDef`, the record `unit!` compiles, field for field:

| Field        | Is                                                                                          | `Rpm`               |
|--------------|---------------------------------------------------------------------------------------------|---------------------|
| `id`         | a `MarkerId`: the declaring crate as Rust names it and the marker's name                     | `my_api::Rpm`       |
| `names`      | how it is written and read (exact_units IO-9): `symbol`, `ascii`, `long` and `plural`, `aliases`, `ucum`, `prefixes` | `"rpm"`, no other names |
| `dim`        | its dimension: seven exponents in halves and the distinguished bases (exact_units UNIT-10)   | rad·s⁻¹             |
| `quantity`   | the quantity it measures, by the quantity marker's `MarkerId` (`exact_units::Resistance` for `Ohm`, UNIT-8), `None` for its dimension's quantity | `None` |
| `scale`      | the exact factor to the coherent SI unit of `dim`: 1 for a volt, 8 for a byte (in bits)      | π/30                |
| `provenance` | `Defined`, or `Conventional { source, year }` for a value a standard prints ([customary](customary.md)) | `Defined` |

A unit has no offset: °C is the origin `origins::Celsius` of kelvins, and a `unit!` line with an `Affine` scale is
an error. Scales and origins carry a `MarkerId` too.

## Identity

A marker is known by its crate and its ident (`my_api`, `Rpm`), not by its module path: it stays the same unit when
it moves between modules, and two crates' `Rpm` are two units that print the same. So a crate has one marker of a
name: the walker refuses an API whose crate declares two markers named `Rpm` in two modules, naming both, and
refuses a model with two different definitions under one identity (two versions of one crate whose `Rpm`s differ).
The crate is the one `CARGO_CRATE_NAME` gives (`my_api` for a package `my-api`), the same `unit!` compiles into its
`UnitDef`, and a test of the walker checks every marker of `exact_units` against its compiled definition.

Not there yet:

- A temperature in °C is a [`Point`](points.md) against `origins::Celsius`; `ww_si`'s `Quantity` and `AnyQuantity`
  follow with STD-74.
- Levels (STD-67) and sentinels (STD-66) append their own variants to `MarkerDef` when their markers exist.
- `ww` and Python show a `Quantity` of any crate's unit (`1500 rpm`), and a `Fixed` with its unit and scale
  ([in clients](exact_units.md#in-ww-python-and-dynamic-clients)).

## The canonical unit

`ww_self::units::unit_of(ty, bundle)` resolves a unit type through these definitions to a `CanonicalUnit`: the
named units it is a product of, sorted by crate and name, each with its power, plus its dimension and exact factor.
Two units are equal when they name the same markers to the same powers, as in F#: `Per<Meter, Second>` and
`Times<Per<Ratio, Second>, Meter>` are both `m·s⁻¹`. Units of the same dimension convert into each other even when
their expressions differ: `factor_to` gives the exact factor (π/30 from `rpm` to `rad/s`), `None` across dimensions.
It is shown with its symbols, `(rad/s)⁻¹·N·m` for `Per<Torque, AngularVelocity>`.

`ww_self::units::scale_of(ty, bundle)` gives the exact factor of a `Fixed`'s scale: a named scale, `Pow10<E>`,
`Pow2<E>`, `Rational<NUM, DEN>`, `FullScale<V, BITS>` (`V / 2^BITS`), `FullScaleMax<V, BITS>` (`V / (2^BITS - 1)`), or
`Product<A, B>` / `Quotient<A, B>` (`A · B`, `A / B`), the scale a product or quotient of two `Fixed` values has:
`Fixed<Watt, Product<Milli, Milli>, i32>` is a step of 10⁻⁶ W, the factor rustc gives `Product<Milli, Milli>::SCALE`.

A bundle from a device is untrusted: both walk at most `MAX_UNIT_NESTING` (16) levels of `Per`, `Times`, `Sqrt`,
full-scale, `Product` and `Quotient` markers, and an overflowing dimension or factor is an error, not a panic.

## Quantities

Some quantities share a dimension and still are not the same: a height and a width are both lengths, Hz and Bq are
both s⁻¹, Ω and V/A are both kg·m²·s⁻³·A⁻². `exact_units` tells them apart with ISO 80000's tree of quantities
(UNIT-8, the book's [Quantities](https://exact.vhrd.tech/maths/quantities/)): a quantity marker declared with
`quantity!`, a unit line that names the quantity it measures (`quantity crate::quantities::Resistance` on `Ohm`'s
line), and `units::Of<Q, U>`, the unit `U` measuring `Q`, as in `Fixed<Of<Height, Meter>, Milli, u16>`.

A quantity marker's definition is `MarkerDef::Quantity(exact_units::QuantityDef)`:

| Field       | Is                                                                                             | `Height`     |
|-------------|------------------------------------------------------------------------------------------------|--------------|
| `id`        | its `MarkerId`, in one namespace with the crate's units                                        | `exact_units::Height` |
| `parent`    | the quantity it is a kind of, by `MarkerId`; `None` for a root                                 | `exact_units::Length` |
| `dim`       | its dimension, its parent's for a child                                                        | m            |
| `is_kind`   | a kind of its own (`Resistance`, `Activity`, `PhaseAngle`): converts with no other quantity of its dimension | `false` |
| `character` | `Scalar`, `Complex` or `Vector`                                                                | `Scalar`     |

`Of<Q, U>` is `MarkerDef::Of`, its two arguments the instance's; `Phase<A>` is an alias of `Of<PhaseAngle, A>`, so
it is no marker of its own. As for origins, a definition names other markers only by `MarkerId`, never by a type of
the model, so a bundle lists every quantity it names: a unit's quantity and each quantity's parent come along when
a device leaves out what the snapshots know and when a client puts it back. The walker refuses a second *open root*
of one dimension (a root without `kind`): each would be the quantity a plain unit of the dimension measures.

`unit_of` gives the quantity in `CanonicalUnit::quantity`, a `QuantityExprOwned`: `Spec` of a quantity by id,
`RatioOf` a dimension (V/V), or `Derived` (its dimension's quantity, which a dimension's open root is too, so
`Of<Length, Meter>` is `Derived`). Two canonical units are the same only with the same quantity, and `factor_to`
takes `exact_units`' conversion rule with the bundle as the table of quantities: a height converts to a length or a
plain metre, not to a width; `Ohm` doesn't convert to `Per<Volt, Ampere>`. `ww`, Python and the dynamic client show
and convert a value of `Of<Q, U>` as one of `U`, as its `UnitExpr` is `U`'s: the quantity is in the type and the
model, not in a value's unit.

## Origins

The origin of a [`Point<O, F>`](points.md) is a marker as well, its definition a `MarkerDef::Origin(OriginDef)`:

| Field       | Is                                                                                         | °C        |
|-------------|--------------------------------------------------------------------------------------------|-----------|
| `id`        | its `MarkerId`                                                                              | `exact_units::Celsius` |
| `unit`      | the unit of the points' numbers and offsets, a named unit's `UnitDef`                       | K         |
| `symbol`    | written after a point's number                                                              | `"°C"`    |
| `reference` | what `offset` is measured from, a `MarkerId`: `exact_units::UnitZero` for the unit's own zero | `UnitZero` |
| `offset`    | where the origin lies from the reference's zero, an exact `Offset` in `unit`                | 273.15    |
| `modulus`   | the period for points that wrap, `None` for points that don't                              | `None`    |
| `step`      | the step the shown number counts, in `unit`                                                 | 1         |
| `shown`     | where the shown number's zero lies: `offset`, or that of the origin it is declared above    | 273.15    |

A definition is flat on purpose: it holds no `Type` of the model, only values and `MarkerId`s (gauge pressure's
`exact_units::AmbientPressure`), so a pass that renumbers the model's types never has to look inside one. For the
same reason an origin's unit is a named unit: the walker refuses an `origin!` over `Per<..>` or `Times<..>`, declare
a unit of that dimension with `unit!` instead. A crate's own origin is declared with `origin!` and written out by the walker:

```rust
ww_si::origin! {
    /// The board's temperature sensor reads 0 at 25 °C
    pub struct BoardTemp { above: Celsius, offset: Offset::int(25) }
}
```

`BoardTemp` comes out as unit K, symbol `"°C"`, offset 298.15 K and `shown` 273.15 K: the register `raw / 256 + 25 °C`
is shown in °C. `origins::Angle<U>` has no fields of its own: it is a `MarkerDef::Angle`, and its unit argument says
the rest. `ww_self::units::origin_of(ty, bundle)` gives the `OriginDef` of either, an `Angle<Degree>` as unit `°`,
modulus 360; `marker_id_of` gives the `MarkerId` of an origin (or any marker).

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
markers (the definition of `my_api::Rpm` is 35 bytes, see the case `model.marker_def.unit`, an origin's about 100,
see `model.marker_def.origin`); stdlib markers are left out as skipped definitions.
