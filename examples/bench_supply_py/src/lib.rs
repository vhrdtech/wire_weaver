//! `wire_weaver`, the extension module of [`wire_weaver_py`] as built from this checkout, for the bench supply
//! example's pytest bench (EX-19, `tests/test_bench_supply.py`). The shipped wheel (vhrdtech/ww_tools) adds a
//! Python package and logring's submodule around the same `init`.

use pyo3::prelude::*;

#[pymodule]
#[pyo3(name = "wire_weaver")]
fn bench_supply_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    wire_weaver_py::init(m)
}
