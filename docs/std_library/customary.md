---
level: basic
---

# Imperial and US customary units

`exact_units` has a marker for every imperial, US customary and other non-SI unit still in use (feature `customary`,
opt-in: not part of `quantities` or `std`, so an API without them pays nothing for them; `ww_si`'s `customary` turns it
on): inches, pounds, gallons, psi, Btu, horsepower, slugs, knots, mils. Each one's scale
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

## The catalogue

The catalogue of markers lives with `exact_units` in [its own repository](https://github.com/romixlab/exact_units/blob/main/docs/customary.md): every unit with its definition and
source, the names that are never a bare marker ("gallon", "ton", "ounce", "BTU", "horsepower": `GallonUs` and
`GallonImperial`, `TonShort`, `TonLong` and `Tonne`, ...), mass vs force (`PoundAvdp` vs `PoundForce`, `Slug`),
temperatures as [points](points.md) (`Point<Fahrenheit, ..>`), and the units of mercury, water, Btu and calories that
are measured rather than defined.

A measured unit's scale is the value NIST SP 811 prints, exact by convention only: its `provenance` is
`Provenance::Conventional`. An API model carries it too, as `ww_self::MarkerDef::ConventionalUnit` (a unit with its
source, see [units in the model](units_in_the_model.md)), while a defined unit stays a plain `MarkerDef::Unit` and
pays nothing for it, so a client knows which scales are conventions. The same holds for an API crate's own `unit!`
lines with `conventional(..)`.

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

`tests/customary` runs the case through a device: a `Fixed`, a `Quantity` and an `AnyQuantity` in lbf·s. The dynamic
client reads the `Fixed` field with its unit (`2000 m(lbf·s)`, `{:#}` `2 lbf·s`), converts it to N·s exactly, and
refuses 4 N·s written into it (not a whole number of m(lbf·s)), as for the others
([in clients](exact_units.md#in-ww-python-and-dynamic-clients)).

- **Evolution checker:** an API that changes `Fixed<Times<PoundForce, Second>, ..>` to `Fixed<Times<Newton, Second>,
  ..>` keeps the bytes, and the checker calls it breaking with the factor (`1 on the old version is 4.448222`), also
  at connect time; see [units compared by meaning](../evolution/units_by_meaning.md) (EVO-17).
