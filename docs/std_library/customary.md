---
level: basic
---

# Imperial and US customary units in WireWeaver

[`exact_units`](https://exact.vhrd.tech/) has a marker for every imperial, US customary and other non-SI unit
still in use (feature `customary`, opt-in; `ww_si`'s `customary` turns it on): inches, pounds, gallons, psi, Btu,
horsepower, slugs, knots, mils, each an exact rational of SI from its legal definition, with the source cited. For
the full catalogue, the ambiguous names no marker uses bare (gallon, ton, ounce, ...), mass vs force, temperatures
as points and the measured units (mercury, water, Btu, calories), see the book's
[Imperial and US customary units](https://exact.vhrd.tech/customary/).

## The Mars Climate Orbiter case

In 1999 a ground program wrote thruster impulse in pound-force seconds where the navigation software expected newton
seconds, and the orbiter was lost. With WireWeaver the unit is part of the API, so a client either converts exactly
or refuses; it never reads the number as another unit's.

- **Typed (Rust):** `Fixed::convert`, as in the book's [Mars Climate Orbiter
  case](https://exact.vhrd.tech/customary/#the-mars-climate-orbiter-case); a `Quantity<Times<PoundForce, Second>>`
  is the same value with its prefix sent along.
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
