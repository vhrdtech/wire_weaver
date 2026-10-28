//! `ww.Quantity`: a value of a `ww_si::Quantity<U>` with its unit (CLT-31), what `decode` / `read` give for one.

use crate::convert::numeric_to_py;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use wire_weaver_client::quantity::{DynQuantity, DynUnit};
use wire_weaver_client::ww_self::units::Bound as Bound_;

/// A value with a unit, as the device sent it: `value` in units of `prefix`, `str()` is `3300 mV`, `float()` the
/// value in the unit itself (3.3), `to_si()` in coherent SI. Read from a `ww_si::Quantity`, an `AnyQuantity` or an
/// `exact_units::Fixed` (CLT-54), whose scale, storage and bounds are fixed by its type: `1730000000000000 µs` exactly
/// from a `u64`, `4 KiB`.
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

    /// The prefix's name: `"Milli"`, `"Kibi"`, `"Unit"` for none, `"Pow10"`, `"Pow2"` or `"FullScale"` for a scale
    /// without a name, its number in [prefix_param](Self::prefix_param); another scale of a `Fixed` by its symbol, or
    /// its factor without one (`"2⁻¹⁵"`).
    #[getter]
    fn prefix(&self) -> String {
        self.q.prefix.name()
    }

    /// The number of a scale without a name: `-4` for `Pow10` 10⁻⁴, `-15` for `Pow2` 2⁻¹⁵, `16` for `FullScale`
    /// 1/65535; `None` for a named prefix.
    #[getter]
    fn prefix_param(&self) -> Option<i8> {
        self.q.prefix.param()
    }

    /// The prefix as a power of ten: `-3` for milli, `-4` for `Pow10` 10⁻⁴; `None` for a scale that is no power of
    /// ten (a binary prefix, `Pow2`, `FullScale`).
    #[getter]
    fn exponent(&self) -> Option<i8> {
        self.q.prefix.magnitude().as_pow10()
    }

    /// The prefix's scale as a float: `0.001` for milli, `1024.0` for kibi, `1/65535` for `FullScale` 16; `nan` for
    /// a scale from a newer version.
    #[getter]
    fn scale(&self) -> f64 {
        self.q.prefix.scale_f64(1.0)
    }

    /// Whether this is an `exact_units::Fixed`: its number is stored in `storage` at the scale `prefix`, and what is
    /// written into it is converted exactly (`5 ms` into µs is 5000) or refused.
    #[getter]
    fn fixed(&self) -> bool {
        self.q.storage.is_some()
    }

    /// The numbers a `Fixed` allows, as stored: `(1, 5000)` for `Range<1, 5000>`, an end `None` when open; `None`
    /// without bounds.
    #[getter]
    fn bounds<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        let Some(storage) = &self.q.storage else {
            return Ok(None);
        };
        let bounds = &storage.bounds;
        if bounds.is_none() {
            return Ok(None);
        }
        let end = |b: Option<Bound_>| -> PyResult<Bound<'py, PyAny>> {
            Ok(match b {
                None => py.None().into_bound(py),
                Some(Bound_::Int(i)) => i.into_pyobject(py)?.into_any(),
                Some(Bound_::Float(x)) => x.into_pyobject(py)?.into_any(),
            })
        };
        Ok(Some(
            (end(bounds.min)?, end(bounds.max)?)
                .into_pyobject(py)?
                .into_any(),
        ))
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
    /// for an angular velocity, `{"bit": 1}` for a byte, counted bases by their name (`{"Sample": 1, "s": -1}`), halves for a root
    /// (V/√Hz: `{"kg": 1, "m": 2, "s": -2.5, "A": -1}`), `{}`
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
            match exp {
                0 => {}
                // a root's odd exponent is a half: V/√Hz has s -2.5
                e if dim.root && e % 2 != 0 => dict.set_item(symbol, e as f64 / 2.0)?,
                e if dim.root => dict.set_item(symbol, e / 2)?,
                e => dict.set_item(symbol, e)?,
            }
        }
        Ok(Some(dict))
    }

    /// The number type the device stores it in: `"U16"`, `"F32"` for a `Quantity`, which may hold any; a `Fixed`'s
    /// as its type names it, `"u64"`.
    #[getter]
    fn storage(&self) -> String {
        match &self.q.storage {
            Some(storage) => storage.name(),
            None => format!("{:?}", self.q.value.ty()),
        }
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

    /// The same quantity in `unit`, a unit of `ww_si` by its type name (`"Times<Newton, Second>"`, `"Meter"`) or
    /// another quantity's unit: exactly, or `ValueError`, never the number reinterpreted in another unit. 2 lbf·s in
    /// `"Times<Newton, Second>"` is `8896443230521 p(N·s)`; a pound of mass doesn't convert to a pound-force, an
    /// integer that no SI prefix holds whole is refused (convert a float for a rounded value).
    fn convert(&self, unit: &Bound<'_, PyAny>) -> PyResult<Quantity> {
        let unit = match unit.cast::<Quantity>() {
            Ok(q) => q.get().q.unit.clone(),
            Err(_) => {
                DynUnit::from_name(&unit.extract::<String>()?).map_err(PyValueError::new_err)?
            }
        };
        let q = self.q.convert(&unit).map_err(PyValueError::new_err)?;
        Ok(Quantity { q })
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
