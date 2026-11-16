# wire_weaver_wheel

The `wire_weaver` Python wheel: a `cdylib` whose one function hands the `wire_weaver._wire_weaver` module to
`wire_weaver_py::init`. The Python package (`python/wire_weaver`), the tests, the console script and `pyproject.toml`
live here; the classes and functions of the module live in `wire_weaver_py`, a library crate.

```sh
uvx maturin build --release        # wheel in target/wheels/
uvx maturin develop --release      # or into the active virtual environment
uv run --group dev pytest          # the tests (`just test-py` from the repository root)
```

Documentation: `docs/python.md`.
