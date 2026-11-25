# thermometer_api

API of the thermometer example device: a sensor board with up to four temperature inputs, a sampling rate, a mode,
an observed gain, a stream of readings and a sink of commands. Every number carries its unit and bounds in its type.

It is wire line 1 at w1.0, the same wire as line 1 of `tests/thermo_api`, which tells this device's later life.
`tests/same_as_thermo_bench.rs` checks that the two stay equal.
