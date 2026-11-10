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

## In `ww`, Python and dynamic clients

A client that only has a device's introspection data reads a `Point` as a `DynQuantity` (`ww.Quantity` in Python,
CLT-65): its own fields (`unit`, `prefix`, `value`, `storage`) stay the inner `Fixed`'s raw reading, the same as for
a plain `Fixed`'s ([exact_units](exact_units.md#in-ww-python-and-dynamic-clients)), but shown and converted through
the origin instead of the `Fixed`'s own unit: a board's temperature register that reads 0 at 25 °C
(`origin! { above: Celsius, offset: Offset::int(25) }`) shows `25.5 °C`, not `2500 cK`, through `ww observe`, `ww
explain` and Python's `str()`/`float()`, the same text and number as the typed `Point`'s own:

```python
t = dev.board_temp.read()       # 25.5 °C
t.origin, t.symbol, t.unit      # "BoardTemp", "°C", "Kelvin"
float(t)                        # 25.5
t.to_si()                       # 298.65, in Kelvin: the Fixed's own unit, from the reference's zero
```

`origin` names the origin (`O`'s marker, `"Celsius"`, `"BoardTemp"`, `"Angle"` for a heading that wraps every 360°);
`unit` and `symbol` stay the `Fixed`'s plain-unit meaning everywhere else, so a point's `symbol` is the origin's own,
not the unit's. The shown number is exact, the offset and step as a ratio first, not cancelled in plain `f64`
(`write!(f, "{:.1}", ..)` honors the precision too, a tie rounding to even as the typed `Point` does); an origin or
scale from a newer version falls back to the `Fixed`'s own honest reading instead of a number under the wrong
symbol.

`dev.p.write(dev.p.read())` writes a point back exactly: `to_value` accepts a `Quantity` of the *same* origin (the
offset cancels, only the coord's own scale and storage convert). A *different* origin needs the affine between
the two (CLT-69, not yet); a script reaches `coord` (the `Fixed` it wraps) directly for that, as before CLT-65.
