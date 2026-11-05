---
level: advanced
---

# Points and origins on the wire

A [`Fixed`](exact_units.md) is an amount: 5 K warmer, 3 ms later, 20° further round. A `Point` is a reading on a
scale that starts somewhere: 25 °C, 1013 hPa absolute, a heading. `exact_units` has one affine algebra for all of
them, `Point<O, Fixed<U, P, T, B>>`: the algebra, the origins the crate ships, declaring your own with `origin!`,
register formulas, conversions and angles that wrap are in the book, starting at
[Points and amounts](https://exact.vhrd.tech/points/points/).

On the wire a point is exactly its number: `DegC` (`Point<Celsius, Fixed<Kelvin, Centi, i16>>`) is an `i16`, the
origin is only in the type.

## On the wire and in the model

`Point` is `transparent`: its bytes are the `Fixed`'s, which are the plain number's. `Offset` is `final_structure`:
the sign bit, then the `Magnitude` behind an option bit. Zero is one byte (`00`), 273.15 twelve, −40 six, 45967/180
fifteen. A negative zero is not canonical and is refused on decode. The cases are `stdlib.si.point.*` and
`stdlib.si.offset.*` in the [conformance corpus](../serdes/conformance.md).

In the API model a point is an instance of `Point` with its origin as the first argument; the origin's definition
(`OriginDef`) goes into the model with INT-13, see [Units in the API model](units_in_the_model.md#origins).
