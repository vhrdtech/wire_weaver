//! The `wire_weaver._wire_weaver` extension module: [`wire_weaver_py`] as a Python wheel, built by maturin from the
//! `pyproject.toml` next to this crate. Everything the module holds is defined in `wire_weaver_py`, and its submodule
//! `logring` in logring's `logring_py` (`wire_weaver.logring`).

use pyo3::prelude::*;

/// The API snapshots of every crate in the build's lock (ww_stdlib, ww_global, ww_tap, exact_units): a device built
/// with them is understood with nothing on disk and no network (EVO-80).
static STDLIB: ww_snapshots::Set = include!(concat!(env!("OUT_DIR"), "/stdlib_set.rs"));

#[pymodule]
#[pyo3(name = "_wire_weaver")]
fn wire_weaver_wheel(m: &Bound<'_, PyModule>) -> PyResult<()> {
    ww_snapshots::embed(&STDLIB);
    wire_weaver_py::init(m)?;
    let logring = PyModule::new(m.py(), "logring")?;
    logring_py::init(&logring)?;
    m.add_submodule(&logring)
}
