# bench_supply

Client of the bench supply example device (`bench_supply_api`), and the device simulated on a PC, over WebSocket
and over the ww hub for the pytest bench (EX-19).

```sh
cargo run -p bench_supply --example bench_supply_sim    # terminal 1: the simulated device
cargo run -p bench_supply --example bench_supply_bench  # terminal 2: set limits, turn an output on, see a refusal
```

`tests/it` runs the same simulated device in process. `bench_supply_hub_device` (a `[[bin]]`) hosts the ww hub
and the device in one process for `examples/bench_supply_py`'s pytest bench, no real hardware needed.
