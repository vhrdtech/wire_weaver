//! `wire_weaver`, the extension module of [`wire_weaver_py`] as built from this checkout, for the blinky example's
//! Python script and its test (`blinky.py`, `tests/`). A reader installs the shipped wheel instead
//! (vhrdtech/ww_tools), which wraps the same `init`.

use pyo3::prelude::*;

#[pymodule]
#[pyo3(name = "wire_weaver")]
fn blinky_dyn_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    wire_weaver_py::init(m)
}
