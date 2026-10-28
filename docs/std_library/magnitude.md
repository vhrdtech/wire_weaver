---
level: advanced
---

# Exact magnitudes

Every scale in `exact_units` is a `Magnitude`: one canonical, exact factor. A prefix, a power of ten or two, a
fraction, a full-scale step, a degree in radians and the legal definition of the pound all have the same form, so two
scales compare equal however they are written, and a conversion between them is worked out exactly at compile time.

```rust
use exact_units::{FixedScale, Magnitude, prefix::{Milli, Pow10, Rational}};

assert_eq!(Milli::SCALE, Pow10::<-3>::SCALE);
assert_eq!(Milli::SCALE, Rational::<1, 1000>::SCALE);
// the pound, 0.45359237 kg = 7 · 11 · 97 · 6073 · 10⁻⁸
const LB: Magnitude = Magnitude::ratio(45359237, 100_000_000);
assert_eq!(LB.to_ratio(), Some((45359237, 100_000_000)));
```

## The form

A magnitude is a product of powers:

`10^a · 2^b · π^c · ln2^d · ln10^e · ∏ pᵢ^kᵢ`

over primes `pᵢ` other than 2 and 5, every exponent a whole or a half number. It is kept canonical: 2 and 5 appear only
in `a` and `b` (`5 = 10 · 2⁻¹`), the other primes in ascending order with exponents that aren't 0, so equal magnitudes
are equal field by field.

| Spelled                         | Magnitude                          |
|---------------------------------|------------------------------------|
| `Micro`, `Pow10<-6>`            | 10⁻⁶                               |
| `Kibi`, `Pow2<10>`              | 2¹⁰                                |
| `Rational<1, 65535>` (`Duty16`) | 10⁻¹ · 2 · 3⁻¹ · 17⁻¹ · 257⁻¹      |
| a degree in rad                 | 10⁻¹ · 2⁻¹ · 3⁻² · π               |
| a decibel in neper              | 10⁻¹ · 2⁻¹ · ln10                  |
| √2 (a noise density's root)     | 2^½                                |

The form is closed under `mul`, `div`, `powi` and `sqrt`: products never round. Two things are refused, both a
compile error in a `const` (`MagnitudeError` from the `checked_` forms at run time): more than `MAX_FACTORS` (12)
primes other than 2 and 5, and an exponent outside ±63½. The 171 imperial and US customary units of STD-72 take at
most 7 such primes (the slug: 3, 7, 11, 97, 127, 6073, 28019), any product or quotient of two of them at most 10, and
their largest exponent is 21. Fractions of plain integers can't do this: psi² takes 87 bits, (lbf·s)³ 130.

π, ln 2 and ln 10 are exact symbols beside the primes. ln 2 and ln 10 are there for the logarithmic units: a decibel
is ln10/20 Np, a shannon ln 2 nat, a decade ln10/ln2 octaves. The square root of a magnitude halves its exponents
(√(mHz) is 10^(-3/2) Hz^½); the root of a half power (a quarter power) is `MagnitudeError::NotSquare`.

## Markers

`Rational<NUM, DEN>` takes `u64` parameters and is factored into primes at compile time (trial division up to 2¹⁶,
then Miller-Rabin and Pollard's rho), so any fraction a datasheet writes works. A prime factor above `u32::MAX` doesn't
compile; no unit definition has one. Besides the prefixes, `Pow10<E>` and `Rational`:

- `Pow2<E>`: binary steps. The Q formats are aliases over it, with the unit defaulting to `Ratio`: `Q15<U>` is
  `Fixed<U, Pow2<-15>, i16>`, `Q31<U>` is `Fixed<U, Pow2<-31>, i32>`, `UQ16_16<U>` is `Fixed<U, Pow2<-16>, u32>`.
- `FullScale<V, BITS>`: `V / 2^BITS`, one step of a converter whose `2^BITS` codes span `V` (LSB = Vref / 2ᴺ). A
  12-bit ADC at 3.3 V is `Fixed<Volt, FullScale<Rational<33, 10>, 12>, u16>`.
- `FullScaleMax<V, BITS>`: `V / (2^BITS - 1)`, where the largest code is `V` (a PWM duty, a DAC reaching its
  reference). `FullScaleMax<One, 16>` is `Duty16`'s 1/65535.
- Binary angles: `Bam16` is `Fixed<Turn, Pow2<-16>, u16>` (65536 steps a turn), `Bam32` the same over `u32`.

## Conversions at compile time

`Fixed::to::<P2, T2>()` works out `P / P2` as a constant. Between integer storages the factor must be a fraction of
integers, and the multiply must fit a 64-bit integer when both storages have up to 32 bits, else 128 bits. If it
doesn't, the conversion doesn't compile; it is never rounded. At run time the conversion is the multiply and divide
by those constants: nothing between equal scales, a division in the storage's own width when the factor is
1/n, `None` when the result isn't whole or doesn't fit `T2`. Float storages scale by the factor, π included.

```rust
use exact_units::{Fixed, Q15, prefix::{FullScale, Milli, Pow2, Rational}, units::Volt};

let half: Q15 = Fixed::new(16384);
assert_eq!(half.to::<Pow2<-14>, i16>().map(Fixed::get), Some(8192));
type Adc = Fixed<Volt, FullScale<Rational<33, 10>, 12>, u16>;
assert_eq!(Adc::new(4096).to::<Milli, u16>().map(Fixed::get), Some(3300));
assert_eq!(Adc::new(1241).to::<Milli, u16>(), None); // 999.8 mV isn't whole
```

On Cortex-M the conversions cost what the same conversion written by hand costs. Instructions per function
(hand-written / `Fixed::to`, `=` where LLVM emits the same code for both), measured from the ELF on 28 Oct 2026:

| Conversion                           | M0 `z` | M0 `3` | M4F `z` | M4F `3` |
|--------------------------------------|--------|--------|---------|---------|
| ms → µs, `u32` (× 1000, checked)     | 16 / 15 | 17 / 17 | 10 / 13 | 11 / 15 |
| µs → ms, `u32` (exact ÷ 1000)        | 18 / 16 | =      | 12 / 9  | =       |
| `Milli` → `Pow10<-3>`                | =      | =      | =       | =       |
| `Duty16` → `One`, `u8`               | 14 / 15 | =      | =       | =       |
| `Q15` → `Pow2<-14>`, `i16`           | 10 / 12 | =      | 10 / 12 | =       |
| ms `u16` → µs `u32`                  | =      | =      | =       | =       |
| `Rational<70, 1000>` `i16` → m `i32` | =      | =      | =       | =       |

The ms → µs difference is a range check on the input (`x ≤ 4294967`) in place of a 64-bit `umull` and a check of its
high word. The gate that keeps such pairs equal is TEST-29.

`Magnitude::apply_int` and `apply_f64` do the same for a scale only known at run time (a `Quantity`'s prefix):
exact or `None` for integers.

## What a magnitude is not

- An offset: °C against K, gauge against absolute pressure, register formulas like `raw/256 + 25` are points with an
  origin (STD-64).
- A level: dBm, dB SPL and pH take a logarithm of the value, not a factor (STD-67). Only the factor between two
  logarithmic units (dB in Np) is a magnitude.
- A non-linear scale: AWG, Beaufort, mpg against L/100 km are named conversion functions (STD-75).
- A factor known only at run time: a calibration or a currency rate is a value (`ww_si::Quantity`, or the
  application's), not a unit.

## Memory and wire

A `Magnitude` is plain `const` data without lifetimes, 104 bytes, so a firmware can hold a `&'static Magnitude` (a
`Quantity`'s exact prefix, STD-59). `Fixed` only uses it at compile time: a `Fixed` field costs what the plain number
costs.

On the wire (`final_structure`) every exponent is twice the exponent as an `i8`, the primes `UNib32`: `pow10`, then
the rest behind one option bit, its π / ln2 / ln10 behind one more, one bit per empty prime slot. A decimal prefix is
2 bytes (Milli `fa 00`), `Kibi` 5 (`00 80 14 00 00`), a degree 11, 1/65535 13, the pound 16. A magnitude that isn't
canonical (a 2 among the primes, a prime that isn't one, an empty rest written out) is refused on decode. The cases
are `stdlib.si.magnitude.*` in the [conformance corpus](../serdes/conformance.md).
