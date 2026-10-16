---
level: advanced
---

# ww_numeric

`ww_numeric` holds a number of any type in one value, `NumericValue`: the storage of every [`ww_si`](ww_si.md)
quantity, so the API fixes the unit and each device picks the number type (a `u16` of mV, an `f32` of V).
`NumericBaseType` and `NumericAnyType` describe a number type without a value, for capabilities and introspection.

## Number types as features

`NumericValue` holds any number, and is as big as its biggest variant: 32 bytes on a PC and 24 on a Cortex-M with
`u128` in it. Each variant except `U8` is behind a cargo feature of its name, so firmware enables only what it uses:

```toml
# host: std enables every number type
ww_numeric = "0.2"
# firmware: 8 bytes per value, no 64 or 128-bit code
ww_numeric = { version = "0.2", default-features = false, features = ["u16", "f32"] }
```

Features: `nibble`, `u16`, `u32`, `unib32`, `u64`, `i32`, `f32`, `un`, `in`, `u128`, `i8`, `i16`, `i64`, `i128`,
`f64`, `uvlq32`, `uvlq64`, and `all`. `ww_si` has the same ones and passes them on.

The wire format is the same for every set. A value of a type that is not enabled is read as
`Error::EnumVariantDisabled`; see [`#[cfg(..)]` on enum variants](../serdes/derive_reference.md#cfg-on-enum-variants).

## Variable-length numbers

For values that are usually small but have no useful bound (counters, lengths, timestamps in `ww_si::Second`),
three variants grow with the value:

| Variant         | Encoding                                           | Bytes on the wire, with discriminant         |
|-----------------|----------------------------------------------------|----------------------------------------------|
| `UNib32(UNib32)`| nibbles, 3 bits each, right after the discriminant | `5`: `45`; `300`: `4C D4`                    |
| `UVlq32(UVlq32)`| bytes, 7 bits each, big endian                     | `5`: `82 05`; `300`: `82 82 2C`              |
| `UVlq64(UVlq64)`| same, up to 10 bytes                               | `5`: `92 05`; `u64::MAX`: 11 bytes           |

`UVlq32` and `UVlq64` have discriminants 16 and 17, which take two nibbles.

## Conversions

`From<u16>`, `From<f32>`, `From<UVlq32>`, .. for `NumericValue` (each behind its type's feature), so APIs can take
`impl Into<NumericValue>`. `to_f64()` and `as_f32()` give the value as a float, `to_i128()` the exact value of an
integer variant, `unsigned_bits()` the width at which an unsigned counter wraps (`U32` and `UVlq32`: 32).
