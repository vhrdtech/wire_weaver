# ww_numeric

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
`f64`, and `all`. `ww_si` has the same ones and passes them on.

The wire format is the same for every set. A value of a type that is not enabled is read as
`Error::EnumVariantDisabled`; see [`#[cfg(..)]` on enum variants](../serdes/derive.md#cfg-on-enum-variants).
