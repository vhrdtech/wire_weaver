---
level: advanced
---

# Rounding and overflow

`Fixed::to` converts only when the result is exact: a whole number of the target's steps that fits its storage,
else `None`. Where losing a part of a step is the intent (a reading shown in coarser steps, a Q15 sample narrowed to
Q7, a binary angle in fewer bits), the conversion names how it rounds and what it does when the result doesn't fit.
`to()` stays the default: a lossy conversion is always written as one.

```rust
use exact_units::{Fixed, Rounding, prefix::{Deci, Milli}, units::Volt};

let v: Fixed<Volt, Milli, i16> = Fixed::new(1250);
assert_eq!(v.to::<Deci, i16>(), None); // 12.5 dV isn't whole
assert_eq!(v.to_rounded::<Deci, i16>(Rounding::NearestEven).unwrap().get(), 12);
assert_eq!(v.to_rounded::<Deci, i16>(Rounding::NearestAway).unwrap().get(), 13);
```

## Rounding

| `Rounding`    | 2.5 | -2.5 | 2.4 | What does it                                       |
|---------------|-----|------|-----|----------------------------------------------------|
| `Truncate`    | 2   | -2   | 2   | integer division in Rust and C                     |
| `Floor`       | 2   | -3   | 2   | an arithmetic shift right                          |
| `Ceil`        | 3   | -2   | 3   |                                                    |
| `NearestEven` | 2   | -2   | 2   | IEEE 754's default, Python's `round`, no bias      |
| `NearestAway` | 3   | -3   | 2   | C's `round` and `lround`                           |

Every mode is exact: the conversion works out the true quotient and rounds once, with no float in between for
integer storages. The factor between the scales is a constant worked out at compile time, as for `to`. Unlike `to`,
a factor that isn't a fraction of integers (π between degrees and radians, ln 2, ln 10, a half exponent) is allowed:
it is multiplied as its 64-bit fixed point, within 2⁻⁶⁴ of the factor, so the result is the correctly rounded one
except where the true value lies within `|x| · 2⁻⁶⁴` of a rounding step. With a float on either side the float
result is rounded.

## Overflow

What a result that doesn't fit the target storage becomes:

| `Overflow` | 200 dV into an `i8` | For                                                        |
|------------|---------------------|------------------------------------------------------------|
| `Checked`  | `None`              | the default, where neither of the others is the meaning    |
| `Saturate` | 127                 | Q formats, levels, set points: the nearest end             |
| `Wrap`     | -56                 | binary angles, free-running counters: modulo 2^bits        |

```rust
use exact_units::{Fixed, Overflow, Rounding, prefix::{Deci, Milli}, units::Volt};

let big: Fixed<Volt, Milli, i16> = Fixed::new(20000);
assert_eq!(big.to_rounded::<Deci, i8>(Rounding::NearestEven), None);
assert_eq!(big.to_saturating::<Deci, i8>(Rounding::NearestEven).get(), 127);
assert_eq!(big.to_wrapping::<Deci, i8>(Rounding::NearestEven).get(), -56);
// both chosen at run time
let r = big.requantize::<Deci, i8>(Rounding::Floor, Overflow::Saturate);
assert_eq!(r.map(Fixed::get), Some(127));
```

`Wrap` is two's complement for a signed storage. From a float, NaN and infinities wrap to 0; an infinity saturates
to its end and NaN to 0.

## Proven by the bounds

When the bounds of a value prove that every rounded result fits, the conversion can't fail and says so in its type:
`into_rounded` returns the value, not an `Option`, and doesn't compile where the proof doesn't hold. Without bounds
the whole range of the storage is what has to fit. A whole factor between integer storages (mV to µV) is a plain
`From` under the same proof:

```rust
use exact_units::{Fixed, Range, Rounding, prefix::{Deci, Micro, Milli}, units::Volt};

type Rail = Fixed<Volt, Milli, i16, Range<0, 3300>>;
let v = Rail::bounded(1250);
let dv: Fixed<Volt, Deci, u8> = v.into_rounded(Rounding::NearestEven); // 0 ..= 33 dV fits a u8
let uv: Fixed<Volt, Micro, i32> = v.into(); // 3 300 000 µV fits an i32
assert_eq!((dv.get(), uv.get()), (12, 1_250_000));
```

A `Fixed<Volt, Milli, i16>` without the range reaches 3276.7 V, more than a `u8` of 0.1 V steps holds: there,
`into_rounded` is a compile error and `to_rounded` the way.

## The policy of a type

Arithmetic (`+`, `-`, `*` and `/` by the storage) has an overflow policy too, and that one is part of the type, in
the bounds parameter `B`:

| `B`          | `+` past the end of the storage           | Aliases with it                 |
|--------------|-------------------------------------------|---------------------------------|
| `Unbounded`  | the storage's own `+`, `checked_add` etc. | `Micros`, `Millis`, `Duty16`... |
| `Wrapping`   | modulo 2^bits                             | `Bam16`, `Bam32`                |
| `Saturating` | the nearest end                           | `Q15`, `Q31`, `UQ16_16`         |

```rust
use exact_units::{Bam16, Fixed, Q15, Wrapping, prefix::Pow2, units::{Turn, Volt}};

// 3/4 turn + 1/2 turn is 1/4 turn
assert_eq!((Bam16::of(49152) + Bam16::of(32768)).get(), 16384);
// Q15 stops at 1 - 2⁻¹⁵
assert_eq!((Q15::<Volt>::of(30000) + Q15::of(10000)).get(), 32767);
// the same bytes, another policy: free
let c: Fixed<Turn, Pow2<-16>, u16, Wrapping> = Fixed::<Turn, Pow2<-16>, u16>::new(65535).with_overflow();
assert_eq!((c + Fixed::of(2)).get(), 1);
```

`Wrapping` and `Saturating` values are made with `Fixed::of` (a `const fn`), `new` stays `Unbounded`'s so that
`Fixed::new(a) + Fixed::new(b)` keeps inferring its type. Only the policy is in the marker: the bytes are the
storage's, so the same value goes on the wire whatever the policy.

## At run time

A dynamic client, `ww` or a test bench has the scales and storages from the API model, not as types.
`requantize` (or a `Requantizer` for a series of values) is the same conversion with the factor a `Magnitude` value
and the target a `StorageKind`, and gives the same number as the typed one, bit for bit:

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
rational and π scales in every mode and policy, run through the typed conversion and the run-time one; Python and C
follow with CLT-54.

## Cost on a device

A conversion is the constants worked out at compile time and, at run time, one multiply, one divide (or shift) and a
compare for the rounding: a factor of 1/n divides in the storage's own width, a factor with π multiplies by a 64-bit
constant. Nothing of the policy is decided at run time unless `requantize` with a run-time policy is used.
