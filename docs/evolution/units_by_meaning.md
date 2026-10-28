---
level: advanced
---

# Units compared by meaning

A `Fixed<U, P, T, B>` or a `Point<O, Fixed<..>>` is its number on the wire: the unit `U`, the scale `P`, the bound
`B` and the origin `O` are in the type only. Changing one of them keeps the bytes, so nothing on the wire tells an old
reader that the number means something else now. The evolution checker (`ww api check`, the client at connect time)
does: since the API model carries the definition of every unit, scale and origin an API uses
([units in the model](../std_library/units_in_the_model.md), INT-13), it compares them by what they mean, not by
their names (EVO-17). The [evolution rules](rules.md#transparent-wrappers-units-and-scales) have the short version.

## One step of the number

The unit and the scale of a `Fixed` together say what one step of its number is: a dimension and an exact factor in
coherent SI. The checker multiplies them out on both sides and compares the result:

* **The same step, spelled differently**: compatible, listed as `same value`.
    * `Fixed<Volt, Milli, i32>` -> `Fixed<Volt, Pow10<-3>, i32>` (a prefix and the power of ten it is),
    * `Fixed<Volt, Milli, i32>` -> `Fixed<Per<Joule, Coulomb>, Pow10<-3>, i32>` (a named unit and its product),
    * `Fixed<Per<Meter, Second>, Milli, i32>` -> `Fixed<MeterPerSecond, Milli, i32>`,
    * `Fixed<Volt, Milli, i32>` -> `Fixed<MilliVolt, One, i32>` (a unit of your own with the prefix in it: unit and
      scale are compared together).
* **The same dimension, another factor**: breaking, under `Rule::Meaning`. The finding gives the factor, exactly and
  rounded: `Fixed<Per<Meter, Second>, One, i32>` -> `Fixed<KmPerHour, One, i32>` is `the same bytes in another scale,
  1 on the old version is 3.6 (exactly 1·18/5) on the new version, which would read it as 1`.
* **Another dimension** (`Fixed<Joule, ..>` -> `Fixed<Newton, ..>`): breaking, `another dimension`.

The Mars Climate Orbiter was lost to the second case: the ground software wrote impulse in lbf·s, the spacecraft read
it as N·s. The same numbers, each 4.45 times too small on the reader. As an API:

```rust
// old version
fn set_impulse(impulse: Fixed<Times<PoundForce, Second>, One, i32>);
// new version
fn set_impulse(impulse: Fixed<Times<Newton, Second>, One, i32>);
```

The checker reports it in both directions, depending on who writes:

```text
Api::set_impulse: argument `impulse`: U of Fixed: marker units::Times<PoundForce, units::Second> on the old version,
units::Times<units::Newton, units::Second> on the new version: the same bytes in another scale, 1 on the old version
is 4.448222 (exactly 1·…) on the new version, which would read it as 1
```

and a client in N·s refuses that resource of a device in lbf·s at connect time instead of reading its numbers as
they are.

## Units and scales of your own

A unit or scale your API crate declares itself (`exact_units::unit!`, `exact_units::scale!`) is compared by its
definition too, also when only the definition changed and every use kept its name: `PoundForce` defined as 1 N by
mistake and fixed to 4.448222 N is `unit PoundForce redefined: the same bytes in another scale`, both for the
marker itself and for every `Fixed` that uses it. Its symbol (`"lbf"`) is how it is shown, not what it means: a new
symbol alone is compatible. A marker that became another kind of marker under the same name (a unit turned scale) is
breaking.

## Origins

An origin is where a point's zero lies (`Celsius`: 273.15 K; a register formula of your own). Two origins are the
same when they have the same dimension, the same reference, the same zero and the same period, in coherent SI.
Otherwise the reader would take every point for another position:

* **Another name for the same zero**: compatible. `Point<Celsius, ..>` -> `Point<OwnCelsius, ..>` with `OwnCelsius`
  your own origin at 273.15 K, shown as `°C` or not.
* **Another zero**: breaking. `Point<Celsius, ..>` -> `Point<BoardTemp, ..>` with `BoardTemp` at 298.15 K is
  `another zero (273.15 K vs 298.15 K)`; so is `Celsius` -> `AbsoluteZero`.
* **Another reference**: breaking. `Point<Vacuum, Fixed<Pascal, ..>>` -> `Point<Gauge, ..>` is `another reference
  (the unit's zero vs exact_units::AmbientPressure)`: a gauge pressure is against a pressure only known at run time.
* **Another period**: breaking. An [angle that wraps](../std_library/points.md#angles-that-wrap) every turn and a
  position that doesn't are different things. `Angle<Degree>` and `Angle<Radian>` are the same origin (zero, one turn),
  so only the `Fixed`'s own unit counts there.
* **Another dimension** (`Point<Angle<Degree>, ..>` -> `Point<Celsius, ..>`): breaking.

How a point is shown (the origin's symbol, its shown unit) is not compared. The step of a point is its `Fixed`'s, and
is compared as above: `Point<Celsius, Fixed<Kelvin, Centi, i16>>` -> `Point<Celsius, Fixed<Kelvin, Milli, i16>>` is
another scale.

## Bounds, wrapping and saturating

A [bound](rules.md#bounds) (`Range`, `RangeU64`, `RangeDec`) is compared by value, and any change of it is breaking
for the side that refuses. In the place of a bound, `Wrapping` and `Saturating`
([rounding and overflow](../std_library/rounding.md#the-policy-of-a-type)) say what happens at the ends of the number:

* **`Saturating`**: the same values with the same meaning as `Unbounded`: compatible both ways.
* **`Wrapping`**: a position modulo its storage (a binary angle, a free-running counter). On one side only (`Unbounded`
  -> `Wrapping` or back) it is breaking, although the bytes are the same: the reader of a wrapping number takes
  `0` after `65535` for the next step, a reader of a plain one for a jump back.
* **A plain number -> `Fixed<.., Wrapping>` or `Fixed<.., Saturating>`**: compatible, as any plain number to a
  `Fixed` of it.
* **`Range<..>` -> `Saturating`**: a bound widened, breaking as such.

## Markers compared by name

A marker without a definition in the model (a unit struct of your own that is no `unit!`, `scale!` or `origin!`, or
a model saved before INT-13) is still compared by name, with its own arguments. Levels (STD-67), kinds (STD-68) and
sentinels (STD-66) come with rules of their own once they are built; until then a marker of a kind the checker
doesn't know is the same only when its definition is exactly the same, so the checker errs on the breaking side:

* **Levels** (`Level<Kind, Ref>`, dBm, dBV, dBFS): a level is a point against its reference, so another reference
  (1 mW vs 1 W, full scale of a sine vs a square) is breaking as another zero is; the same reference under another
  name is compatible.
* **Kinds that share a dimension** (relative humidity vs duty cycle, Hz vs Bq): another kind is breaking, although
  the dimension and the factor are the same; a kind and its parent only through an explicit cast.
* **Sentinels** (niche codes in the raw value): a new code inside the old values, a removed one, or a code with
  another meaning is breaking, since the reader takes the code for a value or the value for a code.

## In code

The checks live in `wire_weaver_evolution::compat::TypeCheck` and are the same for `ww api check`, `Check` in an
API crate's own test, and `wire_weaver_client` at connect time. A compatible change with the same meaning is listed
in `Report::compatible`, ending in `same value` (for a unit, scale or origin) or `same bytes` (a transparent wrapper);
`compat::Unwrapped::same` holds which. The tests in `wire_weaver_evolution/src/tests/by_meaning.rs` run each case above.
