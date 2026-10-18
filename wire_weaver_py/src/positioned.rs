//! Counted and reliable streams (CLT-23): positions, gaps, resume cursors, acknowledgements, see
//! `docs/api/stream_reliability.md`.

use crate::convert::to_py;
use crate::tree::{conn, docs, py_args, repr_with_docs, return_to_py, timeout};
use crate::{Conn, WireWeaverError, client_err, stream_err};
use pyo3::exceptions::{PyTimeoutError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
use shrink_wrap::{UVlq32, UVlq64};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wire_weaver_client::ww_self::{TypeOwned, ValueOwned};
use wire_weaver_client::{
    AckMode, Cause, DynPositionedStream, DynResource, DynResourceKind, DynStreamOpen, Error, Run,
    StreamCursor, StreamError, StreamEvent, seconds,
};

fn item_ty(res: &DynResource) -> TypeOwned {
    match res.kind() {
        Ok(DynResourceKind::Stream { ty, .. }) => ty.clone(),
        _ => unreachable!("PositionedStream wraps a stream"),
    }
}

/// Counted or reliable stream from a device: every item has a position, losses are reported as gaps, a cursor
/// resumes it after a restart. `open()` (or `open(cursor=saved)`), then `recv()` / iterate for values, or
/// `recv_event()` for items with their positions and gaps; `cursor` to save with the data; `close()`. Also a context
/// manager.
#[pyclass(module = "wire_weaver", frozen)]
pub struct PositionedStream {
    pub(crate) res: DynResource,
    pub(crate) conn: Option<Arc<Conn>>,
    pub(crate) stream: Mutex<Option<DynPositionedStream>>,
}

impl PositionedStream {
    fn with_stream<T>(
        &self,
        f: impl FnOnce(&mut DynPositionedStream) -> PyResult<T>,
    ) -> PyResult<T> {
        let mut guard = self.stream.lock().expect("not poisoned");
        let stream = guard.as_mut().ok_or_else(|| {
            WireWeaverError::new_err(format!("'{}' is not open", self.res.path()))
        })?;
        f(stream)
    }

    fn value_to_py(&self, py: Python<'_>, value: &ValueOwned) -> PyResult<Py<PyAny>> {
        let ty = match self.res.kind() {
            Ok(DynResourceKind::Stream { ty, .. }) => ty.clone(),
            _ => unreachable!("PositionedStream wraps a stream"),
        };
        Ok(to_py(py, value, &ty, self.res.bundle())?.unbind())
    }

    /// Wait for one event, checking for Ctrl+C every 100ms, None on timeout.
    fn next_event(
        &self,
        py: Python<'_>,
        timeout: Option<f64>,
    ) -> PyResult<Option<StreamEvent<ValueOwned>>> {
        conn(&self.res, &self.conn)?;
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
            let got =
                self.with_stream(|s| Ok(py.detach(|| s.recv_event_timeout_blocking(slice))))?;
            match got {
                Ok(ev) => return Ok(Some(ev)),
                Err(StreamError::Other(Error::Timeout)) => py.check_signals()?,
                Err(e) => return Err(stream_err(e)),
            }
        }
    }

    fn next_value(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Option<Py<PyAny>>> {
        loop {
            match self.next_event(py, timeout)? {
                Some(StreamEvent::Item { value, .. }) => {
                    return self.value_to_py(py, &value).map(Some);
                }
                Some(_) => continue,
                None => return Ok(None),
            }
        }
    }

    /// Wait for the next whole batch (API-41), checking for Ctrl+C every 100ms, None on timeout. No blocking
    /// variant of `recv_batch` exists on `DynPositionedStream` yet, so this drives the async one through the
    /// connection's runtime, the same way `Stream::recv_inner` (tree.rs) drives `DynStream::recv`.
    fn next_batch(
        &self,
        py: Python<'_>,
        timeout: Option<f64>,
    ) -> PyResult<Option<wire_weaver_client::Batch<ValueOwned>>> {
        let conn = conn(&self.res, &self.conn)?;
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
            let got = self.with_stream(|s| {
                Ok(py.detach(|| {
                    conn.runtime
                        .block_on(async { tokio::time::timeout(slice, s.recv_batch()).await })
                }))
            })?;
            match got {
                Ok(batch) => return batch.map(Some).map_err(stream_err),
                Err(_elapsed) => py.check_signals()?,
            }
        }
    }

    fn event_to_py(&self, py: Python<'_>, ev: StreamEvent<ValueOwned>) -> PyResult<Py<PyAny>> {
        let d = PyDict::new(py);
        match ev {
            StreamEvent::Item {
                pos,
                units,
                t,
                run,
                value,
                ..
            } => {
                d.set_item("kind", "item")?;
                d.set_item("pos", pos)?;
                d.set_item("units", units)?;
                d.set_item("t", t.as_ref().map(seconds))?;
                d.set_item("run", run_name(run))?;
                d.set_item("value", self.value_to_py(py, &value)?)?;
            }
            StreamEvent::Gap {
                run,
                at,
                units,
                cause,
            } => {
                d.set_item("kind", "gap")?;
                d.set_item("at", at)?;
                d.set_item("units", units)?;
                d.set_item(
                    "cause",
                    match cause {
                        Cause::Device => "device",
                        Cause::Link => "link",
                        Cause::Store => "store",
                        Cause::Host => "host",
                    },
                )?;
                d.set_item("run", run_name(run))?;
            }
            StreamEvent::Resumed {
                cursor,
                session,
                ended,
            } => {
                d.set_item("kind", "resumed")?;
                d.set_item("cursor", (cursor.session.0, cursor.pos.0))?;
                d.set_item("session", session)?;
                d.set_item("ended", ended)?;
            }
            StreamEvent::BackfillDone => d.set_item("kind", "backfill_done")?,
            StreamEvent::Disconnected => d.set_item("kind", "disconnected")?,
            StreamEvent::Closed => d.set_item("kind", "closed")?,
        }
        Ok(d.into_any().unbind())
    }
}

fn run_name(run: Run) -> &'static str {
    match run {
        Run::Live => "live",
        Run::Backfill => "backfill",
    }
}

#[pymethods]
impl PositionedStream {
    /// Open the stream by calling its stream function with `args` (positional or by keyword), or resume it from
    /// `cursor` (a `(session, pos)` tuple saved from `cursor`; the device keeps the session's arguments). Returns
    /// the function's result (its `Ok` value), raises `RemoteError` for an `Err`. `ack`: `"receive"` (a reliable
    /// stream's data counts as received when `recv()` returns it) or `"explicit"` (only when `ack(pos)` confirms
    /// it, e.g. after writing it to disk).
    #[pyo3(signature = (*args, cursor=None, ack="receive", **kwargs))]
    fn open(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        cursor: Option<(u32, u64)>,
        ack: &str,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let conn = conn(&self.res, &self.conn)?;
        let ack_mode = match ack {
            "receive" => AckMode::OnReceive,
            "explicit" => AckMode::Explicit,
            other => {
                return Err(PyValueError::new_err(format!(
                    "ack is \"receive\" or \"explicit\", not {other:?}"
                )));
            }
        };
        let (defs, return_ty) = self.res.stream_fn().unwrap_or_default();
        let return_ty = return_ty.cloned();
        let values = match cursor {
            Some(_) => vec![],
            None => py_args(&self.res, defs, args, kwargs)?,
        };
        let resume = cursor.map(|(session, pos)| StreamCursor {
            session: UVlq32(session),
            pos: UVlq64(pos),
        });
        let mut guard = self.stream.lock().expect("not poisoned");
        if guard.is_some() {
            return Err(WireWeaverError::new_err(format!(
                "'{}' is open already",
                self.res.path()
            )));
        }
        let res = &self.res;
        let opened = py
            .detach(|| res.positioned_stream_blocking(conn.client.cmd(), &values, ack_mode, resume))
            .map_err(client_err)?;
        let is_result = return_ty
            .as_ref()
            .is_some_and(|ty| matches!(ty.get_in_line(res.bundle()), Ok(TypeOwned::Result { .. })));
        let value = match opened {
            DynStreamOpen::Open { result, stream } => {
                *guard = Some(*stream);
                match (result, is_result) {
                    (Some(v), true) => Some(ValueOwned::Result(Ok(Box::new(v)))),
                    (result, _) => result,
                }
            }
            DynStreamOpen::Rejected(e) => Some(ValueOwned::Result(Err(Box::new(e)))),
        };
        drop(guard);
        return_to_py(py, res, value, return_ty.as_ref())
    }

    /// Close the stream on the device.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        let stream = self.stream.lock().expect("not poisoned").take();
        if let Some(stream) = stream {
            py.detach(|| stream.close_blocking()).map_err(stream_err)?;
        }
        Ok(())
    }

    /// Wait for the next value (gaps are skipped, see `stats`), `timeout` in seconds, raises TimeoutError.
    #[pyo3(signature = (timeout=None))]
    fn recv(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Py<PyAny>> {
        self.next_value(py, timeout)?
            .ok_or_else(|| PyTimeoutError::new_err(format!("no item from '{}'", self.res.path())))
    }

    /// Wait for the next event, a dict with `kind`: `"item"` (`pos`, `units`, `t` in seconds of device time or
    /// None, `run`: `"live"` / `"backfill"`, `value`), `"gap"` (`at`, `units`, `cause`: `"device"`, `"link"`,
    /// `"store"`, `"host"`, `run`), `"resumed"` (`cursor`, `session`, `ended`), `"backfill_done"`,
    /// `"disconnected"`, `"closed"`. Raises TimeoutError.
    #[pyo3(signature = (timeout=None))]
    fn recv_event(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Py<PyAny>> {
        match self.next_event(py, timeout)? {
            Some(ev) => self.event_to_py(py, ev),
            None => Err(PyTimeoutError::new_err(format!(
                "no event from '{}'",
                self.res.path()
            ))),
        }
    }

    /// Wait for the rest of the next batch, or the next one whole (API-41): a `dict` with `pos`, `t` (seconds of
    /// device time, an array, `None` for an untimed stream) and the item type's own fields, each a numpy array
    /// over the batch's items. An item of a stream that isn't batched is a batch of one. Raises TimeoutError.
    #[pyo3(signature = (timeout=None))]
    fn recv_batch<'py>(
        &self,
        py: Python<'py>,
        timeout: Option<f64>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let batch = self.next_batch(py, timeout)?.ok_or_else(|| {
            PyTimeoutError::new_err(format!("no batch from '{}'", self.res.path()))
        })?;
        crate::columns::batch_dict(
            py,
            Some(batch.pos),
            &batch.items,
            &item_ty(&self.res),
            self.res.bundle(),
        )
    }

    /// Where to resume from after a restart, `(session, pos)`; save it with the data. None before opening.
    #[getter]
    fn cursor(&self) -> PyResult<Option<(u32, u64)>> {
        self.with_stream(|s| Ok(s.cursor().map(|c| (c.session.0, c.pos.0))))
    }

    /// Confirm every unit before `pos` (an item's `pos + units`), with `open(ack="explicit")`.
    fn ack(&self, pos: u64) -> PyResult<()> {
        self.with_stream(|s| {
            s.ack(pos);
            Ok(())
        })
    }

    /// What happened to the data: received, lost by cause, duplicates, ... as a dict.
    #[getter]
    fn stats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let st = self.with_stream(|s| Ok(s.stats()))?;
        let d = PyDict::new(py);
        d.set_item("received", st.received)?;
        d.set_item("lost_device", st.lost_device)?;
        d.set_item("lost_link", st.lost_link)?;
        d.set_item("lost_store", st.lost_store)?;
        d.set_item("lost_host", st.lost_host)?;
        d.set_item("duplicates", st.duplicates)?;
        d.set_item("late", st.late)?;
        d.set_item("out_of_order", st.out_of_order)?;
        d.set_item("resend_requests", st.resend_requests)?;
        d.set_item("queued", st.queued)?;
        d.set_item("queued_bytes", st.queued_bytes)?;
        Ok(d)
    }

    /// `"counted"` or `"reliable"`.
    #[getter]
    fn delivery(&self) -> String {
        self.res
            .delivery()
            .map(|d| format!("{d:?}").to_lowercase())
            .unwrap_or_default()
    }

    fn __iter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    fn __next__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(self.next_value(py, None)?.expect("no timeout"))
    }

    fn __enter__(slf: Py<Self>, py: Python<'_>) -> PyResult<Py<Self>> {
        if slf.get().stream.lock().expect("not poisoned").is_none() {
            let args = PyTuple::empty(py);
            slf.get().open(py, &args, None, "receive", None)?;
        }
        Ok(slf)
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, PyTuple>) -> PyResult<bool> {
        self.close(py)?;
        Ok(false)
    }

    fn __repr__(&self) -> String {
        repr_with_docs("stream", &self.res)
    }

    #[getter]
    fn doc(&self) -> String {
        docs(&self.res)
    }
}
