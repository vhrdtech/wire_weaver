# blinky_dyn_py

> Python scripts of the blinky example, with wire_weaver_py of this checkout as the `wire_weaver` module

`blinky.py` calls the blinky device from Python through the generic `wire_weaver` module, which builds the API from
the device's introspection data: no generated code. Run the simulated device first
(`cargo run -p blinky --example blinky_sim`), then `uv run blinky.py`. `just test-blinky-py` checks that the
script prints what the book shows (`docs/generated/blinky/blinky_py.txt`).

`examples/blinky_py` is the other way to reach the device from Python: a typed module wrapping the generated Rust
client.
