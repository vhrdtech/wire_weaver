# foc_observe_api

> The motor demo's 25 kHz FOC level as an API root, for the observe cell benches of examples_mcu

An API-only crate (`no_std` by default, `std` feature for host-side tooling): depends on `ww_api` and `ww_si`
(`electrical`, `angle`, `dimensionless`, `i32` units). Declares the 21 `ro+observe+batched` properties of the
motor demo's FOC level (phase currents, duty cycles, position, speed, ...), with the same names and types as
`motor_sim`'s `motor_demo_api::Foc`, so `examples_mcu`'s observe-cell benches can sample and poll them from
firmware at 25 kHz without a real motor.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for how to write an API crate.
