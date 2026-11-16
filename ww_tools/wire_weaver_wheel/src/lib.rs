//! The `wire_weaver._wire_weaver` extension module: [`wire_weaver_py`] as a Python wheel, built by maturin from the
//! `pyproject.toml` next to this crate. Everything the module holds is defined in `wire_weaver_py`.

use pyo3::prelude::*;

#[pymodule]
#[pyo3(name = "_wire_weaver")]
fn wire_weaver_wheel(m: &Bound<'_, PyModule>) -> PyResult<()> {
    wire_weaver_py::init(m)
}
