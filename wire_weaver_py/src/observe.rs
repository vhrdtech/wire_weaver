//! Observing a trait level's `+observe` properties without generated code (TOOL-11): `Node.observe(cfg)` returns
//! an [Observer], iterate it for a dict per update. Every sample of `+batched` properties (API-39) comes item by
//! item in `recv()`'s dicts, or as numpy columns per property from `recv_batch()`.

use crate::columns::decode_columns;
use crate::convert::to_py;
use crate::tree::timeout;
use crate::{Conn, WireWeaverError, client_err, stream_err};
use pyo3::exceptions::{PyKeyError, PyTimeoutError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString, PyTuple};
use shrink_wrap::UNib32;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wire_weaver_client::observe::ObserveMode;
use wire_weaver_client::ww_self::TypeOwned;
use wire_weaver_client::{DynObserved, DynObserver, DynResource, seconds};

/// A level's `+observe` properties, by resource index.
type Props = Vec<(u32, String, TypeOwned)>;

/// `cfg` (a dict `{property_name: rate_ms}`, `rate_ms` is `None` for on-change instead of periodic, or
/// `{"samples": max_age_ms, "every": ms}` for every sample of a `+batched` property, `every` optional) into a
/// request for every property of `res`'s level named in it, or every observable property on-change when `cfg` is
/// `None`: the same default `ww observe <path>` (no `--rate`) uses.
fn parse_cfg(
    res: &DynResource,
    cfg: Option<&Bound<'_, PyDict>>,
) -> PyResult<(Props, BTreeMap<u32, ObserveMode>)> {
    let props = res.observable_properties().map_err(client_err)?;
    if props.is_empty() {
        return Err(WireWeaverError::new_err(format!(
            "'{}' has no `+observe` properties",
            res.path()
        )));
    }
    let entries = match cfg {
        None => props
            .iter()
            .map(|(index, ..)| (*index, ObserveMode::OnChange { min_us: UNib32(0) }))
            .collect(),
        Some(cfg) => {
            let mut entries = BTreeMap::new();
            for (k, v) in cfg.iter() {
                let name = k.cast::<PyString>()?.to_str()?.to_string();
                let index = props
                    .iter()
                    .find(|(_, n, _)| n == &name)
                    .map(|(index, ..)| *index)
                    .ok_or_else(|| {
                        PyKeyError::new_err(format!(
                            "'{name}' is not an observable property of '{}'",
                            res.path()
                        ))
                    })?;
                let mode = if v.is_none() {
                    ObserveMode::OnChange { min_us: UNib32(0) }
                } else if let Ok(samples) = v.cast::<PyDict>() {
                    let ms = |key: &str| -> PyResult<f64> {
                        Ok(match samples.get_item(key)? {
                            Some(v) if !v.is_none() => v.extract()?,
                            _ => 0.0,
                        })
                    };
                    if samples.get_item("samples")?.is_none() {
                        return Err(PyKeyError::new_err(format!(
                            "'{name}': a dict asks for every sample: {{\"samples\": max_age_ms, \"every\": ms}}"
                        )));
                    }
                    ObserveMode::Samples {
                        every_us: micros(ms("every")?),
                        max_age_us: micros(ms("samples")?),
                    }
                } else {
                    ObserveMode::Periodic {
                        every_us: micros(v.extract()?),
                    }
                };
                entries.insert(index, mode);
            }
            entries
        }
    };
    Ok((props, entries))
}

fn micros(ms: f64) -> UNib32 {
    UNib32((ms * 1_000.0).clamp(0.0, u32::MAX as f64) as u32)
}

/// Observer of a trait level's `+observe` properties (TOOL-11), from [crate::tree::Node::observe]. Iterate it (or
/// call `recv()`) for a dict per update: `{"values": {name: value, ...}, "samples": {name: [(t, value), ...]},
/// "t": seconds or None, "missed": int, "bytes": int}`; `samples` has every sample of the `+batched` properties
/// in the update (API-39), `values` the newest of each. Also a context manager; `close()` unsubscribes (so does
/// garbage collection).
#[pyclass(module = "wire_weaver", frozen)]
pub struct Observer {
    res: DynResource,
    conn: Arc<Conn>,
    props: Props,
    observer: Mutex<Option<DynObserver>>,
}

impl Observer {
    pub(crate) fn new(
        res: DynResource,
        conn: Arc<Conn>,
        cfg: Option<&Bound<'_, PyDict>>,
        py: Python<'_>,
    ) -> PyResult<Self> {
        let (props, entries) = parse_cfg(&res, cfg)?;
        let observer = {
            let res = &res;
            py.detach(|| res.observe_blocking(conn.client.cmd(), entries))
                .map_err(client_err)?
        };
        Ok(Observer {
            res,
            conn,
            props,
            observer: Mutex::new(Some(observer)),
        })
    }

    fn ty(&self, name: &str) -> Option<&TypeOwned> {
        self.props
            .iter()
            .find(|(_, n, _)| n == name)
            .map(|(_, _, ty)| ty)
    }

    /// `recv()`'s dict, or `recv_batch()`'s with `batches` in place of `samples`.
    fn observed_to_py(
        &self,
        py: Python<'_>,
        observed: DynObserved,
        columns: bool,
    ) -> PyResult<Py<PyAny>> {
        let bundle = self.res.bundle();
        let values = PyDict::new(py);
        for (name, value) in &observed.values {
            let Some(ty) = self.ty(name) else {
                continue;
            };
            values.set_item(name, to_py(py, value, ty, bundle)?)?;
        }
        let d = PyDict::new(py);
        d.set_item("values", values)?;
        if columns {
            d.set_item("batches", self.batches_to_py(py, &observed)?)?;
        } else {
            let samples = PyDict::new(py);
            for s in &observed.samples {
                let Some(ty) = self.ty(&s.name) else {
                    continue;
                };
                let items = s
                    .items
                    .iter()
                    .map(|(value, t)| Ok((t.as_ref().map(seconds), to_py(py, value, ty, bundle)?)))
                    .collect::<PyResult<Vec<_>>>()?;
                samples.set_item(&s.name, items)?;
            }
            d.set_item("samples", samples)?;
        }
        d.set_item("t", observed.t.as_ref().map(seconds))?;
        d.set_item("missed", observed.missed)?;
        d.set_item("bytes", observed.data_len)?;
        Ok(d.into_any().unbind())
    }

    /// `{name: {"t": array of seconds, "value": array, ...}}`: a batch of each `+batched` property in the update as
    /// numpy columns (`decode_columns` of its `Timed<T>` items, `t` unwrapped as in `recv()`).
    fn batches_to_py<'py>(
        &self,
        py: Python<'py>,
        observed: &DynObserved,
    ) -> PyResult<Bound<'py, PyDict>> {
        let batches = PyDict::new(py);
        for s in &observed.samples {
            let Some(series) = &s.series else {
                continue;
            };
            let ty = self.with_observer(|o| {
                o.series_ty(&s.name).cloned().ok_or_else(|| {
                    WireWeaverError::new_err(format!("'{}' is not a +batched property", s.name))
                })
            })?;
            let raw = decode_columns(py, series, &ty, self.res.bundle())?;
            let cols = PyDict::new(py);
            let numpy = py.import("numpy")?;
            let t: Vec<f64> = s
                .items
                .iter()
                .map(|(_, t)| t.as_ref().map_or(f64::NAN, seconds))
                .collect();
            cols.set_item("t", numpy.call_method1("array", (t,))?)?;
            // the device's own (wrapping) times are left out, `t` has them unwrapped
            for (k, v) in raw.iter() {
                let key = k.cast::<PyString>()?.to_str()?.to_string();
                if key != "t" && !key.starts_with("t.") {
                    cols.set_item(key, v)?;
                }
            }
            batches.set_item(&s.name, cols)?;
        }
        Ok(batches)
    }

    fn with_observer<T>(&self, f: impl FnOnce(&mut DynObserver) -> PyResult<T>) -> PyResult<T> {
        let mut guard = self.observer.lock().expect("not poisoned");
        let observer = guard.as_mut().ok_or_else(|| {
            WireWeaverError::new_err(format!("'{}' is not open", self.res.path()))
        })?;
        f(observer)
    }

    /// Wait for one update, checking for Ctrl+C every 100ms, None on timeout.
    fn next_inner(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Option<DynObserved>> {
        let deadline = self::timeout(timeout).map(|t| Instant::now() + t);
        loop {
            let slice = match deadline {
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Ok(None);
                    }
                    left.min(Duration::from_millis(100))
                }
                None => Duration::from_millis(100),
            };
            let got = self.with_observer(|observer| {
                Ok(py.detach(|| {
                    self.conn
                        .runtime
                        .block_on(async { tokio::time::timeout(slice, observer.recv()).await })
                }))
            })?;
            match got {
                Ok(observed) => return observed.map(Some).map_err(stream_err),
                Err(_elapsed) => py.check_signals()?,
            }
        }
    }
}

#[pymethods]
impl Observer {
    /// Wait for the next update, `timeout` in seconds (None waits forever), raises TimeoutError.
    #[pyo3(signature = (timeout=None))]
    fn recv(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Py<PyAny>> {
        match self.next_inner(py, timeout)? {
            Some(observed) => self.observed_to_py(py, observed, false),
            None => Err(PyTimeoutError::new_err(format!(
                "no update from '{}'",
                self.res.path()
            ))),
        }
    }

    /// As `recv()`, with the samples of each `+batched` property (API-39) as numpy columns instead of a list:
    /// `"batches": {name: {"t": array of seconds, "value": array, ...}}`, one array per leaf of the item
    /// (`value.<field>` for a struct, as `decode_columns`). Only batches: a value sent on its own is in `values`.
    #[pyo3(signature = (timeout=None))]
    fn recv_batch(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Py<PyAny>> {
        match self.next_inner(py, timeout)? {
            Some(observed) => self.observed_to_py(py, observed, true),
            None => Err(PyTimeoutError::new_err(format!(
                "no update from '{}'",
                self.res.path()
            ))),
        }
    }

    /// Next update if one is already received, None otherwise.
    fn try_recv(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let observed = self.with_observer(|o| o.try_recv().map_err(stream_err))?;
        observed
            .map(|o| self.observed_to_py(py, o, false))
            .transpose()
    }

    /// Change what this observer asks for (same shape as `observe()`'s `cfg`), its budget stays; returns the new
    /// budget in bytes/s.
    fn configure(&self, py: Python<'_>, cfg: Option<&Bound<'_, PyDict>>) -> PyResult<u32> {
        let (_, entries) = parse_cfg(&self.res, cfg)?;
        let granted = self.with_observer(|o| {
            py.detach(|| o.configure_blocking(entries))
                .map_err(client_err)
        })?;
        Ok(granted.budget)
    }

    /// Bytes per second the level's updates may take on the link, None before the device answered or after `close()`.
    #[getter]
    fn budget(&self) -> Option<u32> {
        self.observer
            .lock()
            .expect("not poisoned")
            .as_ref()?
            .granted()
            .map(|g| g.budget)
    }

    /// Updates dropped because this observer's queue on the host was full.
    #[getter]
    fn dropped(&self) -> u64 {
        self.observer
            .lock()
            .expect("not poisoned")
            .as_ref()
            .map(DynObserver::dropped)
            .unwrap_or_default()
    }

    /// Unsubscribe from the device; also done when the last reference is garbage collected.
    fn close(&self) {
        self.observer.lock().expect("not poisoned").take();
    }

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, _args: &Bound<'_, PyTuple>) -> bool {
        self.close();
        false
    }

    fn __iter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    fn __next__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self.next_inner(py, None) {
            Ok(Some(observed)) => self.observed_to_py(py, observed, false),
            Ok(None) => unreachable!("no timeout"),
            Err(e) => Err(e),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "<observer {}: {}>",
            self.res.path(),
            self.props
                .iter()
                .map(|(_, name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}
