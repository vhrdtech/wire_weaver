//! A `Series<T>` as numpy columns (CLT-29): one array per leaf field over all items, from the column-by-column
//! dynamic decoder ([ValueOwned::des_series_columns_dyn]).
//!
//! Keys are the field paths joined with `.`: `value`, `status.Fault.code` (a variant's fields under its name),
//! `pair.0` (tuple, tuple struct and array elements by position), `""` for a `Series` of plain numbers. An enum
//! also gives the variant name of each item under its own path (`status`). An `Option` is transparent: its value's
//! columns are under the same path. A leaf that some items don't have (in another variant, or `None`) is a masked
//! array (`numpy.ma`), masked at those items.
//!
//! Numbers and bools get their numpy dtype (`uint16`, `float32`, `bool`, ..), 128-bit numbers and everything else
//! (strings, byte lists, `#[series(opaque)]` fields) are object arrays of what [to_py] gives for one value.
//! numpy is imported when called, it is not a dependency of the package.

use crate::convert::to_py;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use wire_weaver_client::ww_numeric::{NumericAnyTypeOwned, NumericBaseType};
use wire_weaver_client::ww_self::{ApiBundleOwned, FieldsOwned, TypeOwned, ValueOwned};
use wire_weaver_client::ww_self::{DecodedSeries, FieldsKind, SeriesColumn};

/// `dict` of path -> numpy array of a `Series` of `ty` in `data`.
pub(crate) fn decode_columns<'py>(
    py: Python<'py>,
    data: &[u8],
    ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) -> PyResult<Bound<'py, PyDict>> {
    let TypeOwned::Series(item_ty) = resolve(ty, bundle)? else {
        return Err(PyValueError::new_err(
            "decode_columns() is only for a Series<T>",
        ));
    };
    let decoded = ValueOwned::des_series_columns_dyn(data, ty, bundle)
        .map_err(|e| crate::WireWeaverError::new_err(format!("{e:#}")))?;
    let DecodedSeries { len, root } = decoded;
    let rows: Vec<usize> = (0..len).collect();
    let mut cols = Columns {
        py,
        bundle,
        numpy: py.import("numpy")?,
        len,
        out: PyDict::new(py),
        cells_left: MAX_CELLS,
    };
    cols.column(&root, item_ty, &rows, "")?;
    Ok(cols.out)
}

struct Columns<'py, 'a> {
    py: Python<'py>,
    bundle: &'a ApiBundleOwned,
    numpy: Bound<'py, PyModule>,
    /// Number of items in the `Series`, the length of each array.
    len: usize,
    out: Bound<'py, PyDict>,
    cells_left: usize,
}

impl<'py> Columns<'py, '_> {
    /// Columns of `col` of type `ty`, its items are the items `rows` of the `Series`.
    fn column(
        &mut self,
        col: &SeriesColumn,
        ty: &TypeOwned,
        rows: &[usize],
        path: &str,
    ) -> PyResult<()> {
        let resolved = resolve(ty, self.bundle)?;
        match col {
            // the columns of a type are only known from its bytes (an evolved struct may have more fields than
            // its definition), a group no item is in has none
            SeriesColumn::Empty => Ok(()),
            SeriesColumn::Leaf(values) => self.leaf(values.iter(), resolved, rows, path),
            SeriesColumn::Default { value, len } => {
                self.leaf(std::iter::repeat_n(value, *len), resolved, rows, path)
            }
            SeriesColumn::Fields { kind, fields, .. } => {
                let shape = match resolved {
                    TypeOwned::Struct(item_struct) => Shape::Fields(&item_struct.fields),
                    TypeOwned::Tuple(types) => Shape::Tuple(types),
                    TypeOwned::Array { ty, .. } => Shape::Array(ty),
                    _ => return Err(mismatch(path)),
                };
                self.fields(fields, shape, *kind, rows, path)
            }
            SeriesColumn::Enum {
                variant_of,
                variants,
            } => {
                let TypeOwned::Enum(item_enum) = resolved else {
                    return Err(mismatch(path));
                };
                let names: Vec<ValueOwned> = variant_of
                    .iter()
                    .map(|&idx| {
                        variants
                            .get(idx as usize)
                            .map(|v| ValueOwned::String(v.ident.clone()))
                            .ok_or_else(|| mismatch(path))
                    })
                    .collect::<PyResult<_>>()?;
                self.leaf(names.iter(), &TypeOwned::String, rows, path)?;
                // the rows of each variant, in one pass over the items
                let mut rows_of: Vec<Vec<usize>> = vec![vec![]; variants.len()];
                for (&row, &slot) in rows.iter().zip(variant_of) {
                    rows_of
                        .get_mut(slot as usize)
                        .ok_or_else(|| mismatch(path))?
                        .push(row);
                }
                for (variant, variant_rows) in variants.iter().zip(rows_of) {
                    let def = item_enum
                        .variants
                        .get(variant.variant_idx as usize)
                        .filter(|d| d.ident == variant.ident)
                        .ok_or_else(|| mismatch(path))?;
                    let path = join(path, &variant.ident);
                    match &variant.fields {
                        SeriesColumn::Fields { kind, fields, .. } => self.fields(
                            fields,
                            Shape::Fields(&def.fields),
                            *kind,
                            &variant_rows,
                            &path,
                        )?,
                        SeriesColumn::Empty => {}
                        _ => return Err(mismatch(&path)),
                    }
                }
                Ok(())
            }
            SeriesColumn::Option { some, inner } => {
                let TypeOwned::Option { some_ty } = resolved else {
                    return Err(mismatch(path));
                };
                let some_rows = subset(rows, some, |s| s);
                self.column(inner, some_ty, &some_rows, path)
            }
        }
    }

    fn fields(
        &mut self,
        fields: &[(String, SeriesColumn)],
        shape: Shape<'_>,
        kind: FieldsKind,
        rows: &[usize],
        path: &str,
    ) -> PyResult<()> {
        // the decoder gives one column per field of the type (flags left out), in its order
        let defs = shape
            .field_types(kind, fields.len())
            .ok_or_else(|| mismatch(path))?;
        if defs.len() != fields.len() {
            return Err(mismatch(path));
        }
        for ((name, col), (ident, ty)) in fields.iter().zip(defs) {
            let path = join(path, name);
            if ident.is_some_and(|ident| ident != name) {
                return Err(mismatch(&path));
            }
            self.column(col, ty, rows, &path)?;
        }
        Ok(())
    }

    /// One array of `self.len` items, the values at `rows`, masked at the others.
    fn leaf<'v>(
        &mut self,
        values: impl Iterator<Item = &'v ValueOwned>,
        ty: &TypeOwned,
        rows: &[usize],
        path: &str,
    ) -> PyResult<()> {
        let py = self.py;
        if self.out.contains(path)? {
            return Err(PyValueError::new_err(format!(
                "two columns at '{path}' (an Option of an Option?)"
            )));
        }
        // the decode's budget bounds the values, not the columns times the items: many variants with fields
        // each, all of them as long as the `Series`
        self.cells_left = self
            .cells_left
            .checked_sub(self.len)
            .ok_or_else(|| PyValueError::new_err(format!("more than {MAX_CELLS} cells")))?;
        let values: Vec<_> = values.collect();
        if values.len() != rows.len() {
            return Err(mismatch(path));
        }
        let np = &self.numpy;
        let dtype = dtype(ty, self.bundle);
        let present = match dtype {
            Some(dtype) => {
                let items = values
                    .iter()
                    .map(|v| to_py(py, v, ty, self.bundle))
                    .collect::<PyResult<Vec<_>>>()?;
                np.call_method1("array", (PyList::new(py, items)?, dtype))?
            }
            None => {
                // element by element: `numpy.array` would make a list of lists 2-D
                let array = np.call_method1("empty", (values.len(), "object"))?;
                for (idx, v) in values.iter().enumerate() {
                    array.set_item(idx, to_py(py, v, ty, self.bundle)?)?;
                }
                array
            }
        };
        let array = if rows.len() == self.len {
            present
        } else {
            let array = np.call_method1("zeros", (self.len, dtype.unwrap_or("object")))?;
            let rows = np.call_method1("array", (PyList::new(py, rows)?, "intp"))?;
            array.set_item(&rows, present)?;
            let mask = np.call_method1("ones", (self.len, "bool"))?;
            mask.set_item(&rows, false)?;
            np.getattr("ma")?
                .call_method1("masked_array", (array, mask))?
        };
        self.out.set_item(path, array)
    }
}

/// Most cells (array elements) of all the columns of one `Series` together.
const MAX_CELLS: usize = 1 << 24;

/// numpy dtype of a leaf of type `ty`, `None` for an object array.
fn dtype(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Option<&'static str> {
    let ty = ty.get_in_line(bundle).ok()?;
    let any = match ty {
        TypeOwned::Bool => return Some("bool"),
        TypeOwned::NumericAny(any) => any,
        TypeOwned::MaxLen { ty, .. } => return dtype(ty, bundle),
        _ => return None,
    };
    let base = match any {
        NumericAnyTypeOwned::Base(base)
        | NumericAnyTypeOwned::SubType { base, .. }
        | NumericAnyTypeOwned::ShiftScale { base, .. } => base,
    };
    use NumericBaseType as B;
    Some(match base {
        B::Nibble | B::U8 => "uint8",
        B::U16 => "uint16",
        B::U32 | B::UNib32 | B::UVlq32 => "uint32",
        B::U64 | B::UVlq64 => "uint64",
        B::I8 => "int8",
        B::I16 => "int16",
        B::I32 => "int32",
        B::I64 => "int64",
        B::F32 => "float32",
        B::F64 => "float64",
        B::UB(bits) => match bits.0 {
            0..=8 => "uint8",
            9..=16 => "uint16",
            17..=32 => "uint32",
            _ => "uint64",
        },
        B::IB(bits) => match bits.0 {
            0..=8 => "int8",
            9..=16 => "int16",
            17..=32 => "int32",
            _ => "int64",
        },
        _ => return None,
    })
}

/// The fields of a struct (or enum variant), tuple or array.
#[derive(Clone, Copy)]
enum Shape<'t> {
    Fields(&'t FieldsOwned),
    Tuple(&'t [TypeOwned]),
    Array(&'t TypeOwned),
}

impl<'t> Shape<'t> {
    /// The name (of a named field) and type of each field in column order, `columns` of them for an array;
    /// `None` when the shape is not of `kind`.
    fn field_types(
        self,
        kind: FieldsKind,
        columns: usize,
    ) -> Option<Vec<(Option<&'t str>, &'t TypeOwned)>> {
        Some(match (self, kind) {
            (Shape::Tuple(types), FieldsKind::Tuple) => types.iter().map(|ty| (None, ty)).collect(),
            (Shape::Array(ty), FieldsKind::Array) => vec![(None, ty); columns],
            (Shape::Fields(FieldsOwned::Unit), FieldsKind::Unit) => vec![],
            (Shape::Fields(FieldsOwned::Named(defs)), FieldsKind::Named)
            | (Shape::Fields(FieldsOwned::Unnamed(defs)), FieldsKind::Unnamed) => defs
                .iter()
                .filter(|d| !d.is_flag())
                .map(|d| (d.ident.as_deref(), &d.ty))
                .collect(),
            _ => return None,
        })
    }
}

/// The rows whose flag passes `keep`.
fn subset<T: Copy>(rows: &[usize], flags: &[T], keep: impl Fn(T) -> bool) -> Vec<usize> {
    rows.iter()
        .zip(flags)
        .filter(|(_, f)| keep(**f))
        .map(|(r, _)| *r)
        .collect()
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}.{name}")
    }
}

fn mismatch(path: &str) -> PyErr {
    PyValueError::new_err(format!("decoded columns don't match the type at '{path}'"))
}

fn resolve<'a>(ty: &'a TypeOwned, bundle: &'a ApiBundleOwned) -> PyResult<&'a TypeOwned> {
    ty.get_in_line(bundle)
        .map_err(|e| PyValueError::new_err(format!("{e:#}")))
}
