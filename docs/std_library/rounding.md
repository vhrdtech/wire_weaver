---
level: advanced
---

# Rounding and overflow at run time

`Fixed::to` converts only when the result is exact. Where losing part of a step is the intent, the conversion
names how it rounds (`Rounding`: `Truncate`, `Floor`, `Ceil`, `NearestEven`, `NearestAway`, `NearestCeil`) and what it does when
the result doesn't fit (`Overflow`: `Checked`, `Saturate`, `Wrap`), or carries the overflow policy in its bounds
type `B` (`Unbounded`, `Saturating`, `Wrapping`). See the book's
[Rounding](https://exact.vhrd.tech/maths/rounding/) and [Overflow](https://exact.vhrd.tech/maths/overflow/), and
[Cost on Cortex-M](https://exact.vhrd.tech/fixed_point/cost/) for the instruction counts.

## At run time

A dynamic client, `ww` or a test bench has the scales and storages from the API model, not as types. `requantize`
(or a `Requantizer` for a series of values) is the same conversion with the factor a `Magnitude` value and the
target a `StorageKind`, and gives the same number as the typed one, bit for bit:

```rust
use exact_units::{Magnitude, Overflow, Repr, Requantizer, Rounding, StorageKind};

// mV to dV in an i8: the factor is 10⁻²
let q = Requantizer::new(
    &Magnitude::pow10(-2),
    StorageKind::Int { bits: 8, signed: true },
    Rounding::NearestEven,
    Overflow::Checked,
);
assert_eq!(q.apply(Repr::Int(1250)), Some(Repr::Int(12)));
```

The [conformance corpus](../serdes/conformance.md#a-requantize-case) holds conversions over decimal, binary,
rational and π scales in every mode and policy, run through the typed conversion, the run-time one, Python's
`ww.requantize` and C's `sw_requantize()`.

The same conversion outside Rust, bit for bit:

- Python: `ww.requantize(value, factor, to, rounding, overflow="checked")`, or `ww.Requantizer(factor, to, rounding,
  overflow)` with `.apply(value)` for many values; the factor is a `Magnitude` as the model gives it, its bytes or
  the `dict` `decode` gives (`{"pow10": -4, "rest": None}` is 1/100).
- C: `sw_requantize()` in `shrink_wrap/c/sw_requantize.h` and `.c` (C99, no `malloc`), with the factor as
  constants worked out on the host: an exact ratio `num / den` of up to 128 bits, or `m * 2^shift` for π and other
  factors that aren't fractions of integers. See [ShrinkWrap in C](../c.md#requantize).
