# power_stage_api

> A power converter's values observed at its switching rate, for the high-rate observe benches

An API-only crate (`no_std` by default, `std` feature for host-side tooling): depends on `ww_api` and `ww_si`
(`electrical`, `i32` units). Declares `PowerStage` (inductor current, output and input voltage) and `PowerStage2`
(current and one voltage), both `ro+observe+batched` properties sampled once per switching period (400 kHz to
2 MHz), for `examples_mcu`'s high-rate observe-cell and column-wise `DeltaPacked` benches.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for how to write an API crate.
