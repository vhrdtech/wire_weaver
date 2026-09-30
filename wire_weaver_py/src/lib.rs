//! Python module talking to any WireWeaver device, without generated code: the API tree (traits, methods,
//! properties, streams) is built at runtime from the device introspection data, or from an API crate.
//!
//! ```python
//! import wire_weaver as ww
//!
//! dev = ww.connect(vid_pid=(0xc0de, 0xcafe))
//! dev                                  # resource tree with signatures
//! dev.led_on()
//! dev.channel[1].gain.write(0.5)
//! with dev.samples as samples:
//!     for s in samples: ...
//! ```
//!
//! See `convert` for how values map to Python objects.

mod convert;
mod tree;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTimeoutError, PyValueError};
use pyo3::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tree::{Array, Method, Node, Property, Sink, Stream};
use wire_weaver_client::{ClientConfig, DynClient, DynResource, Error, StreamError};

create_exception!(
    wire_weaver,
    WireWeaverError,
    PyException,
    "Connection, protocol or device error."
);
create_exception!(
    wire_weaver,
    RemoteError,
    WireWeaverError,
    "A method returned `Err(e)`, `e` is in the `value` attribute."
);

pub(crate) fn client_err(e: Error) -> PyErr {
    match e {
        Error::Timeout => PyTimeoutError::new_err("no response from device"),
        e => WireWeaverError::new_err(err_msg(e)),
    }
}

/// Error message without the `Other error: '..'` wrapping.
pub(crate) fn err_msg(e: Error) -> String {
    match e {
        Error::Other(msg) => msg,
        e => e.to_string(),
    }
}

pub(crate) fn stream_err(e: StreamError) -> PyErr {
    match e {
        StreamError::Other(e) => client_err(e),
        e => WireWeaverError::new_err(e.to_string()),
    }
}

/// Connection shared by all resource objects of a device, the client is dropped (and disconnected) before the
/// runtime running its event loop.
pub(crate) struct Conn {
    pub(crate) client: DynClient,
    pub(crate) runtime: tokio::runtime::Runtime,
    /// Request timeout, None for the client default.
    pub(crate) timeout: Option<Duration>,
}

/// Connected device, its API resources are attributes: `dev.led_on()`, `dev.channel[0].gain.read()`.
/// Use `dev["name"]` for a resource whose name clashes with one of the methods below.
#[pyclass(module = "wire_weaver", frozen)]
struct Device {
    root: Node,
    conn: Arc<Conn>,
}

#[pymethods]
impl Device {
    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        self.root.child(py, name)
    }

    fn __getitem__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        self.root
            .child(py, name)
            .map_err(|e| pyo3::exceptions::PyKeyError::new_err(e.value(py).to_string()))
    }

    fn __dir__(&self) -> Vec<String> {
        let mut names = self.root.names();
        names.extend(["api", "info", "disconnect"].map(String::from));
        names
    }

    fn __repr__(&self) -> String {
        self.root.tree()
    }

    /// API root, same resources as the device itself.
    #[getter]
    fn api(&self) -> Node {
        Node {
            res: self.root.res.clone(),
            conn: self.root.conn.clone(),
        }
    }

    /// API, link and protocol versions reported by the device.
    #[getter]
    fn info(&self) -> String {
        let info = self.conn.client.device_api_info();
        format!(
            "API: {:?}\nAPI model: {:?}\nLink: {:?}\nMax message size: {}",
            info.user_api_version, info.api_model_version, info.link_version, info.max_message_size
        )
    }

    /// Disconnect from the device, also done when the last object referring to it is garbage collected.
    fn disconnect(&self, py: Python<'_>) -> PyResult<()> {
        let conn = &self.conn;
        py.detach(|| conn.client.disconnect().blocking())
            .map_err(client_err)
    }

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, pyo3::types::PyTuple>) -> PyResult<bool> {
        self.disconnect(py)?;
        Ok(false)
    }
}

/// Connect to a device and build its API from introspection data (or from `api`, see `load_api`).
///
/// Filters select which device to connect to, a USB device is used unless `rtt` is given. `timeout` is the default
/// request timeout in seconds.
#[pyfunction]
#[pyo3(signature = (
    *,
    vid_pid=None,
    serial=None,
    serial_contains=None,
    user_label=None,
    product_contains=None,
    manufacturer_contains=None,
    implements_api=None,
    rtt=None,
    rtt_speed_hz=None,
    rtt_elf=None,
    timeout=None,
    api=None,
))]
#[allow(clippy::too_many_arguments)]
fn connect(
    py: Python<'_>,
    vid_pid: Option<(u16, u16)>,
    serial: Option<String>,
    serial_contains: Option<String>,
    user_label: Option<String>,
    product_contains: Option<String>,
    manufacturer_contains: Option<String>,
    implements_api: Option<(String, String)>,
    rtt: Option<String>,
    rtt_speed_hz: Option<u32>,
    rtt_elf: Option<PathBuf>,
    timeout: Option<f64>,
    api: Option<PyRef<'_, Node>>,
) -> PyResult<Device> {
    let mut config = ClientConfig::new();
    if let Some(target) = rtt {
        config = config.rtt(target, rtt_speed_hz);
        if let Some(elf) = rtt_elf {
            config = config.rtt_elf(elf);
        }
    } else {
        config = config.usb();
    }
    if let Some((vid, pid)) = vid_pid {
        config = config.usb_vid_pid(vid, pid);
    }
    if let Some(s) = serial {
        config = config.serial_eq(s);
    }
    if let Some(s) = serial_contains {
        config = config.serial_contains(s);
    }
    if let Some(s) = user_label {
        config = config.user_label_eq(s);
    }
    if let Some(s) = product_contains {
        config = config.product_contains(s);
    }
    if let Some(s) = manufacturer_contains {
        config = config.manufacturer_contains(s);
    }
    if let Some((name, req)) = implements_api {
        let req = semver_req(&req)?;
        config = config.implements_api(name, req);
    }
    let timeout = timeout.map(Duration::from_secs_f64);
    if let Some(t) = timeout {
        config = config.default_timeout(t);
    }
    let api_bundle = api.map(|node| node.res.bundle().clone());

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let (client, bundle) = py.detach(|| -> PyResult<_> {
        let _guard = runtime.enter();
        let client = DynClient::from_config(config)
            .connect_blocking()
            .map_err(|e| WireWeaverError::new_err(format!("{e:#}")))?;
        let bundle = match api_bundle {
            Some(bundle) => bundle,
            None => match client.device_introspect() {
                Some(introspect) => introspect.api_bundle.clone(),
                None => Arc::new(
                    client
                        .cmd()
                        .introspect()
                        .get_blocking()
                        .map_err(|e| WireWeaverError::new_err(format!("{e:#}")))?
                        .ok_or_else(|| {
                            WireWeaverError::new_err(
                                "device has introspection disabled, pass its API: \
                                 connect(api=wire_weaver.load_api(\"path/to/api_crate\"))",
                            )
                        })?,
                ),
            },
        };
        Ok((client, bundle))
    })?;
    let conn = Arc::new(Conn {
        client,
        runtime,
        timeout,
    });
    Ok(Device {
        root: Node {
            res: DynResource::root(bundle),
            conn: Some(conn.clone()),
        },
        conn,
    })
}

fn semver_req(req: &str) -> PyResult<wire_weaver_client::ww_version::semver::VersionReq> {
    req.parse()
        .map_err(|e| PyValueError::new_err(format!("version requirement '{req}': {e}")))
}

/// Load an API from its crate source (`path` to the directory with Cargo.toml), without a device.
/// `trait_name` selects the trait when there are several. Pass the result to `connect(api=...)` for a device with
/// introspection disabled, or use it offline to browse the API and `encode` / `decode` values.
#[pyfunction]
#[pyo3(signature = (path, trait_name=None))]
fn load_api(py: Python<'_>, path: PathBuf, trait_name: Option<String>) -> PyResult<Node> {
    let bundle = py
        .detach(|| wire_weaver_core::load(&path, trait_name, false))
        .map_err(|e| WireWeaverError::new_err(format!("{e:#}")))?;
    Ok(Node {
        res: DynResource::root(Arc::new(bundle)),
        conn: None,
    })
}

/// Connected USB WireWeaver devices, one line each.
#[pyfunction]
fn list_devices(py: Python<'_>) -> PyResult<Vec<String>> {
    py.detach(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let devices = runtime
            .block_on(wire_weaver_client::list_usb_devices())
            .map_err(|e| WireWeaverError::new_err(format!("{e:#}")))?;
        Ok(devices.iter().map(|d| d.to_string()).collect())
    })
}

#[pymodule]
#[pyo3(name = "wire_weaver")]
fn wire_weaver_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(connect, m)?)?;
    m.add_function(wrap_pyfunction!(load_api, m)?)?;
    m.add_function(wrap_pyfunction!(list_devices, m)?)?;
    m.add_class::<Device>()?;
    m.add_class::<Node>()?;
    m.add_class::<Array>()?;
    m.add_class::<Method>()?;
    m.add_class::<Property>()?;
    m.add_class::<Stream>()?;
    m.add_class::<Sink>()?;
    m.add("WireWeaverError", m.py().get_type::<WireWeaverError>())?;
    m.add("RemoteError", m.py().get_type::<RemoteError>())?;
    Ok(())
}
