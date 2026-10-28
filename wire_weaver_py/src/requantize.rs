//! `ww.requantize` and `ww.Requantizer`: a stored number converted to another scale and storage with a named
//! rounding and overflow policy (STD-63), `exact_units::Requantizer` underneath, so the same number as the typed
//! `Fixed::requantize` and the dynamic client, bit for bit (CLT-54).

use exact_units::{
    Irrational, Magnitude, Overflow, PrimePower, Repr, Requantizer as Inner, Rounding, StorageKind,
};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use shrink_wrap::DeserializeShrinkWrapOwned;

/// Converts stored numbers by `factor` (the quotient of the source and target scale, a `Magnitude`) into the
/// storage `to` (`"i8"`, `"u16"`, `"f32"`, ...), rounded by `rounding` (`"truncate"`, `"floor"`, `"ceil"`,
/// `"nearest_even"`, `"nearest_away"`) and settled by `overflow` (`"checked"`: `None` for a result `to` can't hold,
/// `"saturate"`, `"wrap"`). The factor is a `Magnitude` as the API model has it: its wire bytes, or the `dict`
/// `decode` gives for one (`{"pow10": -4, "rest": None}` is 1/100, every exponent doubled).
#[pyclass(module = "wire_weaver", name = "Requantizer", frozen)]
pub(crate) struct Requantizer {
    q: Inner,
    desc: String,
}

#[pymethods]
impl Requantizer {
    #[new]
    #[pyo3(signature = (factor, to, rounding, overflow = "checked"))]
    fn new(factor: &Bound<'_, PyAny>, to: &str, rounding: &str, overflow: &str) -> PyResult<Self> {
        let m = magnitude(factor)?;
        Ok(Requantizer {
            q: Inner::new(
                &m,
                storage(to)?,
                rounding_of(rounding)?,
                overflow_of(overflow)?,
            ),
            desc: format!("Requantizer(× {m} to {to}, {rounding}, {overflow})"),
        })
    }

    /// `value` (an `int` or a `float`, the number as stored) converted: an `int` for an integer storage, a `float`
    /// for `f32` / `f64`, `None` where `overflow` is `"checked"` and the result doesn't fit.
    fn apply<'py>(
        &self,
        py: Python<'py>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let x = if value.is_instance_of::<pyo3::types::PyFloat>() {
            Repr::Float(value.extract()?)
        } else {
            Repr::Int(value.extract().map_err(|_| {
                PyTypeError::new_err(format!("{value} is not an int or float of up to 128 bits"))
            })?)
        };
        Ok(match self.q.apply(x) {
            None => None,
            Some(Repr::Int(i)) => Some(i.into_pyobject(py)?.into_any()),
            Some(Repr::Float(f)) => Some(f.into_pyobject(py)?.into_any()),
        })
    }

    fn __call__<'py>(
        &self,
        py: Python<'py>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.apply(py, value)
    }

    fn __repr__(&self) -> String {
        self.desc.clone()
    }
}

/// `value` converted by `factor` into `to` with `rounding` and `overflow`: `Requantizer(factor, to, rounding,
/// overflow).apply(value)`, for one value.
#[pyfunction]
#[pyo3(signature = (value, factor, to, rounding, overflow = "checked"))]
pub(crate) fn requantize<'py>(
    py: Python<'py>,
    value: &Bound<'py, PyAny>,
    factor: &Bound<'py, PyAny>,
    to: &str,
    rounding: &str,
    overflow: &str,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    Requantizer::new(factor, to, rounding, overflow)?.apply(py, value)
}

/// A `Magnitude` from its wire bytes or the `dict` of its fields.
fn magnitude(factor: &Bound<'_, PyAny>) -> PyResult<Magnitude> {
    if let Ok(b) = factor.cast::<PyBytes>() {
        return Magnitude::from_ww_bytes_owned(b.as_bytes())
            .map_err(|e| PyValueError::new_err(format!("not a Magnitude: {e:?}")));
    }
    let d = factor.cast::<PyDict>().map_err(|_| {
        PyTypeError::new_err("the factor is a Magnitude: its wire bytes or a dict of its fields")
    })?;
    let pow10: i8 = field(d, "pow10")?.extract()?;
    let rest = field(d, "rest")?;
    let (mut pow2, mut irr, mut primes) = (
        0,
        Irrational {
            pi: 0,
            ln2: 0,
            ln10: 0,
        },
        vec![],
    );
    if !rest.is_none() {
        let rest = rest.cast::<PyDict>()?;
        pow2 = field(rest, "pow2")?.extract()?;
        let i = field(rest, "irrational")?;
        if !i.is_none() {
            let i = i.cast::<PyDict>()?;
            irr = Irrational {
                pi: field(i, "pi")?.extract()?,
                ln2: field(i, "ln2")?.extract()?,
                ln10: field(i, "ln10")?.extract()?,
            };
        }
        for f in field(rest, "factors")?.try_iter()? {
            let f = f?;
            if f.is_none() {
                continue;
            }
            let f = f.cast::<PyDict>()?;
            primes.push(PrimePower {
                prime: field(f, "prime")?.extract()?,
                exp: field(f, "exp")?.extract()?,
            });
        }
    }
    Magnitude::from_parts_x2(pow10, pow2, irr, primes)
        .map_err(|e| PyValueError::new_err(format!("not a Magnitude: {e:?}")))
}

fn field<'py>(d: &Bound<'py, PyDict>, name: &str) -> PyResult<Bound<'py, PyAny>> {
    d.get_item(name)?
        .ok_or_else(|| PyValueError::new_err(format!("Magnitude without '{name}'")))
}

fn rounding_of(name: &str) -> PyResult<Rounding> {
    Ok(match name {
        "truncate" => Rounding::Truncate,
        "floor" => Rounding::Floor,
        "ceil" => Rounding::Ceil,
        "nearest_even" => Rounding::NearestEven,
        "nearest_away" => Rounding::NearestAway,
        _ => {
            return Err(PyValueError::new_err(format!(
                "no rounding '{name}': truncate, floor, ceil, nearest_even or nearest_away"
            )));
        }
    })
}

fn overflow_of(name: &str) -> PyResult<Overflow> {
    Ok(match name {
        "checked" => Overflow::Checked,
        "saturate" => Overflow::Saturate,
        "wrap" => Overflow::Wrap,
        _ => {
            return Err(PyValueError::new_err(format!(
                "no overflow policy '{name}': checked, saturate or wrap"
            )));
        }
    })
}

/// A storage by its name: `"u8"`..`"u64"`, `"i8"`..`"i64"`, `"f32"`, `"f64"`.
fn storage(name: &str) -> PyResult<StorageKind> {
    let int = |bits, signed| StorageKind::Int { bits, signed };
    Ok(match name {
        "u8" => int(8, false),
        "u16" => int(16, false),
        "u32" => int(32, false),
        "u64" => int(64, false),
        "i8" => int(8, true),
        "i16" => int(16, true),
        "i32" => int(32, true),
        "i64" => int(64, true),
        "f32" => StorageKind::F32,
        "f64" => StorageKind::F64,
        _ => return Err(PyValueError::new_err(format!("no storage '{name}'"))),
    })
}
