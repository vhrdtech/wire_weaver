# thermometer_py

Python scripts of the thermometer example (`properties.py`, `explore.py`, `values.py`, `streams.py`, `errors.py`), run against the simulated device (`thermometer_sim`):

```sh
cargo run -p thermometer --example thermometer_sim -- --still   # terminal 1
uv run python properties.py                                     # terminal 2, in this directory
```

The crate builds `wire_weaver_py` of this checkout as the `wire_weaver` module; outside this repository, install the `wire_weaver` wheel from vhrdtech/ww_tools instead. `just test-thermometer-py` runs the scripts against a simulated device and checks their output against `docs/generated/thermometer/`.
