## Unreleased

### ⚠️ Breaking

* `Load::Viscous { b }` and `Load::Fan { k }` are `ww_si` quantities now (STD-38): `b: Quantity<Per<Nm, RadPerS>>` (N·m/(rad/s)) and `k: Quantity<Per<Nm, Times<RadPerS, RadPerS>>>` (N·m/(rad/s)²) instead of `f32`. The wire changes (a prefix and a number instead of 4 bytes), hosts and devices are rebuilt together. Build one with `Quantity::from_f32(1.2e-4)`, read it with `to_f32()`.
