//! `ww.Quantity`: a value of a `ww_si::Quantity<U>` with its unit (CLT-31), what `decode` / `read` give for one.

use crate::convert::numeric_to_py;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use wire_weaver_client::quantity::DynQuantity;

/// A value with a unit, as the device sent it: `value` in units of `prefix`, `str()` is `3300 mV`, `float()` the
/// value in the unit itself (3.3), `to_si()` in coherent SI.
#[pyclass(module = "wire_weaver", name = "Quantity", frozen, eq)]
#[derive(PartialEq)]
pub(crate) struct Quantity {
    pub(crate) q: DynQuantity,
}

#[pymethods]
impl Quantity {
    /// The number as stored, in units of `prefix`: `3300` for 3300 mV.
    #[getter]
    fn value<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        numeric_to_py(py, &self.q.value)
    }

    /// The SI prefix's name: `"Milli"`, `"Unit"` for none.
    #[getter]
    fn prefix(&self) -> &'static str {
        self.q.prefix.name()
    }

    /// The prefix as a power of ten: `-3` for milli.
    #[getter]
    fn exponent(&self) -> i8 {
        self.q.prefix.into()
    }

    /// The unit's symbol, without the prefix: `"V"`, `"N·m/(rad/s)"`, empty for a ratio; a unit `ww_si` doesn't know
    /// has its name here.
    #[getter]
    fn symbol(&self) -> String {
        self.q.unit.symbol()
    }

    /// The unit's type name: `"Volt"`, `"Per<Torque, AngularVelocity>"`, `"Rpm"`.
    #[getter]
    fn unit(&self) -> String {
        self.q.unit.name()
    }

    /// The dimension as base-unit exponents, `{"kg": 1, "m": 2, "s": -3, "A": -1}` for a volt, `{"rad": 1, "s": -1}`
    /// for an angular velocity, `{"bit": 1}` for a byte, counted bases by their name (`{"Sample": 1, "s": -1}`), `{}`
    /// when dimensionless, `None` when the unit is a marker known by its name only (not one of `ww_si`, nor a unit a
    /// device sent with its dimension in an `AnyQuantity`).
    #[getter]
    fn dim<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyDict>>> {
        let Some(dim) = self.q.unit.dim() else {
            return Ok(None);
        };
        let dict = PyDict::new(py);
        let [s, m, kg, a, k, mol, cd, rad, bit] = dim.exps();
        let fixed = [
            ("kg", kg),
            ("m", m),
            ("s", s),
            ("A", a),
            ("K", k),
            ("mol", mol),
            ("cd", cd),
            ("rad", rad),
            ("bit", bit),
        ];
        for (symbol, exp) in fixed.into_iter().chain(dim.counted_bases()) {
            if exp != 0 {
                dict.set_item(symbol, exp)?;
            }
        }
        Ok(Some(dict))
    }

    /// The number type the device stores it in: `"U16"`, `"F32"`.
    #[getter]
    fn storage(&self) -> String {
        format!("{:?}", self.q.value.ty())
    }

    /// Doc lines of a unit `ww_si` doesn't know, empty otherwise.
    #[getter]
    fn docs(&self) -> Vec<String> {
        self.q.unit.docs().to_vec()
    }

    /// The value in coherent SI through the unit's scale: 3300 mV is 3.3, 90 ° is 1.5708 (rad), 20 °C is 293.15 (K).
    /// `ValueError` for a unit `ww_si` doesn't know.
    fn to_si(&self) -> PyResult<f64> {
        self.q.to_si_f64().ok_or_else(|| {
            PyValueError::new_err(format!(
                "unit {} is not known to ww_si and has no scale to SI",
                self.q.unit.name()
            ))
        })
    }

    /// The value in the unit itself, without the prefix: 3.3 for 3300 mV.
    fn __float__(&self) -> f64 {
        self.q.to_f64()
    }

    fn __str__(&self) -> String {
        self.q.to_string()
    }

    fn __repr__(&self) -> String {
        format!("Quantity({} {})", self.q, self.storage())
    }
}
