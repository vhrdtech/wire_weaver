//! Python objects mirroring the API tree: traits are attributes, arrays are indexed with `[]`, methods are callable,
//! properties are read and written with `read()` / `write()`, streams are iterated.

use crate::convert::{prefix_err, to_py, to_value, type_name};
use crate::{Conn, RemoteError, WireWeaverError, client_err, err_msg, stream_err};
use pyo3::exceptions::{PyAttributeError, PyIndexError, PyKeyError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyString, PyTuple};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wire_weaver_client::ww_self::{ApiItemKindOwned, PropertyAccess, TypeOwned, ValueOwned};
use wire_weaver_client::{DynResource, DynResourceKind, DynSink, DynStream};

/// Python object for a resource: trait node, array, method, property, stream or sink.
pub(crate) fn wrap(
    py: Python<'_>,
    res: DynResource,
    conn: Option<Arc<Conn>>,
) -> PyResult<Py<PyAny>> {
    let obj = match res.kind().map_err(client_err)? {
        DynResourceKind::Level(_) => Py::new(py, Node { res, conn })?.into_any(),
        DynResourceKind::Array(_) => Py::new(py, Array { res, conn })?.into_any(),
        DynResourceKind::Method { .. } => Py::new(py, Method { res, conn })?.into_any(),
        DynResourceKind::Property { .. } => Py::new(py, Property { res, conn })?.into_any(),
        DynResourceKind::Stream { is_up: true, .. } => Py::new(
            py,
            Stream {
                res,
                conn,
                stream: Mutex::new(None),
            },
        )?
        .into_any(),
        DynResourceKind::Stream { is_up: false, .. } => Py::new(
            py,
            Sink {
                res,
                conn,
                sink: Mutex::new(None),
            },
        )?
        .into_any(),
    };
    Ok(obj)
}

fn conn<'a>(res: &DynResource, conn: &'a Option<Arc<Conn>>) -> PyResult<&'a Arc<Conn>> {
    conn.as_ref().ok_or_else(|| {
        WireWeaverError::new_err(format!(
            "'{}' belongs to an API loaded offline, connect to a device to use it",
            res.path()
        ))
    })
}

fn timeout(t: Option<f64>) -> Option<Duration> {
    t.map(Duration::from_secs_f64)
}

fn docs(res: &DynResource) -> String {
    res.item()
        .map(|i| {
            i.docs
                .iter()
                .map(|d| d.trim())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// One-line signature, e.g. `add(a: u32, b: i16) -> i64`, `rw speed: u16`, `stream samples: Vec<u8>`.
pub(crate) fn signature(res: &DynResource) -> String {
    let Some(item) = res.item() else {
        return "API root".into();
    };
    let bundle = res.bundle();
    let array = if item.is_array() { "[]" } else { "" };
    let name = &item.ident;
    match &item.kind {
        ApiItemKindOwned::Method { args, return_ty } => {
            let args: Vec<_> = args
                .iter()
                .map(|a| format!("{}: {}", a.ident, type_name(&a.ty, bundle)))
                .collect();
            let ret = return_ty
                .as_ref()
                .map(|t| format!(" -> {}", type_name(t, bundle)))
                .unwrap_or_default();
            format!("{name}{array}({}){ret}", args.join(", "))
        }
        ApiItemKindOwned::Property { ty, access, .. } => {
            let access = match access {
                PropertyAccess::Const => "const",
                PropertyAccess::ReadOnly { .. } => "ro",
                PropertyAccess::ReadWrite { .. } => "rw",
                PropertyAccess::WriteOnly => "wo",
            };
            format!("{access} {name}{array}: {}", type_name(ty, bundle))
        }
        ApiItemKindOwned::Stream { ty, is_up } => {
            let kind = if *is_up { "stream" } else { "sink" };
            format!("{kind} {name}{array}: {}", type_name(ty, bundle))
        }
        ApiItemKindOwned::Trait { .. } => {
            let level = item.get_as_level(bundle).map(|l| l.trait_name.clone());
            format!("{name}{array}: {}", level.unwrap_or_default())
        }
    }
}

fn repr_with_docs(kind: &str, res: &DynResource) -> String {
    let docs = docs(res);
    let docs = if docs.is_empty() {
        String::new()
    } else {
        format!("\n{docs}")
    };
    format!("<{kind} {}>{docs}", signature(res))
}

// ---------------------------------------------------------------------------------------------------------------------

/// API root or a trait, its resources are attributes: `dev.led_on()`, `dev.channel[0].gain.read()`.
/// Use `node["name"]` for a resource whose name clashes with a Python attribute.
#[pyclass(module = "wire_weaver", frozen)]
pub struct Node {
    pub(crate) res: DynResource,
    pub(crate) conn: Option<Arc<Conn>>,
}

impl Node {
    pub(crate) fn child(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        let child = self
            .res
            .child(name)
            .map_err(|e| PyAttributeError::new_err(err_msg(e)))?;
        wrap(py, child, self.conn.clone())
    }

    pub(crate) fn names(&self) -> Vec<String> {
        self.res
            .children()
            .iter()
            .map(|i| i.ident.clone())
            .collect()
    }

    /// Resource tree, one resource per line with its signature and first doc line.
    pub(crate) fn tree(&self) -> String {
        let mut out = String::new();
        let level = match self.res.kind() {
            Ok(DynResourceKind::Level(level)) => level,
            _ => return out,
        };
        let title = if self.res.path().is_empty() {
            level.trait_name.clone()
        } else {
            format!("{}: {}", self.res.path(), level.trait_name)
        };
        out.push_str(&title);
        for item in &level.items {
            let Ok(child) = self.res.child(&item.ident) else {
                continue;
            };
            out.push_str("\n  ");
            out.push_str(&signature(&child));
            if let Some(first) = item.docs.first() {
                out.push_str("  # ");
                out.push_str(first.trim());
            }
        }
        out
    }
}

#[pymethods]
impl Node {
    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        self.child(py, name)
    }

    fn __getitem__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        self.child(py, name)
            .map_err(|e| PyKeyError::new_err(e.value(py).to_string()))
    }

    fn __dir__(&self) -> Vec<String> {
        self.names()
    }

    fn __repr__(&self) -> String {
        self.tree()
    }

    /// Doc comment of this trait.
    #[getter]
    fn doc(&self) -> String {
        docs(&self.res)
    }

    /// Path from the API root, e.g. `periph[1].channel[0]`.
    #[getter]
    fn path(&self) -> String {
        self.res.path().to_string()
    }
}

// ---------------------------------------------------------------------------------------------------------------------

/// Array of traits, methods, properties or streams, index it: `dev.channel[0]`.
#[pyclass(module = "wire_weaver", frozen)]
pub struct Array {
    res: DynResource,
    conn: Option<Arc<Conn>>,
}

#[pymethods]
impl Array {
    fn __getitem__(&self, py: Python<'_>, index: i64) -> PyResult<Py<PyAny>> {
        let index = u32::try_from(index)
            .map_err(|_| PyIndexError::new_err(format!("index {index} is out of range")))?;
        let res = self.res.index(index).map_err(client_err)?;
        wrap(py, res, self.conn.clone())
    }

    /// Indices a device accepts, as a list.
    fn valid_indices(&self, py: Python<'_>) -> PyResult<Vec<u32>> {
        let conn = conn(&self.res, &self.conn)?;
        let res = &self.res;
        py.detach(|| res.valid_indices_blocking(conn.client.cmd()))
            .map(|v| v.iter().collect())
            .map_err(client_err)
    }

    fn __repr__(&self) -> String {
        repr_with_docs("array", &self.res)
    }

    #[getter]
    fn doc(&self) -> String {
        docs(&self.res)
    }

    #[getter]
    fn path(&self) -> String {
        self.res.path().to_string()
    }
}

// ---------------------------------------------------------------------------------------------------------------------

/// Method, call it with positional or keyword arguments: `dev.add(1, b=2)`.
///
/// A method returning `Result<T, E>` returns `T` or raises `RemoteError` with `E` in its `value` attribute.
#[pyclass(module = "wire_weaver", frozen)]
pub struct Method {
    res: DynResource,
    conn: Option<Arc<Conn>>,
}

impl Method {
    fn args(
        &self,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Vec<ValueOwned>> {
        let Ok(DynResourceKind::Method { args: defs, .. }) = self.res.kind() else {
            unreachable!("Method wraps a method");
        };
        let bundle = self.res.bundle();
        let name = self.res.path();
        if args.len() > defs.len() {
            return Err(PyTypeError::new_err(format!(
                "{name}() takes {} arguments, got {}",
                defs.len(),
                args.len()
            )));
        }
        let mut slots: Vec<Option<Bound<'_, PyAny>>> = args.iter().map(Some).collect();
        slots.resize(defs.len(), None);
        if let Some(kwargs) = kwargs {
            for (k, v) in kwargs.iter() {
                let k = k.cast::<PyString>()?.to_str()?.to_string();
                let Some(idx) = defs.iter().position(|d| d.ident == k) else {
                    return Err(PyTypeError::new_err(format!(
                        "{name}() got an unexpected keyword argument '{k}'"
                    )));
                };
                if slots[idx].is_some() {
                    return Err(PyTypeError::new_err(format!(
                        "{name}() got multiple values for argument '{k}'"
                    )));
                }
                slots[idx] = Some(v);
            }
        }
        defs.iter()
            .zip(slots)
            .map(|(def, slot)| {
                let obj = slot.ok_or_else(|| {
                    PyTypeError::new_err(format!(
                        "{name}() missing argument '{}: {}'",
                        def.ident,
                        type_name(&def.ty, bundle)
                    ))
                })?;
                to_value(&obj, &def.ty, bundle).map_err(|e| {
                    prefix_err(obj.py(), e, &format!("{name}() argument '{}'", def.ident))
                })
            })
            .collect()
    }

    fn return_ty(&self) -> Option<TypeOwned> {
        match self.res.kind() {
            Ok(DynResourceKind::Method { return_ty, .. }) => return_ty.cloned(),
            _ => None,
        }
    }

    fn return_to_py(&self, py: Python<'_>, value: Option<ValueOwned>) -> PyResult<Py<PyAny>> {
        let (Some(value), Some(ty)) = (value, self.return_ty()) else {
            return Ok(py.None());
        };
        let bundle = self.res.bundle();
        // Result<T, E> return: T or raise RemoteError(E)
        if let (ValueOwned::Result(r), Ok(TypeOwned::Result { ok_ty, err_ty })) =
            (&value, ty.get_in_line(bundle))
        {
            return match r {
                Ok(v) => Ok(to_py(py, v, ok_ty, bundle)?.unbind()),
                Err(e) => {
                    let e = to_py(py, e, err_ty, bundle)?;
                    let msg = format!("{}() returned an error: {}", self.res.path(), e.repr()?);
                    let err = RemoteError::new_err(msg);
                    err.value(py).setattr("value", e)?;
                    Err(err)
                }
            };
        }
        Ok(to_py(py, &value, &ty, bundle)?.unbind())
    }
}

#[pymethods]
impl Method {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let conn = conn(&self.res, &self.conn)?;
        let values = self.args(args, kwargs)?;
        let res = &self.res;
        let timeout = conn.timeout;
        let reply = py
            .detach(|| res.call_blocking(conn.client.cmd(), &values, timeout))
            .map_err(client_err)?;
        self.return_to_py(py, reply)
    }

    /// Serialize arguments as they are sent to a device, without calling.
    #[pyo3(signature = (*args, **kwargs))]
    fn encode_args<'py>(
        &self,
        py: Python<'py>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let values = self.args(args, kwargs)?;
        let Ok(DynResourceKind::Method { args: defs, .. }) = self.res.kind() else {
            unreachable!();
        };
        let bytes = ValueOwned::ser_shrink_wrap_vec_dyn(
            &values,
            defs.iter().map(|d| &d.ty),
            self.res.bundle(),
        )
        .map_err(|e| PyTypeError::new_err(format!("{e:#}")))?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Deserialize a return value as sent by a device, `Result` is returned as `{"Ok": ..}` or `{"Err": ..}`.
    fn decode_return(&self, py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
        let Some(ty) = self.return_ty() else {
            return Ok(py.None());
        };
        let bundle = self.res.bundle();
        let value = ValueOwned::des_shrink_wrap_dyn(data, &ty, bundle)
            .map_err(|e| WireWeaverError::new_err(format!("{e:#}")))?;
        Ok(to_py(py, &value, &ty, bundle)?.unbind())
    }

    fn __repr__(&self) -> String {
        repr_with_docs("method", &self.res)
    }

    #[getter]
    fn doc(&self) -> String {
        docs(&self.res)
    }

    #[getter]
    fn path(&self) -> String {
        self.res.path().to_string()
    }

    /// Signature, e.g. `add(a: u32, b: i16) -> i64`.
    #[getter]
    fn signature(&self) -> String {
        signature(&self.res)
    }
}

// ---------------------------------------------------------------------------------------------------------------------

/// Property, `read()` and `write(value)` it.
#[pyclass(module = "wire_weaver", frozen)]
pub struct Property {
    res: DynResource,
    conn: Option<Arc<Conn>>,
}

impl Property {
    fn ty(&self) -> TypeOwned {
        match self.res.kind() {
            Ok(DynResourceKind::Property { ty, .. }) => ty.clone(),
            _ => unreachable!("Property wraps a property"),
        }
    }
}

#[pymethods]
impl Property {
    /// Read the value from a device, `timeout` in seconds overrides the default one.
    #[pyo3(signature = (timeout=None))]
    fn read(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Py<PyAny>> {
        let conn = conn(&self.res, &self.conn)?;
        let res = &self.res;
        let t = self::timeout(timeout).or(conn.timeout);
        let value = py
            .detach(|| res.read_blocking(conn.client.cmd(), t))
            .map_err(client_err)?;
        Ok(to_py(py, &value, &self.ty(), self.res.bundle())?.unbind())
    }

    /// Write the value to a device, `timeout` in seconds overrides the default one.
    #[pyo3(signature = (value, timeout=None))]
    fn write(
        &self,
        py: Python<'_>,
        value: &Bound<'_, PyAny>,
        timeout: Option<f64>,
    ) -> PyResult<()> {
        let conn = conn(&self.res, &self.conn)?;
        let value = to_value(value, &self.ty(), self.res.bundle())?;
        let res = &self.res;
        let t = self::timeout(timeout).or(conn.timeout);
        py.detach(|| res.write_blocking(conn.client.cmd(), &value, t))
            .map_err(client_err)
    }

    /// Serialize a value as it is sent to a device.
    fn encode<'py>(
        &self,
        py: Python<'py>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let ty = self.ty();
        let value = to_value(value, &ty, self.res.bundle())?;
        let bytes = value
            .ser_shrink_wrap_dyn(&ty, self.res.bundle())
            .map_err(|e| PyTypeError::new_err(format!("{e:#}")))?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Deserialize a value as sent by a device.
    fn decode(&self, py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
        let ty = self.ty();
        let value = ValueOwned::des_shrink_wrap_dyn(data, &ty, self.res.bundle())
            .map_err(|e| WireWeaverError::new_err(format!("{e:#}")))?;
        Ok(to_py(py, &value, &ty, self.res.bundle())?.unbind())
    }

    fn __repr__(&self) -> String {
        repr_with_docs("property", &self.res)
    }

    #[getter]
    fn doc(&self) -> String {
        docs(&self.res)
    }

    #[getter]
    fn path(&self) -> String {
        self.res.path().to_string()
    }

    #[getter]
    fn signature(&self) -> String {
        signature(&self.res)
    }
}

// ---------------------------------------------------------------------------------------------------------------------

/// Stream from a device: `open()`, then `recv()` or iterate over it, `close()`. Also a context manager:
/// `with dev.samples as s: for x in s: ...`.
#[pyclass(module = "wire_weaver", frozen)]
pub struct Stream {
    res: DynResource,
    conn: Option<Arc<Conn>>,
    stream: Mutex<Option<DynStream>>,
}

impl Stream {
    fn with_stream<T>(&self, f: impl FnOnce(&mut DynStream) -> PyResult<T>) -> PyResult<T> {
        let mut guard = self.stream.lock().expect("not poisoned");
        let stream = guard.as_mut().ok_or_else(|| {
            WireWeaverError::new_err(format!("'{}' is not open", self.res.path()))
        })?;
        f(stream)
    }

    fn item_to_py(&self, py: Python<'_>, value: ValueOwned) -> PyResult<Py<PyAny>> {
        let ty = match self.res.kind() {
            Ok(DynResourceKind::Stream { ty, .. }) => ty.clone(),
            _ => unreachable!("Stream wraps a stream"),
        };
        Ok(to_py(py, &value, &ty, self.res.bundle())?.unbind())
    }

    /// Wait for one item, checking for Ctrl+C every 100ms, None on timeout.
    fn recv_inner(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Option<ValueOwned>> {
        let conn = conn(&self.res, &self.conn)?;
        let deadline = self::timeout(timeout).map(|t| std::time::Instant::now() + t);
        loop {
            let slice = match deadline {
                Some(d) => {
                    let left = d.saturating_duration_since(std::time::Instant::now());
                    if left.is_zero() {
                        return Ok(None);
                    }
                    left.min(Duration::from_millis(100))
                }
                None => Duration::from_millis(100),
            };
            let got = self.with_stream(|stream| {
                Ok(py.detach(|| {
                    conn.runtime
                        .block_on(async { tokio::time::timeout(slice, stream.recv()).await })
                }))
            })?;
            match got {
                Ok(item) => return item.map(Some).map_err(stream_err),
                Err(_elapsed) => py.check_signals()?,
            }
        }
    }
}

#[pymethods]
impl Stream {
    /// Subscribe and ask a device to start the stream.
    fn open(&self, py: Python<'_>) -> PyResult<()> {
        let conn = conn(&self.res, &self.conn)?;
        let mut guard = self.stream.lock().expect("not poisoned");
        if guard.is_none() {
            let res = &self.res;
            *guard = Some(
                py.detach(|| res.stream_blocking(conn.client.cmd()))
                    .map_err(client_err)?,
            );
        }
        let stream = guard.as_ref().expect("just set");
        py.detach(|| stream.open_blocking()).map_err(stream_err)
    }

    /// Ask a device to stop the stream and unsubscribe.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        let stream = self.stream.lock().expect("not poisoned").take();
        if let Some(stream) = stream {
            py.detach(|| stream.close_blocking()).map_err(stream_err)?;
        }
        Ok(())
    }

    /// Wait for the next item, `timeout` in seconds (None waits forever), raises TimeoutError.
    #[pyo3(signature = (timeout=None))]
    fn recv(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Py<PyAny>> {
        match self.recv_inner(py, timeout)? {
            Some(value) => self.item_to_py(py, value),
            None => Err(pyo3::exceptions::PyTimeoutError::new_err(format!(
                "no item from '{}'",
                self.res.path()
            ))),
        }
    }

    /// Next item if one is already received, None otherwise.
    fn try_recv(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let item = self.with_stream(|s| s.try_recv().map_err(stream_err))?;
        item.map(|v| self.item_to_py(py, v)).transpose()
    }

    fn __iter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    fn __next__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self.recv_inner(py, None) {
            Ok(Some(value)) => self.item_to_py(py, value),
            Ok(None) => unreachable!("no timeout"),
            Err(e) => Err(e),
        }
    }

    fn __enter__(slf: Py<Self>, py: Python<'_>) -> PyResult<Py<Self>> {
        slf.get().open(py)?;
        Ok(slf)
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, PyTuple>) -> PyResult<bool> {
        self.close(py)?;
        Ok(false)
    }

    /// Deserialize an item as sent by a device.
    fn decode(&self, py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
        let ty = match self.res.kind() {
            Ok(DynResourceKind::Stream { ty, .. }) => ty.clone(),
            _ => unreachable!(),
        };
        if ty.is_byte_slice(self.res.bundle()).unwrap_or(false) {
            return Ok(PyBytes::new(py, data).into_any().unbind());
        }
        let value = ValueOwned::des_shrink_wrap_dyn(data, &ty, self.res.bundle())
            .map_err(|e| WireWeaverError::new_err(format!("{e:#}")))?;
        self.item_to_py(py, value)
    }

    fn __repr__(&self) -> String {
        repr_with_docs("stream", &self.res)
    }

    #[getter]
    fn doc(&self) -> String {
        docs(&self.res)
    }

    #[getter]
    fn path(&self) -> String {
        self.res.path().to_string()
    }

    #[getter]
    fn signature(&self) -> String {
        signature(&self.res)
    }
}

// ---------------------------------------------------------------------------------------------------------------------

/// Sink on a device: `open()`, `send(value)`, `close()`. Also a context manager.
#[pyclass(module = "wire_weaver", frozen)]
pub struct Sink {
    res: DynResource,
    conn: Option<Arc<Conn>>,
    sink: Mutex<Option<DynSink>>,
}

impl Sink {
    fn ty(&self) -> TypeOwned {
        match self.res.kind() {
            Ok(DynResourceKind::Stream { ty, .. }) => ty.clone(),
            _ => unreachable!("Sink wraps a sink"),
        }
    }
}

#[pymethods]
impl Sink {
    fn open(&self, py: Python<'_>) -> PyResult<()> {
        let conn = conn(&self.res, &self.conn)?;
        let mut guard = self.sink.lock().expect("not poisoned");
        if guard.is_none() {
            let res = &self.res;
            *guard = Some(
                py.detach(|| res.sink_blocking(conn.client.cmd()))
                    .map_err(client_err)?,
            );
        }
        let sink = guard.as_ref().expect("just set");
        py.detach(|| sink.open_blocking()).map_err(stream_err)
    }

    fn close(&self, py: Python<'_>) -> PyResult<()> {
        let sink = self.sink.lock().expect("not poisoned").take();
        if let Some(sink) = sink {
            py.detach(|| sink.close_blocking()).map_err(stream_err)?;
        }
        Ok(())
    }

    /// Send one item, `bytes` for a byte slice sink.
    fn send(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let value = to_value(value, &self.ty(), self.res.bundle())?;
        let mut guard = self.sink.lock().expect("not poisoned");
        let sink = guard.as_mut().ok_or_else(|| {
            WireWeaverError::new_err(format!("'{}' is not open", self.res.path()))
        })?;
        py.detach(|| sink.send_blocking(&value)).map_err(stream_err)
    }

    fn __enter__(slf: Py<Self>, py: Python<'_>) -> PyResult<Py<Self>> {
        slf.get().open(py)?;
        Ok(slf)
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, PyTuple>) -> PyResult<bool> {
        self.close(py)?;
        Ok(false)
    }

    /// Serialize an item as it is sent to a device.
    fn encode<'py>(
        &self,
        py: Python<'py>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let ty = self.ty();
        if ty.is_byte_slice(self.res.bundle()).unwrap_or(false) {
            let bytes: Vec<u8> = value.extract()?;
            return Ok(PyBytes::new(py, &bytes));
        }
        let value = to_value(value, &ty, self.res.bundle())?;
        let bytes = value
            .ser_shrink_wrap_dyn(&ty, self.res.bundle())
            .map_err(|e| PyTypeError::new_err(format!("{e:#}")))?;
        Ok(PyBytes::new(py, &bytes))
    }

    fn __repr__(&self) -> String {
        repr_with_docs("sink", &self.res)
    }

    #[getter]
    fn doc(&self) -> String {
        docs(&self.res)
    }

    #[getter]
    fn path(&self) -> String {
        self.res.path().to_string()
    }

    #[getter]
    fn signature(&self) -> String {
        signature(&self.res)
    }
}
