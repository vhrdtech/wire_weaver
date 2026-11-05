---
level: advanced
---

# Exact magnitudes on the wire

Every scale in [`exact_units`](https://exact.vhrd.tech/) is a `Magnitude`: one canonical, exact factor, a product of
powers of 10, 2, π, ln 2 and ln 10 and other primes. Two scales compare equal however they are written, and a
conversion between them is worked out exactly at compile time. For the form, what it is and isn't, and how a
conversion is chosen at compile time, see the book's [Exact magnitudes](https://exact.vhrd.tech/nomicon/magnitude/)
and [Conversions at compile time](https://exact.vhrd.tech/nomicon/compile_time/).

## On the wire

A `Magnitude` is plain `const` data without lifetimes, 104 bytes, so a firmware can hold a `&'static Magnitude`.
`Fixed` only uses it at compile time: a `Fixed` field costs what the plain number costs. `ww_si::Quantity`'s
run-time `Prefix` computes its magnitude when asked (`Prefix::magnitude()`), never storing one.

On the wire (`self_describing`: the bytes end by themselves, so a magnitude can be the field of an enum variant,
`SIExpr`'s `Scale::Exact`) every exponent is twice the exponent as an `i8`, the primes `UNib32`: `pow10`, then
the rest behind one option bit, its π / ln2 / ln10 behind one more, one bit per empty prime slot. A decimal prefix is
2 bytes (Milli `fa 00`), `Kibi` 5 (`00 80 14 00 00`), a degree 11, 1/65535 13, the pound 16. A magnitude that isn't
canonical (a 2 among the primes, a prime that isn't one, an empty rest written out) is refused on decode. The cases
are `stdlib.si.magnitude.*` in the [conformance corpus](../serdes/conformance.md).
